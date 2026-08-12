//! 初始化服务
//! - 加载配置文件
//! - 日志初始化
//! - QUIC 配置初始化 (返回)

use std::{net::SocketAddr, path::PathBuf};

use vauid_shared::{
    Result,
    conf::{ConfRW, ServerConf},
};

/// 服务初始化：加载 `conf/server.conf.toml`（不存在时自动创建默认配置）、
/// 初始化日志，返回 (监听地址, QUIC 配置文件路径)。
pub fn init() -> Result<(SocketAddr, PathBuf)> {
    // 加载配置文件（addr/port/log/quic），未填写的字段使用默认值
    let ServerConf {
        addr,
        port,
        log,
        quic,
    } = ServerConf::new::<&str>(None)?;

    // 日志初始化：控制台 + （output_enabled 时）JSON 文件
    crate::log::init(Some(log))?;

    // 组装监听地址：addr 字符串（如 "127.0.0.1"）+ 端口
    let listen = SocketAddr::new(addr.parse()?, port);
    Ok((listen, quic))
}
