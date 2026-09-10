//! Modbus RTU 真驱动（RS485 串口）。

use async_trait::async_trait;
use serde::Deserialize;
use tokio_modbus::client::{Context, Reader, rtu};
use tokio_modbus::prelude::Slave;
use tokio_modbus::slave::SlaveContext;
use tokio_serial::{DataBits, Parity, SerialPortBuilderExt, StopBits};

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

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: "/dev/ttyUSB0".to_string(),
            baud: default_baud(),
            data_bits: default_data_bits(),
            stop_bits: default_stop_bits(),
            parity: "NONE".to_string(),
        }
    }
}

/// 持有一条串口连接的 RTU 主站。
pub struct ModbusRtuTransport {
    ctx: Context,
}

impl ModbusRtuTransport {
    /// 打开串口并附加默认从站（每次读取前按表地址 set_slave）。
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
        let ctx = rtu::attach_slave(stream, Slave(0));
        Ok(Self { ctx })
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
        self.ctx.set_slave(Slave(slave));
        // tokio-modbus 0.16 返回双层 Result：外层=传输错误，内层=Modbus 异常码
        let response = match function_code {
            3 => self.ctx.read_holding_registers(address, quantity).await,
            4 => self.ctx.read_input_registers(address, quantity).await,
            other => {
                return Err(TransportError::Exception(format!(
                    "unsupported function code {other} @slave{slave}/0x{address:04X}"
                )));
            }
        };
        let words: Vec<u16> = match response {
            Ok(Ok(words)) => words,
            Ok(Err(exception)) => {
                return Err(TransportError::Exception(format!(
                    "fc{function_code}/slave{slave}/0x{address:04X}: {exception:?}"
                )));
            }
            Err(e) => {
                return Err(TransportError::Io(format!(
                    "modbus 传输错误 @fc{function_code}/slave{slave}/0x{address:04X}: {e}"
                )));
            }
        };
        if words.len() != quantity as usize {
            return Err(TransportError::BadLength {
                want: quantity,
                got: words.len(),
            });
        }
        Ok(words)
    }
}
