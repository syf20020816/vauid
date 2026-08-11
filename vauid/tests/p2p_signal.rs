//! # P2P 信令集成测试
//!
//! 覆盖 P2P MVP 的核心链路：
//! - join：`joined` 回执 + 房间内已有客户端列表
//! - 新客户端加入：房间内其他人收到 `peer_joined`
//! - SDP/ICE 定向中继：`offer` / `answer` / `ice`
//! - 断开与显式 leave：房间内其他人收到 `peer_left`

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{Router, routing::get};
use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use vauid::core::signal::{server::ws_handler, AppState};
use vauid_shared::proto::{ServerEvent, SignalMessage};

/// 启动测试服务器（随机端口），返回地址
async fn spawn_server() -> SocketAddr {
    let state = Arc::new(AppState::new());
    let app = Router::new().route("/ws", get(ws_handler)).with_state(state);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind test listener");
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve test server");
    });

    addr
}

/// 建立 WS 连接
async fn connect(addr: SocketAddr) -> tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
> {
    let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}/ws"))
        .await
        .expect("connect ws");
    ws
}

/// 发送一条信令消息（JSON）
async fn send(ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>, msg: &SignalMessage) {
    let text = serde_json::to_string(msg).unwrap();
    ws.send(Message::Text(text.into())).await.expect("send");
}

