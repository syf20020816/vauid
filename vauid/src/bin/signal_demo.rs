//! 信令服务器演示入口。
//!
//! 为 [`web/p2p-test.html`] 提供 WebSocket 信令中继（WebRTC 握手）。
//! 握手完成后，媒体/数据流直接在两个浏览器之间点对点传输，服务器只做"介绍人"。
//!
//! 使用：
//! ```bash
//! # 启动信令服务器（默认监听 127.0.0.1:8080，可传参指定监听地址）
//! cargo run -p vauid --bin signal_demo
//!
//! # 浏览器打开 vauid/web/p2p-test.html，两个标签页/设备加入同一房间即可互通
//! ```

use std::sync::Arc;

use axum::{Router, routing::get};
use tokio::net::TcpListener;
use tracing::{error, info};

use vauid::core::signal::server::ws_handler;
use vauid::core::signal::AppState;

#[tokio::main]
async fn main() {
    vauid::log::init(None).expect("日志初始化失败");

    let listen: std::net::SocketAddr = std::env::args()
        .nth(1)
        .map(|s| s.parse().expect("监听地址格式错误，如 127.0.0.1:8080"))
        .unwrap_or_else(|| "127.0.0.1:8080".parse().unwrap());

    let state = Arc::new(AppState::new());
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state);

    let listener = TcpListener::bind(listen)
        .await
        .expect("bind signal server");
    info!(
        addr = %listen,
        "信令服务器已启动；浏览器打开 vauid/web/p2p-test.html，两个标签页/设备加入同一房间即可测试 P2P"
    );
    if let Err(e) = axum::serve(listener, app).await {
        error!(error = %e, "信令服务器运行出错");
    }
}
