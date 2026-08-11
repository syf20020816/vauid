
/// 统一错误类型
pub type Result<T> = vauid_shared::Result<T>;

/// 信令服务：WS 连接、消息分发、房间生命周期
pub mod signal;
/// 核心 rtc 服务：str0m 驱动的媒体面（SFU 阶段启用，P2P MVP 阶段留空）
mod rtc;
/// QUIC 相关
pub mod quic;


