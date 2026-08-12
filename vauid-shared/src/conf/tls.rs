use serde::{Deserialize, Serialize};

/// TLS 常规配置结构体
/// 与具体 QUIC 实现的 TLS 配置（tquic 的 TlsConfig 等）解耦，由应用层负责转换。
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct TlsConf {
    /// PEM 证书文件路径（服务器端必填）
    pub cert_file: Option<String>,
    /// PEM 私钥文件路径（服务器端必填）
    pub key_file: Option<String>,
    /// CA 证书路径（文件或目录），用于校验对端证书
    pub ca_file: Option<String>,
    /// ALPN 协议列表，如 `["vauid", "h3"]`
    pub alpn: Vec<String>,
    /// 是否启用 0-RTT 早数据
    pub enable_early_data: bool,
    /// 客户端是否校验对端证书
    pub verify: bool,
}