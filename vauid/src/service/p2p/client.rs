//! QUIC 客户端（P2P 客户端测试与互通使用）。
//!
//! 与 [`super::P2PServer`] 对称：同一套 `QuicSocket` + tquic `Endpoint` 封装，
//! 以客户端角色（`is_server = false`）发起连接，握手完成后自动打开双向流发送消息，
//! 并在收到回显后结束事件循环。

use std::cell::RefCell;
use std::net::SocketAddr;
use std::path::Path;
use std::rc::Rc;
use std::time::Instant;

use bytes::Bytes;
use tquic::{Connection, Endpoint, PacketInfo, TransportHandler};
use vauid_shared::error::{Error, QuicError};

use crate::core::quic::socket::{QuicSocket, RecvFrom};
use crate::core::quic::wrap::conf::QuicConfig;
use crate::core::Result;

/// 客户端会话状态：由 handler 写入，事件循环据此判断完成条件
#[derive(Debug, Default)]
pub struct ClientState {
    /// 连接是否已建立（握手完成）
    pub established: bool,
    /// 已打开的双向流 id
    pub stream_id: Option<u64>,
    /// 是否已发送消息
    pub sent: bool,
    /// 收到的回显消息
    pub received: Option<Vec<u8>>,
}

/// P2P 客户端
pub struct QuicClient {
    /// Quic 端点（客户端角色）
    pub endpoint: Endpoint,
    /// 底层 UDP 套接字
    pub socket: Rc<QuicSocket>,
    /// 接收数据缓冲区
    pub recv_buf: Vec<u8>,
    /// handler 会话状态（事件循环读取完成条件）
    pub state: Rc<RefCell<ClientState>>,
}

impl QuicClient {
    /// 连接远程 QUIC 服务器。
    ///
    /// - `remote`：服务器地址；
    /// - `path`：客户端配置文件路径（经 [`QuicConfig::client`] 加载）；
    /// - `msg`：握手完成后通过双向流发送的消息。
    ///
    /// 返回后握手已发起（Initial 已发出），需调用 [`Self::run`] 驱动事件循环。
    pub async fn connect(
        remote: SocketAddr,
        path: impl AsRef<Path>,
        msg: Vec<u8>,
    ) -> Result<Self> {
        let config = QuicConfig::client(path)?.into_inner();
        let socket = Rc::new(QuicSocket::new_client_socket(true).await?);

        let state = Rc::new(RefCell::new(ClientState::default()));
        let handler = Box::new(P2PClientHandler {
            state: state.clone(),
            send_msg: msg,
        });

        let mut endpoint = Endpoint::new(
            Box::new(config),
            false, // is_server
            handler,
            socket.clone(),
        );
        // 发起握手：connect 排队发送 Initial，process_connections 真正发出
        let _conn_idx = endpoint
            .connect(socket.local_addr, remote, None, None, None, None)
            .map_err(tquic_err)?;
        endpoint.process_connections().map_err(tquic_err)?;

        Ok(Self {
            endpoint,
            socket,
            recv_buf: vec![0u8; 64 * 1024],
            state,
        })
    }

    /// 事件循环：收包 → 喂给 QUIC 栈 → 驱动连接事件；
    /// 收到回显消息后返回。
    pub async fn run(&mut self) -> Result<()> {
        loop {
            let RecvFrom {
                len, local, remote, ..
            } = self
                .socket
                .recv_from(&mut self.recv_buf, self.socket.local_addr)
                .await?;

            let info = PacketInfo {
                src: remote,
                dst: local,
                time: Instant::now(),
            };
            self.endpoint
                .recv(&mut self.recv_buf[..len], &info)
                .map_err(tquic_err)?;
            self.endpoint.process_connections().map_err(tquic_err)?;

            if self.state.borrow().received.is_some() {
                return Ok(());
            }
        }
    }
}

/// 客户端传输事件处理器：握手完成后开流发消息，收到数据时记录回显
struct P2PClientHandler {
    state: Rc<RefCell<ClientState>>,
    send_msg: Vec<u8>,
}

impl TransportHandler for P2PClientHandler {
    fn on_conn_created(&mut self, _conn: &mut Connection) {}

    fn on_conn_established(&mut self, conn: &mut Connection) {
        let mut state = self.state.borrow_mut();
        state.established = true;
        // 打开双向流并发送消息（fin 结束）
        if let Ok(stream_id) = conn.stream_bidi_new(0, false) {
            state.stream_id = Some(stream_id);
            let msg = self.send_msg.clone();
            if conn.stream_write(stream_id, Bytes::from(msg), true).is_ok() {
                state.sent = true;
            }
        }
    }

    fn on_conn_closed(&mut self, _conn: &mut Connection) {}

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    /// 流可读：读出全部数据记录到 [`ClientState::received`]
    fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
        let mut state = self.state.borrow_mut();
        let mut data = Vec::new();
        loop {
            let mut buf = [0u8; 4096];
            let (n, fin) = match conn.stream_read(stream_id, &mut buf) {
                Ok(v) => v,
                // 暂无更多数据（Done）或流已错误：结束本轮
                Err(_) => break,
            };
            if n > 0 {
                data.extend_from_slice(&buf[..n]);
            }
            if fin {
                break;
            }
        }
        if !data.is_empty() {
            state.received = Some(data);
        }
    }

    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

/// tquic 原生错误 → 项目统一错误
fn tquic_err(e: tquic::Error) -> Error {
    Error::Quic(QuicError::Config(e.to_string()))
}
