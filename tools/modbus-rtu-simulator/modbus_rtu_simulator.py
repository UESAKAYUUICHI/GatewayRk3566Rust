#!/usr/bin/env python3
"""Modbus RTU device simulator driven by platform-synced gateway configuration.

The program is intentionally independent from the gateway.  It opens the other
end of Windows virtual COM-port pairs and behaves as physical Modbus RTU slaves.
"""

from __future__ import annotations

import argparse
import json
import math
import signal
import struct
import threading
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import serial


def crc16(frame: bytes) -> int:
    value = 0xFFFF
    for byte in frame:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ 0xA001 if value & 1 else value >> 1
    return value


def append_crc(frame: bytes) -> bytes:
    return frame + struct.pack("<H", crc16(frame))


def frame_crc_valid(frame: bytes) -> bool:
    return len(frame) >= 4 and crc16(frame[:-2]) == struct.unpack("<H", frame[-2:])[0]


def fetch_json(url: str) -> Any:
    request = urllib.request.Request(url, headers={"Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=5) as response:
        return json.loads(response.read().decode("utf-8"))


@dataclass(frozen=True)
class Point:
    code: str
    address: int
    quantity: int
    data_type: str
    scale: float
    offset: float


@dataclass(frozen=True)
class Device:
    sn: str
    name: str
    channel_id: str
    slave_id: int
    points: tuple[Point, ...]


class PlatformInventory:
    def __init__(self, api_url: str, channel_ids: set[str]) -> None:
        self.api_url = api_url.rstrip("/")
        self.channel_ids = channel_ids

    def load(self) -> dict[str, dict[int, Device]]:
        snapshot = fetch_json(f"{self.api_url}/snapshot")
        models = fetch_json(f"{self.api_url}/thing-models")
        model_points: dict[str, tuple[Point, ...]] = {}
        for model in models:
            points = tuple(
                Point(
                    code=item["code"],
                    address=int(item["address"]),
                    quantity=int(item["quantity"]),
                    data_type=item["dataType"].lower(),
                    scale=float(item["scale"]),
                    offset=float(item["offset"]),
                )
                for item in model.get("points", [])
                if int(item.get("functionCode", 0)) == 3
            )
            model_points[model["profile"]] = points

        result: dict[str, dict[int, Device]] = {channel_id: {} for channel_id in self.channel_ids}
        for meter in snapshot.get("meters", []):
            channel_id = meter.get("channelId", "")
            if channel_id not in result or not meter.get("enabled", False):
                continue
            profile = meter.get("profile", "")
            points = model_points.get(profile, ())
            if not points:
                continue
            device = Device(
                sn=meter["sn"],
                name=meter.get("name", meter["sn"]),
                channel_id=channel_id,
                slave_id=int(meter["address"]),
                points=points,
            )
            result[channel_id][device.slave_id] = device
        return result


class DeviceCatalog:
    def __init__(self) -> None:
        self._lock = threading.Lock()
        self._channels: dict[str, dict[int, Device]] = {}

    def replace(self, channels: dict[str, dict[int, Device]]) -> None:
        with self._lock:
            self._channels = channels

    def find(self, channel_id: str, slave_id: int) -> Device | None:
        with self._lock:
            return self._channels.get(channel_id, {}).get(slave_id)

    def summary(self) -> str:
        with self._lock:
            entries = [
                f"{channel}/{slave}:{device.sn}"
                for channel, devices in self._channels.items()
                for slave, device in devices.items()
            ]
        return ", ".join(entries) if entries else "no enabled platform-synced devices"


class ValueSource:
    def __init__(self, values: dict[str, Any], started_at: float) -> None:
        self.values = values
        self.started_at = started_at

    def get(self, device: Device, point: Point) -> float:
        device_values = self.values.get("devices", {}).get(device.sn, {})
        spec = device_values.get(point.code, self.values.get("default", {}).get(point.code, 0))
        if isinstance(spec, (int, float)):
            return float(spec)
        if not isinstance(spec, dict):
            return 0.0
        mode = spec.get("mode", "fixed")
        base = float(spec.get("base", spec.get("value", 0)))
        elapsed = time.monotonic() - self.started_at
        if mode == "sine":
            period = max(float(spec.get("period_seconds", 60)), 1.0)
            return base + float(spec.get("amplitude", 0)) * math.sin(2 * math.pi * elapsed / period)
        if mode == "counter":
            return base + float(spec.get("per_hour", 0)) * elapsed / 3600
        return base


class FaultSource:
    def __init__(self, faults: dict[str, Any]) -> None:
        self.faults = faults.get("devices", {})

    def for_device(self, device: Device) -> dict[str, Any]:
        return self.faults.get(device.sn, {})


def encode_words(point: Point, value: float) -> list[int]:
    if point.scale == 0:
        raise ValueError(f"{point.code}: scale cannot be zero")
    raw = round((value - point.offset) / point.scale)
    if point.data_type == "u16":
        return [raw & 0xFFFF]
    if point.data_type == "u32":
        raw &= 0xFFFFFFFF
    elif point.data_type == "i32":
        raw &= 0xFFFFFFFF
    else:
        raise ValueError(f"unsupported data type {point.data_type}")
    return [(raw >> 16) & 0xFFFF, raw & 0xFFFF]


def registers_for(device: Device, values: ValueSource) -> dict[int, int]:
    registers: dict[int, int] = {}
    for point in device.points:
        words = encode_words(point, values.get(device, point))
        for offset, word in enumerate(words):
            registers[point.address + offset] = word
    return registers


class ChannelWorker(threading.Thread):
    def __init__(
        self,
        channel: dict[str, Any],
        catalog: DeviceCatalog,
        values: ValueSource,
        faults: FaultSource,
        stopping: threading.Event,
    ) -> None:
        super().__init__(name=f"modbus-{channel['gateway_channel_id']}", daemon=True)
        self.channel = channel
        self.catalog = catalog
        self.values = values
        self.faults = faults
        self.stopping = stopping

    def run(self) -> None:
        channel_id = self.channel["gateway_channel_id"]
        port = self.channel["port"]
        baud = int(self.channel.get("baud", 9600))
        announced_waiting = False
        while not self.stopping.is_set():
            try:
                with serial.Serial(port, baudrate=baud, bytesize=8, parity="N", stopbits=1, timeout=0.05) as connection:
                    print(f"[{channel_id}] listening on {port} at {baud} 8N1", flush=True)
                    announced_waiting = False
                    pending = bytearray()
                    while not self.stopping.is_set():
                        received = connection.read(connection.in_waiting or 1)
                        if received:
                            pending.extend(received)
                        self._consume(connection, pending)
            except serial.SerialException as error:
                if not announced_waiting:
                    print(f"[{channel_id}] waiting for {port}: {error}", flush=True)
                    announced_waiting = True
                self.stopping.wait(3)

    def _consume(self, connection: serial.Serial, pending: bytearray) -> None:
        while len(pending) >= 8:
            candidate = bytes(pending[:8])
            if not frame_crc_valid(candidate):
                del pending[0]
                continue
            del pending[:8]
            self._respond(connection, candidate)

    def _respond(self, connection: serial.Serial, request: bytes) -> None:
        channel_id = self.channel["gateway_channel_id"]
        slave_id, function = request[0], request[1]
        address, quantity = struct.unpack(">HH", request[2:6])
        device = self.catalog.find(channel_id, slave_id)
        if device is None:
            return
        fault = self.faults.for_device(device)
        if not fault.get("online", True) or fault.get("drop_response", False):
            print(f"[{channel_id}] slave {slave_id} drops request", flush=True)
            return
        delay_ms = float(fault.get("delay_ms", 0))
        if delay_ms:
            time.sleep(delay_ms / 1000)
        if function != 3:
            self._write_exception(connection, slave_id, function, 1)
            return
        if quantity < 1 or quantity > 125:
            self._write_exception(connection, slave_id, function, 3)
            return
        words_by_address = registers_for(device, self.values)
        words = [words_by_address.get(address + offset) for offset in range(quantity)]
        if any(word is None for word in words):
            self._write_exception(connection, slave_id, function, 2)
            return
        payload = b"".join(struct.pack(">H", int(word)) for word in words)
        response = append_crc(bytes([slave_id, function, len(payload)]) + payload)
        if fault.get("bad_crc", False):
            response = response[:-1] + bytes([response[-1] ^ 0xFF])
        connection.write(response)
        connection.flush()
        print(
            f"[{channel_id}] {device.sn} slave={slave_id} read 0x{address:04X} x{quantity} -> {len(payload)} bytes",
            flush=True,
        )

    @staticmethod
    def _write_exception(connection: serial.Serial, slave_id: int, function: int, code: int) -> None:
        connection.write(append_crc(bytes([slave_id, function | 0x80, code])))
        connection.flush()


def verify_protocol() -> None:
    request = append_crc(bytes([1, 3, 0, 0, 0, 2]))
    assert request.hex() == "010300000002c40b"
    assert frame_crc_valid(request)
    point = Point("voltage_a", 0, 2, "u32", 0.1, 0)
    assert encode_words(point, 230.1) == [0, 2301]
    signed = Point("active_power_total", 18, 2, "i32", 0.01, 0)
    assert encode_words(signed, -1.0) == [65535, 65436]
    print("protocol self-test passed")


def main() -> None:
    parser = argparse.ArgumentParser(description="Platform-driven Modbus RTU serial device simulator")
    parser.add_argument("--config", type=Path, default=Path(__file__).with_name("simulator.json"))
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    verify_protocol()
    if args.self_test:
        return

    config = json.loads(args.config.read_text(encoding="utf-8"))
    channels = [item for item in config.get("channels", []) if item.get("enabled", True)]
    if not channels:
        raise SystemExit("No enabled simulator channels in configuration")
    channel_ids = {item["gateway_channel_id"] for item in channels}
    catalog = DeviceCatalog()
    source = PlatformInventory(config["gateway_api"], channel_ids)
    values = ValueSource(config.get("values", {}), time.monotonic())
    faults = FaultSource(config.get("faults", {}))
    stopping = threading.Event()

    def reload_inventory() -> None:
        while not stopping.is_set():
            try:
                catalog.replace(source.load())
                print(f"platform inventory: {catalog.summary()}", flush=True)
            except Exception as error:  # Keep serving the last good platform inventory.
                print(f"platform inventory refresh failed: {error}", flush=True)
            stopping.wait(max(float(config.get("refresh_seconds", 10)), 1))

    signal.signal(signal.SIGINT, lambda *_: stopping.set())
    signal.signal(signal.SIGTERM, lambda *_: stopping.set())
    threading.Thread(target=reload_inventory, name="platform-inventory", daemon=True).start()
    workers = [ChannelWorker(channel, catalog, values, faults, stopping) for channel in channels]
    for worker in workers:
        worker.start()
    while not stopping.wait(0.5):
        pass


if __name__ == "__main__":
    main()
