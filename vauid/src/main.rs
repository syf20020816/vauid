//! vauid 是基于 Rust 的 QUIC P2P 服务器。
//!
//! 当前为 **QUIC P2P MVP 阶段**：vauid 以 tquic 作为 QUIC 协议栈，
//! 负责 QUIC 连接的建立与消息转发（当前为回显），
//! 后续媒体数据走客户端之间的点对点直连。

use tracing::{error, info};
use vauid::service;
use vauid::service::p2p::P2PServer;
use vauid_shared::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let (listen, conf_path) = service::init::init()?;

    match P2PServer::bind_with_conf(listen, &conf_path).await {
        Ok(mut server) => {
            info!(addr = %listen, conf = %conf_path.display(), "vauid quic p2p server is starting");
            if let Err(e) = server.run().await {
                error!(error = %e, "vauid server run error");
            }
        }
        Err(e) => {
            error!(
                error = %e,
                "failed to start quic server; please fill tls.cert_file/key_file in {}",
                conf_path.display()
            );
        }
    }

    Ok(())
}
