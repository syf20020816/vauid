//! vauid 是基于 Rust 的 WebRTC 信令与媒体服务器。
//!
//! 当前为 **P2P MVP 阶段**：服务器仅承担信令介绍人（SDP/ICE 中继），
//! 媒体流完全走客户端之间的点对点连接，服务器带宽成本为零。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tracing::{info, level_filters::LevelFilter};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};
use vauid::core::signal::{server::ws_handler, AppState};

/// 默认监听地址
const DEFAULT_ADDR: SocketAddr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);

#[tokio::main]
async fn main() {
    init_tracing();

    let state = Arc::new(AppState::new());

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(state);

    info!(addr = %DEFAULT_ADDR, "vauid is starting");

    let listener = TcpListener::bind(DEFAULT_ADDR)
        .await
        .expect("failed to bind address");
    let _ = axum::serve(listener, app).await;
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
