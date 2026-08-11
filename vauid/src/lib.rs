//! vauid 库入口：信令/媒体核心逻辑。
//!
//! binary (`main.rs`) 与集成测试 (`tests/`) 共用此库。

pub mod core;
/// 相关服务模块: P2P服务， SFU服务
pub mod service;