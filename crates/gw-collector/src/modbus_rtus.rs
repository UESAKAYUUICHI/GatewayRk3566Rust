//! Modbus RTU 真驱动（RS485 串口）。
//!
//! 使用严格的请求/响应整帧校验，不依赖会逐字节丢弃并重同步的通用
//! decoder。串口线上出现残留字节、错位帧、非法功能码或 CRC 错误时，
//! 整次采样失败，禁止把“恢复解码”的结果送入正式计量链路。

use async_trait::async_trait;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::{
    ClearBuffer, DataBits, Parity, SerialPort, SerialPortBuilderExt, SerialStream, StopBits,
};

use crate::transport::{MeterTransport, TransportError, TransportResult};

/// 串口参数（默认 9600 8N1，主流导轨表出厂配置）。
#[derive(Debug, Clone, Deserialize)]
pub struct SerialConfig {
    /// 串口设备路径，如 /dev/ttyS3、/dev/ttyUSB0 或 Windows COM3
    pub port: String,
    #[serde(default = "default_baud")]
    pub baud: u32,
    #[serde(default = "default_data_bits")]
    pub data_bits: u8,
    #[serde(default = "default_stop_bits")]
    pub stop_bits: u8,
    #[serde(default)]
    pub parity: String,
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
    #[serde(default = "default_retry_count")]
    pub retry_count: u32,
}

fn default_baud() -> u32 {
    9600
}
fn default_data_bits() -> u8 {
    8
}
fn default_stop_bits() -> u8 {
    1
}
fn default_timeout_ms() -> u64 {
    1_000
}
fn default_retry_count() -> u32 {
    2
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud: default_baud(),
            data_bits: default_data_bits(),
            stop_bits: default_stop_bits(),
            parity: "NONE".to_string(),
            timeout_ms: default_timeout_ms(),
            retry_count: default_retry_count(),
        }
    }
}

/// 持有一条串口连接的严格 RTU 主站。
pub struct ModbusRtuTransport {
    stream: SerialStream,
    timeout: std::time::Duration,
    retry_count: u32,
}

impl ModbusRtuTransport {
    pub async fn connect(serial: &SerialConfig) -> Result<Self, TransportError> {
        let data_bits = match serial.data_bits {
            5 => DataBits::Five,
            6 => DataBits::Six,
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        };
        let stop_bits = match serial.stop_bits {
            2 => StopBits::Two,
            _ => StopBits::One,
        };
        let parity = match serial.parity.trim().to_ascii_uppercase().as_str() {
            "EVEN" => Parity::Even,
            "ODD" => Parity::Odd,
            _ => Parity::None,
        };
        let builder = tokio_serial::new(&serial.port, serial.baud)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .parity(parity);
        let stream = builder
            .open_native_async()
            .map_err(|e| TransportError::Io(format!("打开串口 {} 失败: {e}", serial.port)))?;
        Ok(Self {
            stream,
            timeout: std::time::Duration::from_millis(serial.timeout_ms.max(100)),
            retry_count: serial.retry_count,
        })
    }

    fn clear_input(&self) -> TransportResult<()> {
        self.stream
            .clear(ClearBuffer::Input)
            .map_err(|e| TransportError::Io(format!("清理串口输入缓冲失败: {e}")))
    }

    async fn read_exact_timeout(&mut self, bytes: &mut [u8]) -> TransportResult<()> {
        tokio::time::timeout(self.timeout, self.stream.read_exact(bytes))
            .await
            .map_err(|_| TransportError::NoResponse)?
            .map(|_| ())
            .map_err(|e| TransportError::Io(format!("读取 RTU 响应失败: {e}")))
    }

    async fn transact(
        &mut self,
        request: &[u8],
        slave: u8,
        function_code: u8,
        quantity: Option<u16>,
        address: u16,
    ) -> TransportResult<Vec<u8>> {
        self.clear_input()?;
        self.stream
            .write_all(request)
            .await
            .map_err(|e| TransportError::Io(format!("发送 RTU 请求失败: {e}")))?;
        self.stream
            .flush()
            .await
            .map_err(|e| TransportError::Io(format!("刷新串口发送缓冲失败: {e}")))?;

        let mut prefix = [0u8; 3];
        self.read_exact_timeout(&mut prefix).await?;
        if prefix[0] != slave {
            return Err(TransportError::CorruptFrame(format!(
                "从站号不匹配 @fc{function_code}/0x{address:04X}: expected {slave}, got {}",
                prefix[0]
            )));
        }

        let response_function = prefix[1];
        if response_function == (function_code | 0x80) {
            let mut crc = [0u8; 2];
            self.read_exact_timeout(&mut crc).await?;
            let mut frame = prefix.to_vec();
            frame.extend_from_slice(&crc);
            validate_crc(&frame)?;
            return Err(TransportError::Exception(format!(
                "fc{function_code}/slave{slave}/0x{address:04X}: exception code 0x{:02X}",
                prefix[2]
            )));
        }
        if response_function != function_code {
            return Err(TransportError::CorruptFrame(format!(
                "功能码不匹配 @slave{slave}/0x{address:04X}: expected 0x{function_code:02X}, got 0x{response_function:02X}"
            )));
        }

        let byte_count = usize::from(prefix[2]);
        if let Some(quantity) = quantity {
            let expected = usize::from(quantity) * 2;
            if byte_count != expected {
                return Err(TransportError::CorruptFrame(format!(
                    "字节数不匹配 @slave{slave}/0x{address:04X}: expected {expected}, got {byte_count}"
                )));
            }
        }
        let mut frame = prefix.to_vec();
        let mut body_and_crc = vec![0u8; byte_count + 2];
        self.read_exact_timeout(&mut body_and_crc).await?;
        frame.extend_from_slice(&body_and_crc);
        validate_crc(&frame)?;
        Ok(frame)
    }

