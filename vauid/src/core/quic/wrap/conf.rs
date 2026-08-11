//! 对 tquic `Config` 的常规化配置包装
//!
//! 职责划分：
//! - [`QuicConf`]（来自 `vauid-shared` 抽象层）：配置文件加载（[`QuicConf::new`] 等）；
//! - [`QuicConfig`]：接收配置文件路径，内部经 [`QuicConf::new`] 加载后转换为
//!   tquic 的 [`tquic::Config`]。
//!
//! 单位换算约定：`QuicConf` 的时间字段以**秒**为单位（便于配置），
//! tquic 内部以**毫秒**为单位，此处统一使用 `saturating_mul` 换算防止溢出。

use std::path::Path;

use tquic::{Config, CongestionControlAlgorithm, TlsConfig};
use vauid_shared::error::{Error, QuicError};

/// 应用层常规配置类型与默认配置路径，从共享抽象层重新导出。
///
/// vauid 包内的业务模块统一从本包路径取用配置类型，
/// 不直接依赖 `vauid-shared`；`QuicConfig` 负责将其转换为 tquic [`Config`]。
pub use vauid_shared::conf::{CcAlgorithm, QuicConf, TlsConf, QUIC_CONF_PATH};

/// tquic 配置包装：持有构建完成的 tquic [`Config`]。
///
/// 用法：直接传入配置文件路径，内部经 [`QuicConf::new`] 加载
/// （文件不存在时自动创建默认配置），再构建 tquic 配置：
/// `QuicConfig::server(QUIC_CONF_PATH)` / `QuicConfig::client(path)`。
pub struct QuicConfig(pub Config);

impl QuicConfig {
    /// 从配置文件构建 tquic **服务器端** 配置。
    ///
    /// 经 [`QuicConf::new`] 加载配置（文件不存在时自动创建默认配置）；
    /// 要求 `QuicConf::tls` 存在且配置了 `cert_file` / `key_file`，
    /// 否则返回 [`QuicError::Config`]。
    pub fn server<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let conf = QuicConf::new(path)?;
        let tls_conf = conf
            .tls
            .as_ref()
            .ok_or_else(|| config_err("server 需要 tls 配置（cert_file / key_file）"))?;
        let tls = server_tls(tls_conf)?;
        Ok(Self(build_config(&conf, tls)?))
    }

    /// 从配置文件构建 tquic **客户端** 配置。
    ///
    /// 经 [`QuicConf::new`] 加载配置；`tls` 未配置时使用空 ALPN、
    /// 禁 0-RTT 的默认客户端 TLS。
    pub fn client<P: AsRef<Path>>(path: P) -> Result<Self, Error> {
        let conf = QuicConf::new(path)?;
        let tls = match conf.tls.as_ref() {
            Some(tls_conf) => client_tls(tls_conf)?,
            None => TlsConfig::new_client_config(Vec::new(), false).map_err(tquic_err)?,
        };
        Ok(Self(build_config(&conf, tls)?))
    }

    /// 解包出 tquic 原生 [`Config`]
    pub fn into_inner(self) -> Config {
        self.0
    }
}

/// 组装传输层参数 + TLS
fn build_config(conf: &QuicConf, tls: TlsConfig) -> Result<Config, Error> {
    let mut config = Config::new().map_err(tquic_err)?;

    // 传输层
    config.set_max_idle_timeout(conf.max_idle_timeout.saturating_mul(1000));
    config.set_max_handshake_timeout(conf.max_handshake_timeout.saturating_mul(1000));
    config.set_max_concurrent_conns(conf.max_concurrent_conns);
    config.set_recv_udp_payload_size(conf.recv_udp_payload_size);
    config.set_send_udp_payload_size(conf.send_udp_payload_size);

    // 流控
    config.set_initial_max_data(conf.initial_max_data);
    config.set_initial_max_stream_data_bidi_local(conf.initial_max_stream_data_bidi_local);
    config.set_initial_max_stream_data_bidi_remote(conf.initial_max_stream_data_bidi_remote);
    config.set_initial_max_stream_data_uni(conf.initial_max_stream_data_uni);
    config.set_initial_max_streams_bidi(conf.initial_max_streams_bidi);
    config.set_initial_max_streams_uni(conf.initial_max_streams_uni);

    // 拥塞控制 + TLS
    config.set_congestion_control_algorithm(cc_to_tquic(conf.cc_algorithm));
    config.set_tls_config(tls);

    Ok(config)
}

