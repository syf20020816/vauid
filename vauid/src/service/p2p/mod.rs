//! P2P 服务模块
//! 与传统使用WebSocket方式进行的P2P通信不同，我们使用Quic协议进行P2P通信。
//! Vauid 项目主打小队P2P通信场景，四人小队之间通过P2P通信进行实时互动。
//! 目前选型使用 tquic 库实现 QUIC 协议。
//!
//! 说明：tquic 的事件循环是同步驱动且内部使用 `Rc<RefCell<..>>`（非 `Send`），
//! 因此 `P2PServer` 不做跨线程共享；单连接处理建议在独立任务中独占运行。

use std::net::SocketAddr;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use tquic::{Connection, Endpoint, PacketInfo, TransportHandler};
use vauid_shared::error::{Error, QuicError};

use crate::core::quic::socket::{QuicSocket, RecvFrom};
use crate::core::quic::wrap::conf::{QuicConfig, QUIC_CONF_PATH};
use crate::core::Result;

/// P2P 服务端
/// 负责接收客户端连接，转发消息，维护客户端状态。
/// 只负责消息转发，不负责任何业务逻辑
pub struct P2PServer {
    /// Quic 服务端点
    pub endpoint: Endpoint,
    /// 收发监听的Quic套接字
    pub socket: Rc<QuicSocket>,

    /// 接收数据缓冲区
    /// 用于存储从客户端接收的数据
    pub recv_buf: Vec<u8>,
}

impl P2PServer {
    /// 使用默认配置文件 `conf/quic.conf.toml` 在 `listen` 地址上启动 P2P 服务器。
    ///
    /// 配置文件不存在时会自动创建默认配置；服务器需要 TLS 证书，
    /// 请编辑该文件补全 `tls` 的 `cert_file` / `key_file` 后调用。
    pub async fn bind(listen: SocketAddr) -> Result<Self> {
        Self::bind_with_conf(listen, QUIC_CONF_PATH).await
    }

    /// 使用指定配置文件在 `listen` 地址上启动 P2P 服务器。
    ///
    /// 配置经 [`QuicConfig::server`] 加载并转换为 tquic [`Config`]，
    /// 因此要求配置文件中 `tls` 配置了 `cert_file` / `key_file`。
    pub async fn bind_with_conf(listen: SocketAddr, path: impl AsRef<Path>) -> Result<Self> {
        let config = QuicConfig::server(path)?.into_inner();

        let socket = Rc::new(QuicSocket::new(&listen).await?);
        let endpoint = Endpoint::new(
            Box::new(config),
            true, // is_server
            Box::new(P2PHandler),
            socket.clone(),
        );

        Ok(Self {
            endpoint,
            socket,
            recv_buf: vec![0u8; 64 * 1024],
        })
    }

    /// 事件循环（骨架）：接收 UDP 数据包 → 交给 QUIC 端点解析 → 驱动连接内部事件。
    ///
    /// 流读写等业务回调目前由 [`P2PHandler`] 占位，后续在此接入消息转发逻辑。
    pub async fn run(&mut self) -> Result<()> {
        loop {
            // 1. 从本地 UDP socket 接收一个数据包
            let RecvFrom {
                len, local, remote, ..
            } = self
                .socket
                .recv_from(&mut self.recv_buf, self.socket.local_addr)
                .await?;

            // 2. 喂给 QUIC 协议栈（解析 + 解密 + 分发到对应连接）
            let info = PacketInfo {
                src: remote,
                dst: local,
                time: Instant::now(),
            };
            self.endpoint
                .recv(&mut self.recv_buf[..len], &info)
                .map_err(tquic_err)?;

            // 3. 驱动所有连接的状态推进（握手、确认、流事件回调等）
            self.endpoint.process_connections().map_err(tquic_err)?;
        }
    }
}

/// 默认 QUIC 传输事件处理器（骨架）。
/// 目前仅占位，业务回调（连接生命周期 / 流读写）待后续实现。
#[derive(Default)]
pub struct P2PHandler;

impl TransportHandler for P2PHandler {
    fn on_conn_created(&mut self, _conn: &mut Connection) {}

    fn on_conn_established(&mut self, _conn: &mut Connection) {}

    fn on_conn_closed(&mut self, _conn: &mut Connection) {}

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_readable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

/// tquic 原生错误 → 项目统一错误
fn tquic_err(e: tquic::Error) -> Error {
    Error::Quic(QuicError::Config(e.to_string()))
}
