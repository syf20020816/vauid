use crate::Result;
use serde::{Deserialize, Serialize};
use std::{fs, fs::read_to_string, path::Path};

/// 默认配置目录（相对应用根目录）
pub const QUIC_CONF_DIR: &str = "conf";
/// 默认 Quic 配置文件路径：应用根目录下 `conf/quic.conf.toml`
pub const QUIC_CONF_PATH: &str = "conf/quic.conf.toml";

/// 拥塞控制算法（常规化枚举，与具体 QUIC 实现解耦）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CcAlgorithm {
    /// CUBIC
    Cubic,
    /// BBR（默认）
    #[default]
    Bbr,
    /// BBRv3（实验性）
    Bbr3,
    /// COPA（实验性）
    Copa,
    /// Dummy（测试用，静态拥塞窗口）
    Dummy,
}

impl CcAlgorithm {
    /// 算法名称字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cubic => "cubic",
            Self::Bbr => "bbr",
            Self::Bbr3 => "bbr3",
            Self::Copa => "copa",
            Self::Dummy => "dummy",
        }
    }
}

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

/// Quic 常规配置结构体
/// 在应用层会为 Quic 服务器配置 Quic 相关参数，具体转换看应用层处理
/// 这里只定义常规的 Quic 配置参数，方便转为任何 Quic 服务器的配置结构体（tquic, ant-quic, s2n-quic等）
///
/// 所有字段均有默认值（对齐 tquic 1.6 默认），配置文件仅需填写需要覆盖的项。
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct QuicConf {
    /// 最大空闲超时时间，单位秒；0 表示禁用（默认）
    pub max_idle_timeout: u64,
    /// 握手超时时间，单位秒；0 表示关闭超时（默认 30）
    pub max_handshake_timeout: u64,
    /// 最大并发连接数（默认 1_000_000）
    pub max_concurrent_conns: u32,
    /// 接收 UDP 载荷大小上限，单位字节（默认 65527）
    pub recv_udp_payload_size: u16,
    /// 最大发送 UDP 载荷大小，单位字节（默认 1200，实际由 DPLPMTUD 探测）
    pub send_udp_payload_size: usize,
    /// 连接级初始流控窗口，单位字节（默认 10_485_760）
    pub initial_max_data: u64,
    /// 双向流本地数据流控窗口，单位字节（默认 5_242_880）
    pub initial_max_stream_data_bidi_local: u64,
    /// 双向流对端数据流控窗口，单位字节（默认 2_097_152）
    pub initial_max_stream_data_bidi_remote: u64,
    /// 单向流数据流控窗口，单位字节（默认 1_048_576）
    pub initial_max_stream_data_uni: u64,
    /// 最大双向流数量（默认 200）
    pub initial_max_streams_bidi: u64,
    /// 最大单向流数量（默认 100）
    pub initial_max_streams_uni: u64,
    /// 拥塞控制算法（默认 bbr）
    pub cc_algorithm: CcAlgorithm,
    /// TLS 配置；`None` 表示不配置 TLS（由应用层决定是否必需）
    pub tls: Option<TlsConf>,
}

impl Default for QuicConf {
    fn default() -> Self {
        Self {
            max_idle_timeout: 0,
            max_handshake_timeout: 30,
            max_concurrent_conns: 1_000_000,
            recv_udp_payload_size: 65_527,
            send_udp_payload_size: 1_200,
            initial_max_data: 10_485_760,
            initial_max_stream_data_bidi_local: 5_242_880,
            initial_max_stream_data_bidi_remote: 2_097_152,
            initial_max_stream_data_uni: 1_048_576,
            initial_max_streams_bidi: 200,
            initial_max_streams_uni: 100,
            cc_algorithm: CcAlgorithm::default(),
            tls: None,
        }
    }
}

impl QuicConf {
    /// 加载默认配置文件 `conf/quic.conf.toml`（相对应用根目录）。
    ///
    /// 文件不存在时使用默认值创建并写入，后续可直接读取。
    pub fn load() -> Result<Self> {
        Self::new(QUIC_CONF_PATH)
    }

    /// 加载 TOML 配置文件。
    ///
    /// - 文件已存在：读取并解析，未填写的字段使用常规默认值；
    /// - 文件不存在：使用默认值创建该配置文件并写入，再返回默认配置。
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if path.is_file() {
            let content = read_to_string(path)?;
            return Ok(toml::from_str(&content)?);
        }

        let conf = Self::default();
        conf.save(path)?;
        Ok(conf)
    }

    /// 将当前配置序列化为 TOML 并写入指定路径（自动创建父目录）。
    pub fn save<P>(&self, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 配置文件不存在时自动创建默认 TOML，之后可直接读取
    #[test]
    fn new_creates_default_toml_when_missing() {
        let dir = std::env::temp_dir().join(format!("vauid-conf-test-{}", std::process::id()));
        let path = dir.join("quic.conf.toml");
        let _ = fs::remove_dir_all(&dir);

        // 不存在：创建默认配置文件
        let conf = QuicConf::new(&path).expect("create default");
        assert_eq!(conf.max_handshake_timeout, 30);
        assert!(path.is_file());

        // 已存在：直接读取，字段与默认值一致
        let conf2 = QuicConf::new(&path).expect("reload");
        assert_eq!(conf2.max_handshake_timeout, 30);
        assert_eq!(conf2.cc_algorithm, CcAlgorithm::Bbr);
        assert_eq!(conf2.tls, None);

        let _ = fs::remove_dir_all(&dir);
    }

    /// 默认配置文件路径约定
    #[test]
    fn default_path_convention() {
        assert_eq!(QUIC_CONF_PATH, "conf/quic.conf.toml");
        assert_eq!(QUIC_CONF_DIR, "conf");
    }
}