/// 服务器端 TLS：必填证书/私钥
fn server_tls(conf: &TlsConf) -> Result<TlsConfig, Error> {
    let cert = conf
        .cert_file
        .as_deref()
        .ok_or_else(|| config_err("server tls 缺少 cert_file"))?;
    let key = conf
        .key_file
        .as_deref()
        .ok_or_else(|| config_err("server tls 缺少 key_file"))?;

    let mut tls =
        TlsConfig::new_server_config(cert, key, alpn_vec(&conf.alpn), conf.enable_early_data)
            .map_err(tquic_err)?;
    if let Some(ca) = conf.ca_file.as_deref() {
        tls.set_ca_certs(ca).map_err(tquic_err)?;
    }
    Ok(tls)
}

/// 客户端 TLS：可选校验证书/CA
fn client_tls(conf: &TlsConf) -> Result<TlsConfig, Error> {
    let mut tls =
        TlsConfig::new_client_config(alpn_vec(&conf.alpn), conf.enable_early_data)
            .map_err(tquic_err)?;
    tls.set_verify(conf.verify);
    if let Some(ca) = conf.ca_file.as_deref() {
        tls.set_ca_certs(ca).map_err(tquic_err)?;
    }
    Ok(tls)
}

/// ALPN 字符串列表 → 字节序列列表
fn alpn_vec(alpn: &[String]) -> Vec<Vec<u8>> {
    alpn.iter().map(|s| s.as_bytes().to_vec()).collect()
}

/// 常规化拥塞控制枚举 → tquic 枚举
fn cc_to_tquic(cc: CcAlgorithm) -> CongestionControlAlgorithm {
    match cc {
        CcAlgorithm::Cubic => CongestionControlAlgorithm::Cubic,
        CcAlgorithm::Bbr => CongestionControlAlgorithm::Bbr,
        CcAlgorithm::Bbr3 => CongestionControlAlgorithm::Bbr3,
        CcAlgorithm::Copa => CongestionControlAlgorithm::Copa,
        CcAlgorithm::Dummy => CongestionControlAlgorithm::Dummy,
    }
}

/// 构建配置错误（缺少必填项等）
fn config_err(msg: impl Into<String>) -> Error {
    Error::Quic(QuicError::Config(msg.into()))
}

/// tquic 原生错误 → 统一错误
fn tquic_err(e: tquic::Error) -> Error {
    Error::Quic(QuicError::Config(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// 生成临时配置文件
    fn tmp_conf_path(name: &str, conf: &QuicConf) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vauid-wrap-test-{}", std::process::id()));
        let path = dir.join(name);
        conf.save(&path).expect("write tmp conf");
        path
    }

    /// 服务器端配置缺少 tls 时应报错
    #[test]
    fn server_requires_tls() {
        let path = tmp_conf_path("no_tls.toml", &QuicConf::default());
        assert!(matches!(
            QuicConfig::server(path),
            Err(Error::Quic(QuicError::Config(_)))
        ));
    }

    /// 服务器端配置缺少证书字段时应报错
    #[test]
    fn server_requires_cert() {
        let conf = QuicConf {
            tls: Some(TlsConf {
                alpn: vec!["vauid".into()],
                ..Default::default()
            }),
            ..Default::default()
        };
        let path = tmp_conf_path("no_cert.toml", &conf);
        assert!(QuicConfig::server(path).is_err());
    }

    /// 客户端配置无需 tls 即可构建成功
    #[test]
    fn client_builds_without_tls() {
        let path = tmp_conf_path("client_default.toml", &QuicConf::default());
        let cfg = QuicConfig::client(path).expect("client config builds");
        // 解包出的 tquic Config 应可复用
        let _inner = cfg.into_inner();
    }

    /// 拥塞控制算法映射完整
    #[test]
    fn cc_mapping_covers_all() {
        for cc in [
            CcAlgorithm::Cubic,
            CcAlgorithm::Bbr,
            CcAlgorithm::Bbr3,
            CcAlgorithm::Copa,
            CcAlgorithm::Dummy,
        ] {
            let _ = cc_to_tquic(cc);
        }
    }
}