/// 读取下一条服务器事件（JSON），并解析为 `ServerEvent`
async fn recv_event(
    ws: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> ServerEvent {
    let msg = ws.next().await.expect("stream alive").expect("no ws error");
    match msg {
        Message::Text(text) => serde_json::from_str(&text).expect("parse ServerEvent"),
        other => panic!("unexpected ws message: {other:?}"),
    }
}

/// join → joined / peer_joined 广播
#[tokio::test]
async fn join_broadcasts_peer_joined() {
    let addr = spawn_server().await;

    let mut a = connect(addr).await;
    send(
        &mut a,
        &SignalMessage::Join {
            room: "r1".into(),
            client: "a".into(),
        },
    )
    .await;
    assert_eq!(
        recv_event(&mut a).await,
        ServerEvent::Joined {
            you: "a".into(),
            clients: vec![],
        }
    );

    // B 加入：B 收到 joined(含 A)，A 收到 peer_joined
    let mut b = connect(addr).await;
    send(
        &mut b,
        &SignalMessage::Join {
            room: "r1".into(),
            client: "b".into(),
        },
    )
    .await;
    assert_eq!(
        recv_event(&mut b).await,
        ServerEvent::Joined {
            you: "b".into(),
            clients: vec!["a".into()],
        }
    );
    assert_eq!(
        recv_event(&mut a).await,
        ServerEvent::PeerJoined { client: "b".into() }
    );
}

/// offer / answer / ice 定向中继
#[tokio::test]
async fn relays_sdp_and_ice_between_peers() {
    let addr = spawn_server().await;
    let mut a = connect(addr).await;
    let mut b = connect(addr).await;

    for (ws, id) in [(&mut a, "a"), (&mut b, "b")] {
        send(
            ws,
            &SignalMessage::Join {
                room: "r2".into(),
                client: id.into(),
            },
        )
        .await;
        let _ = recv_event(ws).await; // joined
    }
    // A 收到 peer_joined B
    assert_eq!(
        recv_event(&mut a).await,
        ServerEvent::PeerJoined { client: "b".into() }
    );

    // A → offer → B
    send(
        &mut a,
        &SignalMessage::Offer {
            to: "b".into(),
            sdp: vauid_shared::proto::SessionDescription {
                sdp_type: "offer".into(),
                sdp: "v=0 fake-sdp".into(),
            },
        },
    )
    .await;
    eprintln!("[relays] sent offer from a to b");
    assert_eq!(
        recv_event(&mut b).await,
        ServerEvent::Offer {
            from: "a".into(),
            sdp: vauid_shared::proto::SessionDescription {
                sdp_type: "offer".into(),
                sdp: "v=0 fake-sdp".into(),
            },
        }
    );
    eprintln!("[relays] b received offer");

    // B → answer → A
    send(
        &mut b,
        &SignalMessage::Answer {
            to: "a".into(),
            sdp: vauid_shared::proto::SessionDescription {
                sdp_type: "answer".into(),
                sdp: "v=0 fake-answer".into(),
            },
        },
    )
    .await;
    assert_eq!(
        recv_event(&mut a).await,
        ServerEvent::Answer {
            from: "b".into(),
            sdp: vauid_shared::proto::SessionDescription {
                sdp_type: "answer".into(),
                sdp: "v=0 fake-answer".into(),
            },
        }
    );

    // A → ice → B
    send(
        &mut a,
        &SignalMessage::Ice {
            to: "b".into(),
            candidate: vauid_shared::proto::IceCandidate {
                candidate: "candidate:1 1 udp 2130706431 192.0.2.1 5000 typ host".into(),
                sdp_mid: Some("0".into()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            },
        },
    )
    .await;
    assert_eq!(
        recv_event(&mut b).await,
        ServerEvent::Ice {
            from: "a".into(),
            candidate: vauid_shared::proto::IceCandidate {
                candidate: "candidate:1 1 udp 2130706431 192.0.2.1 5000 typ host".into(),
                sdp_mid: Some("0".into()),
                sdp_mline_index: Some(0),
                username_fragment: None,
            },
        }
    );
}

/// 连接断开 → 房间内其他人收到 peer_left
#[tokio::test]
async fn disconnect_broadcasts_peer_left() {
    let addr = spawn_server().await;
    let mut a = connect(addr).await;
    let mut b = connect(addr).await;

    for (ws, id) in [(&mut a, "a"), (&mut b, "b")] {
        send(
            ws,
            &SignalMessage::Join {
                room: "r3".into(),
                client: id.into(),
            },
        )
        .await;
        let _ = recv_event(ws).await;
    }
    let _ = recv_event(&mut a).await; // peer_joined b

    // A 直接断开
    drop(a);
    assert_eq!(
        recv_event(&mut b).await,
        ServerEvent::PeerLeft { client: "a".into() }
    );
}

/// 显式 leave → 房间内其他人收到 peer_left，且 A 连接被关闭
#[tokio::test]
async fn explicit_leave_broadcasts_peer_left() {
    let addr = spawn_server().await;
    let mut a = connect(addr).await;
    let mut b = connect(addr).await;

    for (ws, id) in [(&mut a, "a"), (&mut b, "b")] {
        send(
            ws,
            &SignalMessage::Join {
                room: "r4".into(),
                client: id.into(),
            },
        )
        .await;
        let _ = recv_event(ws).await;
    }
    let _ = recv_event(&mut a).await; // peer_joined b

    send(&mut a, &SignalMessage::Leave).await;

    // B 收到 peer_left
    assert_eq!(
        recv_event(&mut b).await,
        ServerEvent::PeerLeft { client: "a".into() }
    );

    // A 的连接被服务器关闭（收到 Close 帧或流结束）
    tokio::select! {
        r = a.next() => {
            match r {
                Some(Ok(Message::Close(_))) => {}
                Some(Ok(_)) => panic!("expected close frame"),
                Some(Err(_)) => {}
                None => {}
            }
        }
        _ = tokio::time::sleep(std::time::Duration::from_millis(500)) => {
            panic!("connection was not closed after leave");
        }
    }
}

/// 重复客户端 ID join 被拒绝（不发 joined）
#[tokio::test]
async fn duplicate_client_id_join_rejected() {
    let addr = spawn_server().await;
    let mut a = connect(addr).await;
    let mut b = connect(addr).await;

    for (ws, id) in [(&mut a, "a"), (&mut b, "b")] {
        send(
            ws,
            &SignalMessage::Join {
                room: "r5".into(),
                client: id.into(),
            },
        )
        .await;
        let _ = recv_event(ws).await;
    }
    let _ = recv_event(&mut a).await; // peer_joined b

    // B 再次以 "a" 身份 join 应被拒绝：B 不应再收到任何事件
    send(
        &mut b,
        &SignalMessage::Join {
            room: "r5".into(),
            client: "a".into(),
        },
    )
    .await;

    // 等待一小段窗口，断言 B 无新事件（而非永久阻塞等待）
    tokio::select! {
        evt = b.next() => panic!("unexpected event after rejected join: {evt:?}"),
        _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
    }
}
