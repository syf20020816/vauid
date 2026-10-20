//! P2P 服务模块
//! 与传统使用WebSocket方式进行的P2P通信不同，我们使用Quic协议进行P2P通信。
//! Vauid 项目主打小队P2P通信场景，四人小队之间通过P2P通信进行实时互动。
//! 目前选型使用 tquic 库实现 QUIC 协议。
//!
//! 转发语义：服务端维护「连接索引 -> 会话」注册表；某会话发来消息时，
//! 转发给注册表中所有其他会话（多会话转发），
//! 无其他会话时退化为原样回显（单会话回显，兼容既有行为）。
//!
//! 说明：tquic 的事件循环是同步驱动且内部使用 `Rc<RefCell<..>>`（非 `Send`），
//! 因此 `P2PServer` 不做跨线程共享；单连接处理建议在独立任务中独占运行。

pub mod client;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tquic::{Connection, Endpoint, PacketInfo, TransportHandler};
use vauid_shared::error::{Error, QuicError, SignalError};
use vauid_shared::proto::{Message, MsgType};

use crate::core::quic::socket::{QuicSocket, RecvFrom};
use crate::core::quic::wrap::conf::{QuicConfig, QUIC_CONF_PATH};
use crate::core::Result;

/// 单个已连接会话的状态
struct SessionState {
    /// 服务端发起的下行流 id（welcome 与转发消息共用）
    downstream: u64,
    /// 待转发到该会话的出站消息队列
    outbox: VecDeque<Bytes>,
    /// 客户端上行流的分帧缓冲：流 id -> 已累计字节。
    /// 帧可能跨多个 QUIC 包分片到达（一次 `on_stream_readable` 不一定收全），
    /// fin 到达前缓冲，fin 后整帧解析。
    recv_bufs: HashMap<u64, Vec<u8>>,
}

/// 会话注册表：endpoint 连接索引 -> 会话状态
type ConnRegistry = HashMap<u64, SessionState>;

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
    /// 会话注册表（与 handler 共享，run 循环中冲洗各会话的出站队列）
    registry: Rc<RefCell<ConnRegistry>>,
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
        // 会话注册表：handler 与服务端共享（handler 投递出站消息，run 循环冲洗）
        let registry = Rc::new(RefCell::new(ConnRegistry::new()));
        let endpoint = Endpoint::new(
            Box::new(config),
            true, // is_server
            Box::new(P2PHandler::new(registry.clone())),
            socket.clone(),
        );

        Ok(Self {
            endpoint,
            socket,
            recv_buf: vec![0u8; 64 * 1024],
            registry,
        })
    }

    /// 事件循环：接收 UDP 数据包 → 交给 QUIC 端点解析 → 驱动连接内部事件 →
    /// 冲洗各会话的出站队列（转发消息经下行流下发）。
    pub async fn run(&mut self) -> Result<()> {
        loop {
            // 1. 竞速：下一个包到达 vs QUIC 定时器到期。
            //    只等包会造成定时器饥饿：丢包后对端等重传，而重传依赖
            //    PTO 定时器；空闲期 keep-alive 同理永远发不出去。
            let next_timer = self.endpoint.timeout();
            let timer_fired = timer_wait(next_timer);
            tokio::select! {
                res = self
                    .socket
                    .recv_from(&mut self.recv_buf, self.socket.local_addr) =>
                {
                    let RecvFrom { len, local, remote, .. } = res?;
                    // 2. 喂给 QUIC 协议栈（解析 + 解密 + 分发到对应连接）
                    let info = PacketInfo {
                        src: remote,
                        dst: local,
                        time: Instant::now(),
                    };
                    self.endpoint
                        .recv(&mut self.recv_buf[..len], &info)
                        .map_err(tquic_err)?;
                }
                _ = timer_fired => {}
            }

            // 3. 驱动所有连接的状态推进（超时检测、握手、确认、流事件回调等；
            //    回调可能向其他会话的出站队列投递转发消息）
            self.endpoint.on_timeout(Instant::now());
            self.endpoint.process_connections().map_err(tquic_err)?;

            // 4. 冲洗出站队列；有写入则再驱动一次连接发包
            if self.flush_outboxes() {
                self.endpoint.process_connections().map_err(tquic_err)?;
            }
        }
    }

    /// 冲洗所有会话的出站队列：把 outbox 中的消息经各自的下行流写出。
    ///
    /// 返回是否发生过写入（调用方据此再驱动一次连接发包）。
    /// 连接不存在或写入失败仅告警：连接可能已断开，
    /// `on_conn_closed` 回调会从注册表清理对应会话。
    /// 连接不存在时按信令面 [`SignalError::TargetNotFound`] 归因
    /// （错误树 Slice 4 接入），写入失败保留结构化 error 字段。
    fn flush_outboxes(&mut self) -> bool {
        // 先 clone 出 Rc 再借用，避免与 self.endpoint 的使用产生借用冲突
        let registry = self.registry.clone();
        // 收集 outbox 非空的会话（take 取走消息，尽快结束 RefCell 借用）
        let mut pending: Vec<(u64, u64, VecDeque<Bytes>)> = Vec::new();
        for (idx, session) in registry.borrow_mut().iter_mut() {
            if session.outbox.is_empty() {
                continue;
            }
            pending.push((*idx, session.downstream, std::mem::take(&mut session.outbox)));
        }

        let mut written = false;
        for (idx, downstream, msgs) in pending {
            let Some(conn) = self.endpoint.conn_get_mut(idx) else {
                // 转发目标连接已断开/被移除：构造信令面错误归因记录
                let e = Error::Signal(SignalError::TargetNotFound { target: format!("conn-{idx}") });
                tracing::warn!(index = idx, error = %e, "冲洗出站队列时连接不存在");
                continue;
            };
            for msg in msgs {
                match conn.stream_write(downstream, msg, false) {
                    Ok(_) => written = true,
                    Err(e) => {
                        tracing::warn!(index = idx, stream_id = downstream, error = %e,
                            "转发消息写入下行流失败");
                    }
                }
            }
        }
        written
    }
}