    async fn transact_write(
        &mut self,
        request: &[u8],
        slave: u8,
        address: u16,
    ) -> TransportResult<Vec<u8>> {
        self.clear_input()?;
        self.stream
            .write_all(request)
            .await
            .map_err(|e| TransportError::Io(format!("发送 RTU 写请求失败: {e}")))?;
        self.stream
            .flush()
            .await
            .map_err(|e| TransportError::Io(format!("刷新串口发送缓冲失败: {e}")))?;

        let mut frame = [0u8; 8];
        self.read_exact_timeout(&mut frame).await?;
        if frame[0] != slave || frame[1] != 6 {
            return Err(TransportError::CorruptFrame(format!(
                "写入响应头不匹配 @slave{slave}/0x{address:04X}: {:02X?}",
                &frame[..2]
            )));
        }
        validate_crc(&frame)?;
        Ok(frame.to_vec())
    }
}

#[async_trait]
impl MeterTransport for ModbusRtuTransport {
    async fn read_registers(
        &mut self,
        function_code: u8,
        slave: u8,
        address: u16,
        quantity: u16,
    ) -> TransportResult<Vec<u16>> {
        if function_code != 3 && function_code != 4 {
            return Err(TransportError::Exception(format!(
                "unsupported function code {function_code} @slave{slave}/0x{address:04X}"
            )));
        }
        let mut request = vec![
            slave,
            function_code,
            (address >> 8) as u8,
            address as u8,
            (quantity >> 8) as u8,
            quantity as u8,
        ];
        append_crc(&mut request);

        let mut last_error = None;
        for attempt in 0..=self.retry_count {
            match self
                .transact(&request, slave, function_code, Some(quantity), address)
                .await
            {
                Ok(frame) => {
                    let words = frame[3..frame.len() - 2]
                        .chunks_exact(2)
                        .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                        .collect::<Vec<_>>();
                    if words.len() == usize::from(quantity) {
                        return Ok(words);
                    }
                    last_error = Some(TransportError::BadLength {
                        want: quantity,
                        got: words.len(),
                    });
                }
                Err(error) => last_error = Some(error),
            }
            let _ = self.clear_input();
            if attempt < self.retry_count {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        Err(last_error.unwrap_or(TransportError::NoResponse))
    }

    async fn write_single_register(
        &mut self,
        slave: u8,
        address: u16,
        value: u16,
    ) -> TransportResult<()> {
        let mut request = vec![
            slave,
            6,
            (address >> 8) as u8,
            address as u8,
            (value >> 8) as u8,
            value as u8,
        ];
        append_crc(&mut request);

        let mut last_error = None;
        for attempt in 0..=self.retry_count {
            match self.transact_write(&request, slave, address).await {
                Ok(frame) if frame == request => return Ok(()),
                Ok(frame) => {
                    last_error = Some(TransportError::CorruptFrame(format!(
                        "写入回显不匹配 @slave{slave}/0x{address:04X}: {frame:02X?}"
                    )));
                }
                Err(error) => last_error = Some(error),
            }
            let _ = self.clear_input();
            if attempt < self.retry_count {
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        }
        Err(last_error.unwrap_or(TransportError::NoResponse))
    }
}

fn append_crc(frame: &mut Vec<u8>) {
    let crc = crc16(frame);
    frame.push((crc & 0xFF) as u8);
    frame.push((crc >> 8) as u8);
}

fn validate_crc(frame: &[u8]) -> TransportResult<()> {
    if frame.len() < 4 {
        return Err(TransportError::CorruptFrame("RTU 帧长度不足".into()));
    }
    let expected = u16::from_le_bytes([frame[frame.len() - 2], frame[frame.len() - 1]]);
    let actual = crc16(&frame[..frame.len() - 2]);
    if expected != actual {
        return Err(TransportError::CorruptFrame(format!(
            "CRC 校验失败: expected 0x{expected:04X}, actual 0x{actual:04X}"
        )));
    }
    Ok(())
}

fn crc16(bytes: &[u8]) -> u16 {
    let mut crc = 0xFFFF;
    for byte in bytes {
        crc ^= u16::from(*byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}
