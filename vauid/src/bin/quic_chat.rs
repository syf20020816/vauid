//! # QUIC 交互式测试客户端
//!
//! 连接 [`main.rs`]（`P2PServer`，回显模式）启动的 QUIC 服务器，
//! 终端输入一行消息 → 发送 → 服务器原样回显 → 打印，用于验证 QUIC 收发链路。
//!
//! ## 为什么每条消息新建一条 QUIC 连接
//! tquic 的流操作（`stream_bidi_new` / `stream_write`）只能在 `TransportHandler`
//! 回调内进行：`Endpoint` 不对外暴露 `Connection`，也没有"外部 tick"接口，
//! 无法在事件循环外任意时刻向已建立连接发送数据。因此这里让"发送"始终发生在
//! 回调内（新连接握手完成即发送待发消息），完全由事件驱动，无需任何 hack。
//! 本机握手开销可忽略，且与服务器回显一一对应，适合作为 P2P 链路测试工具。
//!
//! ## 用法
//! ```bash
//! # 1. 先启动 QUIC 服务器（见 main.rs，需 conf/quic.conf.toml 配置好证书）
//! cargo run -p vauid --bin vauid
//!
//! # 2. 连接服务器（默认使用与服务器相同的 conf/quic.conf.toml，ALPN 一致）
//! cargo run -p vauid --bin quic_chat -- 127.0.0.1:4433
//! # 输入消息回车发送，输入 quit 退出
//! ```

use std::cell::RefCell;
use std::io::{self, Write};
use std::net::SocketAddr;
use std::rc::Rc;
use std::time::Instant;

use bytes::Bytes;
use tquic::{Connection, Endpoint, PacketInfo, TransportHandler};
use tokio::io::AsyncBufReadExt;
use vauid::core::quic::socket::{QuicSocket, RecvFrom};
use vauid::core::quic::wrap::conf::{QuicConfig, QUIC_CONF_PATH};

/// 客户端共享状态
#[derive(Default)]
struct ChatState {
    /// 待发送的消息（下一条新连接建立时消费）
    next_msg: Option<Vec<u8>>,
}

/// 客户端传输事件处理器：新连接握手完成后发送待发消息；收到回显时打印
struct ChatHandler {
    state: Rc<RefCell<ChatState>>,
}

impl TransportHandler for ChatHandler {
    fn on_conn_created(&mut self, _conn: &mut Connection) {}

    fn on_conn_established(&mut self, conn: &mut Connection) {
        let msg = self.state.borrow_mut().next_msg.take();
        if let Some(msg) = msg
            && let Ok(stream_id) = conn.stream_bidi_new(0, false)
        {
            let _ = conn.stream_write(stream_id, Bytes::from(msg), true);
        }
    }

    fn on_conn_closed(&mut self, _conn: &mut Connection) {}

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
        loop {
            let mut buf = [0u8; 4096];
            let (n, fin) = match conn.stream_read(stream_id, &mut buf) {
                Ok(v) => v,
                // 暂无更多数据（Done）或流已错误：结束本轮
                Err(_) => break,
            };
            if n > 0 {
                print!("< {}", String::from_utf8_lossy(&buf[..n]));
                let _ = io::stdout().flush();
            }
            if fin {
                break;
            }
        }
    }

    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    vauid::log::init().expect("日志初始化失败");

    let args: Vec<String> = std::env::args().collect();
    let remote: SocketAddr = args
        .get(1)
        .expect("用法: quic_chat <server_addr> [conf_path]")
        .parse()
        .expect("无效的服务器地址，如 127.0.0.1:4433");
    let conf_path = args.get(2).map(String::as_str).unwrap_or(QUIC_CONF_PATH);

    let config = QuicConfig::client(conf_path)?.into_inner();
    let socket = Rc::new(QuicSocket::new_client_socket(true).await?);
    let state = Rc::new(RefCell::new(ChatState::default()));
    let mut endpoint = Endpoint::new(
        Box::new(config),
        false, // is_server
        Box::new(ChatHandler { state: state.clone() }),
        socket.clone(),
    );

    let mut recv_buf = vec![0u8; 64 * 1024];
    println!("已连接 {remote}（回显模式）；输入消息回车发送，输入 quit 退出");

    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    loop {
        tokio::select! {
            // 终端输入：排队一条消息，并新建一条 QUIC 连接来发送
            line = lines.next_line() => {
                let line = match line? {
                    Some(l) => l,
                    None => break, // stdin EOF
                };
                let line = line.trim().to_string();
                if line.is_empty() { continue; }
                if line == "quit" { break; }
                state.borrow_mut().next_msg = Some(line.into_bytes());
                endpoint.connect(socket.local_addr, remote, None, None, None, None)?;
                endpoint.process_connections()?;
            }
            // 网络收包：交给 QUIC 栈解析并驱动连接状态推进
            recv = socket.recv_from(&mut recv_buf, socket.local_addr) => {
                let RecvFrom { len, local, remote: from, .. } = recv?;
                let info = PacketInfo { src: from, dst: local, time: Instant::now() };
                endpoint.recv(&mut recv_buf[..len], &info)?;
                endpoint.process_connections()?;
            }
        }
    }
    Ok(())
}
