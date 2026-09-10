//! SQLite 存储层。单写连接 + WAL；所有操作短小且走索引，
//! 以 std::sync::Mutex 串行化（嵌入式单进程足够，避免异步运行时侵入）。

pub mod records;
mod store;

pub use records::*;
pub use store::{Store, StoreError, StoreResult};
