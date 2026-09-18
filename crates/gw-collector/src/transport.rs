//! 采集传输端口：agent 只依赖此 trait，生产由真实驱动实现。

use async_trait::async_trait;

#[derive(thiserror::Error, Debug)]
pub enum TransportError {
    #[error("串口/IO 错误: {0}")]
    Io(String),
    #[error("Modbus 异常响应: {0}")]
    Exception(String),
    #[error("响应长度非法: 需要 {want} 字, 实得 {got} 字")]
    BadLength { want: u16, got: usize },
    #[error("Modbus RTU 帧已拒绝: {0}")]
    CorruptFrame(String),
    #[error("设备无响应（可能掉线）")]
    NoResponse,
}

pub type TransportResult<T> = Result<T, TransportError>;

/// Modbus 传输端口（半双工，调用方负责串行化）。
#[async_trait]
pub trait MeterTransport: Send {
    /// 读取寄存器（功能码 03/04）。返回恰好 `quantity` 个字。
    async fn read_registers(
        &mut self,
        function_code: u8,
        slave: u8,
        address: u16,
        quantity: u16,
    ) -> TransportResult<Vec<u16>>;

    /// Write one holding register (function code 06).
    async fn write_single_register(
        &mut self,
        slave: u8,
        address: u16,
        value: u16,
    ) -> TransportResult<()> {
        let _ = (slave, address, value);
        Err(TransportError::Exception(
            "write single register is not supported by this transport".into(),
        ))
    }
}