/// 服务端连接建立后主动下发的欢迎文本（Chat 类型 Message 帧的载荷）
const WELCOME_PAYLOAD: &[u8] = b"welcome to vauid";

/// 默认 QUIC 传输事件处理器。
///
/// 服务器只负责消息转发：维护「连接索引 -> 会话」注册表，
/// 客户端发来的消息投递到其他会话的出站队列（由 [`P2PServer::run`]
/// 统一冲洗下发），无其他会话时退化为回显；连接建立后主动下发
/// welcome。不负责任何业务逻辑。
///
/// 错误归因（错误树 Slice 4 起）：消息帧解析失败经
/// `From<serde_json::Error>` 归入 `Error::Serde`；转发目标连接缺失
/// 由 [`P2PServer::flush_outboxes`] 归入 `Error::Signal(SignalError::TargetNotFound)`。
pub struct P2PHandler {
    /// 会话注册表（与 [`P2PServer`] 共享）
    registry: Rc<RefCell<ConnRegistry>>,
}

impl P2PHandler {
    /// 创建 handler，绑定共享会话注册表
    fn new(registry: Rc<RefCell<ConnRegistry>>) -> Self {
        Self { registry }
    }
}

impl TransportHandler for P2PHandler {
    fn on_conn_created(&mut self, _conn: &mut Connection) {}

