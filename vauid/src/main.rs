//! vauid 是基于 Rust 的 QUIC P2P 服务器。
//!
//! 当前为 **QUIC P2P MVP 阶段**：vauid 以 tquic 作为 QUIC 协议栈，
//! 负责 QUIC 连接的建立与消息转发（当前为回显），
//! 后续媒体数据走客户端之间的点对点直连。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use tracing::{error, info, level_filters::LevelFilter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use vauid::core::quic::wrap::conf::QUIC_CONF_PATH;
use vauid::service::p2p::P2PServer;

/// 默认监听地址（QUIC 常用端口 4433）
const DEFAULT_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4433);

#[tokio::main]
async fn main() {
    init_tracing();

    match P2PServer::bind_with_conf(DEFAULT_ADDR, QUIC_CONF_PATH).await {
        Ok(mut server) => {
            info!(addr = %DEFAULT_ADDR, conf = QUIC_CONF_PATH, "vauid quic p2p server is starting");
            if let Err(e) = server.run().await {
                error!(error = %e, "vauid server run error");
            }
        }
        Err(e) => {
            error!(
                error = %e,
                "failed to start quic server; please fill tls.cert_file/key_file in {}",
                QUIC_CONF_PATH
            );
        }
    }
}

/// 初始化 tracing 日志：默认 info 级别，支持 `RUST_LOG` 环境变量覆盖
fn init_tracing() {
    let filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .from_env_lossy();

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().compact())
        .init();
}
