use crate::conf::ConfRW;
use serde::{Deserialize, Serialize};

use super::CcAlgorithm;
use super::TlsConf;

/// 默认 Quic 配置文件路径（相对应用根目录）：`conf/quic.conf.toml`
pub const QUIC_CONF_PATH: &str = "conf/quic.conf.toml";

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

impl ConfRW for QuicConf {
    /// 默认 Quic 配置文件路径：应用根目录下 `conf/quic.conf.toml`
    const DEFAULT_PATH: &str = QUIC_CONF_PATH;
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// 配置文件不存在时自动创建默认 TOML，之后可直接读取
    #[test]
    fn new_creates_default_toml_when_missing() {
        let dir = std::env::temp_dir().join(format!("vauid-conf-test-{}", std::process::id()));
        let path = dir.join("quic.conf.toml");
        let _ = fs::remove_dir_all(&dir);

        // 不存在：创建默认配置文件
        let conf = QuicConf::new(Some(&path)).expect("create default");
        assert_eq!(conf.max_handshake_timeout, 30);
        assert!(path.is_file());

        // 已存在：直接读取，字段与默认值一致
        let conf2 = QuicConf::new(Some(&path)).expect("reload");
        assert_eq!(conf2.max_handshake_timeout, 30);
        assert_eq!(conf2.cc_algorithm, CcAlgorithm::Bbr);
        assert_eq!(conf2.tls, None);

        let _ = fs::remove_dir_all(&dir);
    }

    /// 文件已存在但为空（0 字节，历史遗留）时，应重新生成默认配置而非静默读为空
    #[test]
    fn new_regenerates_when_file_empty() {
        let dir = std::env::temp_dir().join(format!("vauid-conf-empty-{}", std::process::id()));
        let path = dir.join("quic.conf.toml");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "").unwrap();

        let conf = QuicConf::new(Some(&path)).expect("regenerate defaults");
        assert_eq!(conf.max_handshake_timeout, 30);
        // 文件应被重写为默认内容，而非继续保持空
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("max_handshake_timeout"));

        let _ = fs::remove_dir_all(&dir);
    }
}
