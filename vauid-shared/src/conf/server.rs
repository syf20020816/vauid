use std::{net::Ipv4Addr, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::conf::{ConfRW, quic::QuicConf};

const DEFAULT_LOG_OUTPUT: &str = "log/vauid.log";

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct ServerConf {
    /// 服务器监听地址
    pub addr: String,
    /// 服务器监听端口
    pub port: u16,
    /// 日志配置
    pub log: LogConf,
    /// Quic 配置文件路径
    pub quic: PathBuf,
}

impl Default for ServerConf {
    fn default() -> Self {
        Self {
            addr: Ipv4Addr::LOCALHOST.to_string(),
            port: 8080,
            log: Default::default(),
            quic: PathBuf::from(QuicConf::DEFAULT_PATH),
        }
    }
}

impl ConfRW for ServerConf {
    const DEFAULT_PATH: &str = "conf/server.conf.toml";
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub struct LogConf {
    /// 日志级别
    pub level: LogLevel,
    /// 日志输出路径
    pub output: PathBuf,
    /// 是否开启日志输出
    pub output_enabled: bool,
}

impl Default for LogConf {
    fn default() -> Self {
        Self {
            level: Default::default(),
            output: PathBuf::from(DEFAULT_LOG_OUTPUT),
            output_enabled: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Copy, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    #[default]
    Info = 2,
    Warn = 3,
    Error = 4,
}
