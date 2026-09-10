//! 电表采集端口与实现。

pub mod profile;
pub mod transport;

#[cfg(feature = "modbus")]
pub mod modbus_rtus;

pub use profile::{DataType, MeterProfile, RegisterSpec};
pub use transport::{MeterTransport, TransportError};

#[cfg(feature = "modbus")]
pub use modbus_rtus::{ModbusRtuTransport, SerialConfig};