    /// 连接建立：服务端主动打开双向流（流 id 从 1 开始，低 2 位为 1），
    /// 以统一 `Message` 帧（Chat 类型）下发欢迎消息，
    /// 建立服务端 → 客户端方向的通路，并把会话登记到注册表。
    fn on_conn_established(&mut self, conn: &mut Connection) {
        // endpoint 内部 slab 索引（on_conn_created 前已设置，用于注册表键）
        let idx = match conn.index() {
            Some(v) => v,
            None => {
                tracing::warn!("连接建立但缺少 endpoint 索引，放弃注册会话");
                return;
            }
        };
        let downstream = match conn.stream_bidi_new(0, false) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!(error = %e, "打开服务端下发流失败");
                return;
            }
        };
        let welcome = Message {
            ty: MsgType::Chat,
            seq: 0,
            flags: 0,
            payload: WELCOME_PAYLOAD.to_vec(),
        };
        let Ok(bytes) = serde_json::to_vec(&welcome) else {
            tracing::warn!("序列化 welcome Message 帧失败");
            return;
        };
        // fin=false 保持流打开，welcome 与后续转发消息共用该下行流
        if let Err(e) = conn.stream_write(downstream, Bytes::from(bytes), false) {
            tracing::warn!(stream_id = downstream, error = %e, "下发 welcome 消息失败");
        }
        self.registry.borrow_mut().insert(
            idx,
            SessionState {
                downstream,
                outbox: VecDeque::new(),
                recv_bufs: HashMap::new(),
            },
        );
    }

    /// 连接关闭：从注册表移除会话，停止向其转发
    /// （此时连接索引仍有效，process_connections 随后才移除连接）
    fn on_conn_closed(&mut self, conn: &mut Connection) {
        if let Some(idx) = conn.index() {
            self.registry.borrow_mut().remove(&idx);
        }
    }

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    /// 流可读：读取客户端消息（统一 `Message` 帧）。
    ///
    /// 帧可能跨多个 QUIC 包分片到达（一次回调不保证收全），因此按流缓冲，
    /// fin 到达后整帧解析。客户端上行流以 fin 结尾（每流一帧）。
    ///
    /// - 多会话：把完整帧投递到除本连接外所有会话的出站队列，
    ///   由 [`P2PServer::run`] 冲洗后经各自下行流转发；
    /// - 无其他会话：原样写回流（回显，兼容既有行为）。
    fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
        if stream_id & 3 == 1 {
            // 服务端发起的下行流不应有客户端数据
            tracing::warn!(stream_id, "服务端下行流出现可读数据，忽略");
            return;
        }
        let idx = match conn.index() {
            Some(v) => v,
            None => {
                tracing::warn!(stream_id, "读取客户端流时缺少连接索引，忽略");
                return;
            }
        };

        let mut data = Vec::new();
        let mut fin = false;
        loop {
            let mut buf = [0u8; 4096];
            let (n, f) = match conn.stream_read(stream_id, &mut buf) {
                Ok(v) => v,
                // 暂无更多数据（Done）或流已错误：结束本轮
                Err(_) => break,
            };
            if n > 0 {
                data.extend_from_slice(&buf[..n]);
            }
            fin = f;
            if f {
                break;
            }
        }
        if data.is_empty() {
            return;
        }

        // 分帧缓冲：fin 到达前累计，fin 后取走整帧并清理该流缓冲
        let frame = {
            let mut registry = self.registry.borrow_mut();
            let Some(session) = registry.get_mut(&idx) else {
                // 连接建立时已注册，正常不应发生
                tracing::warn!(stream_id, index = idx, "读取客户端流时会话未注册，忽略");
                return;
            };
            let buf = session.recv_bufs.entry(stream_id).or_default();
            buf.extend_from_slice(&data);
            if !fin {
                tracing::debug!(stream_id, buffered = buf.len(), "上行帧未收全，继续缓冲");
                return;
            }
            let frame = std::mem::take(buf);
            session.recv_bufs.remove(&stream_id);
            frame
        };

        // 校验为合法 Message 帧：解析失败经 From<serde_json::Error> 归入
        // Error::Serde 类别（错误树 Slice 4 接入），按类别归因后丢弃
        if let Err(err) = serde_json::from_slice::<Message>(&frame) {
            let e: Error = err.into();
            tracing::warn!(stream_id, len = frame.len(), error = %e, "非法 Message 帧，丢弃");
            return;
        }

        // 转发语义：投递到除本连接外的所有会话；无其他会话时退化为回显
        let mut has_other = false;
        {
            let mut registry = self.registry.borrow_mut();
            for (other, session) in registry.iter_mut() {
                if *other == idx {
                    continue;
                }
                has_other = true;
                session.outbox.push_back(Bytes::copy_from_slice(&frame));
            }
        }
        if !has_other {
            // 回显：原样写回（每流一帧，fin 结尾）
            let _ = conn.stream_write(stream_id, Bytes::from(frame), true);
        }
    }

    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

/// QUIC 定时器等待 future：`Some(d)` 睡到到期；`None` 永不完成
/// （仅等新包；select 分支表达式无条件求值，不能在此 unwrap）
pub(super) async fn timer_wait(next: Option<Duration>) {
    match next {
        Some(d) => tokio::time::sleep(d).await,
        None => std::future::pending().await,
    }
}

/// tquic 原生错误 → 项目统一错误
fn tquic_err(e: tquic::Error) -> Error {
    Error::Quic(QuicError::Config(e.to_string()))
}
