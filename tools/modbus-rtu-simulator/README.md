# Platform-driven Modbus RTU simulator

This is a Windows-only development helper. It is not linked into the gateway
binary and does not create gateway devices, profiles, or point mappings.

The simulator reads `snapshot` and `thing-models` from the running local
gateway. Those endpoints already reflect the result of the platform sync
button. A device responds only when the platform has assigned it to a mapped
channel and marked it enabled.

## Virtual port pairs

Create two paired virtual ports using a Windows virtual serial-port driver:

| Gateway channel | Gateway port | Simulator port |
| --- | --- | --- |
| `rs485-1` | `COM10` | `COM11` |
| `rs485-2` | `COM12` | `COM13` |

Set the gateway channel ports to `COM10` and `COM12` in the gateway UI. The
simulator defaults in `simulator.json` use the opposite ends, `COM11` and
`COM13`. Both sides use `9600 8N1`, matching the current production gateway
serial transport.

## Start

```powershell
py -3 -m pip install -r requirements.txt
py -3 modbus_rtu_simulator.py
```

Run `py -3 modbus_rtu_simulator.py --self-test` to verify the Modbus RTU CRC
and register encoding without opening a serial port.

## Values and faults

`simulator.json` controls only emitted measurements and deliberate fault
injection. Add per-device overrides under `values.devices.<device SN>` after a
device exists on the platform. The physical/device model definition remains
owned by the platform sync result.

For fault testing, add a device entry under `faults.devices` with any of:
`online`, `drop_response`, `delay_ms`, or `bad_crc`.
