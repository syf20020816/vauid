//! # 信令服务
//!
//! 基于 axum WebSocket 的信令服务：连接管理、消息分发、房间生命周期驱动。
//!
//! ## 连接生命周期
//! 1. 客户端连接 `/ws`，尚未加入任何房间。
//! 2. 收到 `join` 后注册进房间，服务器回 `joined`（含房间内已有客户端），并向房间内其他客户端广播 `peer_joined`。
//! 3. `offer` / `answer` / `ice` 由服务器定向中继给 `to` 指定的客户端。
//! 4. 客户端显式 `leave` 或连接断开时，从房间移除并广播 `peer_left`；房间空则触发清理。

use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use vauid_shared::proto::{ClientId, RoomId, ServerEvent, SignalMessage};

use super::room::RoomRegistry;

/// axum 应用共享状态
#[derive(Clone)]
pub struct AppState {
    /// 房间注册表
    pub rooms: RoomRegistry,
}

impl AppState {
    /// 创建应用状态
    pub fn new() -> Self {
        Self {
            rooms: RoomRegistry::new(),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// `/ws` 升级处理器
pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<Arc<AppState>>) -> Response {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// 单条 WS 连接的事件循环
async fn handle_socket(socket: WebSocket, state: Arc<AppState>) {
    let (mut ws_tx, mut ws_rx) = socket.split();

    // 服务器 -> 客户端 的信令事件通道；由房间持有，连接结束时一并关闭
    let (evt_tx, mut evt_rx) = mpsc::unbounded_channel::<ServerEvent>();

    // 当前连接所处的房间与身份（join 后才有值）
    let mut room: Option<Arc<super::room::Room>> = None;
    let mut me: Option<ClientId> = None;
    let mut room_id: Option<RoomId> = None;

    loop {
        tokio::select! {
            // ---------- 客户端 -> 服务器 ----------
            msg = ws_rx.next() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        match serde_json::from_str::<SignalMessage>(text.as_str()) {
                            Ok(msg) => {
                                let close = handle_message(
                                    msg,
                                    state.clone(),
                                    &mut room,
                                    &mut me,
                                    &mut room_id,
                                    &evt_tx,
                                );
                                if close {
                                    break;
                                }
                            }
                            Err(e) => warn!(error = %e, "invalid signal message"),
                        }
                    }
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(_)) => { /* Ping/Pong/Binary 由 axum 与对端处理，忽略 */ }
                    Some(Err(e)) => {
                        warn!(error = %e, "ws receive error");
                        break;
                    }
                    None => break,
                }
            }

            // ---------- 服务器 -> 客户端（房间内事件） ----------
            evt = evt_rx.recv() => {
                match evt {
                    Some(evt) => {
                        let text = serde_json::to_string(&evt).unwrap_or_default();
                        if ws_tx.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    // ---------- 连接关闭清理（显式 leave 后 room 已置 None，此处自动跳过） ----------
    cleanup(&state, room.as_ref(), me.as_ref(), room_id.as_ref());
}

/// 处理单条客户端信令消息。
/// 返回 `true` 表示需要关闭当前连接（如显式 `leave`）。
#[allow(clippy::too_many_arguments)]
fn handle_message(
    msg: SignalMessage,
    state: Arc<AppState>,
    room: &mut Option<Arc<super::room::Room>>,
    me: &mut Option<ClientId>,
    room_id: &mut Option<RoomId>,
    evt_tx: &mpsc::UnboundedSender<ServerEvent>,
) -> bool {
    match msg {
        SignalMessage::Join { room: rid, client } => {
            // 已加入过房间则忽略重复 join
            if room.is_some() {
                warn!(room = %rid, client = %client, "duplicate join ignored");
                return false;
            }

            let r = state.rooms.get_or_create(&rid);
            match r.join(&client, evt_tx.clone()) {
                Ok(existing) => {
                    info!(room = %rid, client = %client, peers = existing.len(), "client joined");

                    *room = Some(r.clone());
                    *me = Some(client.clone());
                    *room_id = Some(rid.clone());

                    // 通知新客户端：join 成功 + 房间内已有客户端
                    let _ = evt_tx.send(ServerEvent::Joined {
                        you: client.clone(),
                        clients: existing,
                    });

                    // 通知房间内其他人：有新客户端加入
                    r.broadcast_except(
                        &client,
                        ServerEvent::PeerJoined {
                            client: client.clone(),
                        },
                    );
                }
                Err(e) => {
                    warn!(room = %rid, client = %client, error = ?e, "join rejected");
                }
            }
            false
        }

        SignalMessage::Leave => {
            if let (Some(r), Some(c)) = (room.as_ref(), me.as_ref()) {
                info!(room = %room_id.as_deref().unwrap_or("?"), client = %c, "client left");
                leave_room(&state, r, c.clone(), room_id.as_ref());
            }
            // 置空身份并请求关闭连接
            *room = None;
            *me = None;
            *room_id = None;
            true
        }

        // ---------- P2P 中继 ----------
        SignalMessage::Offer { to, sdp } => {
            if let (Some(r), Some(c)) = (room.as_ref(), me.as_ref()) {
                r.send_to(
                    &to,
                    ServerEvent::Offer {
                        from: c.clone(),
                        sdp,
                    },
                );
            }
            false
        }
        SignalMessage::Answer { to, sdp } => {
            if let (Some(r), Some(c)) = (room.as_ref(), me.as_ref()) {
                r.send_to(
                    &to,
                    ServerEvent::Answer {
                        from: c.clone(),
                        sdp,
                    },
                );
            }
            false
        }
        SignalMessage::Ice { to, candidate } => {
            if let (Some(r), Some(c)) = (room.as_ref(), me.as_ref()) {
                r.send_to(
                    &to,
                    ServerEvent::Ice {
                        from: c.clone(),
                        candidate,
                    },
                );
            }
            false
        }
    }
}

/// 从房间移除客户端并广播 `peer_left`；房间空则清理
fn leave_room(
    state: &Arc<AppState>,
    room: &Arc<super::room::Room>,
    client: ClientId,
    rid: Option<&RoomId>,
) {
    let empty = room.leave(&client);
    room.broadcast_except(&client.clone(), ServerEvent::PeerLeft { client });
    if empty && let Some(rid) = rid {
        state.rooms.cleanup(rid);
    }
}

/// 连接结束时统一清理
fn cleanup(
    state: &Arc<AppState>,
    room: Option<&Arc<super::room::Room>>,
    me: Option<&ClientId>,
    rid: Option<&RoomId>,
) {
    if let (Some(r), Some(c)) = (room, me) {
        debug!(client = %c, "connection closed, leaving room");
        leave_room(state, r, c.clone(), rid);
    }
}
