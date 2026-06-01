//! vauid is a rtc-server based on webrtc-rtc
//!

use axum::{
    Router,
    extract::{WebSocketUpgrade, ws::Message},
    http::StatusCode,
    response::Response,
    routing::get,
    serve,
};
use log::info;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::TcpListener;

// mod log;

mod core;

#[tokio::main]
async fn main() {
    info!("vauid is starting...");

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);

    let app = Router::new().route("/ws", get(ws_hello));

    info!("vauid is listening on {}", addr);

    let listener = TcpListener::bind(addr).await.unwrap();
    let _ = serve(listener, app).await;
}

async fn ws_hello(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(async move |mut ws| {
        while let Some(Ok(msg)) = ws.recv().await {
            if let Message::Text(text) = msg {
                let _ = ws
                    .send(Message::Text(format!("Hello, you said: {}", text).into()))
                    .await;
            }
        }
    })
}

// use axum::{
//     extract::WebSocketUpgrade,
//     response::Response,
//     ws::{Message, WebSocket},
//     Extension,
// };
// use serde::Deserialize;
// use std::sync::Arc;
// use webrtc::peer_connection::{
//     configuration::RTCConfiguration,
//     sdp::SessionDescription,
// };

// #[derive(Deserialize)]
// pub struct WsMessage {
//     room: String,
//     offer: String,
// }

// pub async fn ws_handler(ws: WebSocketUpgrade) -> Response {
//     ws.on_upgrade(handle_socket)
// }

// async fn handle_socket(mut socket: WebSocket) {
//     let api = webrtc_api();
//     let peer = api.new_peer_connection(RTCConfiguration::default()).await.unwrap();
//     let peer = Arc::new(peer);

//     // 监听 Track
//     let peer_clone = Arc::clone(&peer);
//     peer.on_track(Box::new(move |track, _| {
//         let peer = Arc::clone(&peer_clone);
//         Box::pin(async move {
//             let room = get_or_create_room("default");
//             room.router.forward_track(track, &room).await;
//         })
//     }));

//     // 处理信令
//     while let Some(Ok(msg)) = socket.recv().await {
//         if let Message::Text(text) = msg {
//             let msg: WsMessage = serde_json::from_str(&text).unwrap();
//             let room = get_or_create_room(&msg.room);

//             // 设置 Offer
//             peer.set_remote_description(SessionDescription::offer(msg.offer)).await.unwrap();

//             // 创建 Answer
//             let answer = peer.create_answer(None).await.unwrap();
//             peer.set_local_description(answer.clone()).await.unwrap();

//             // 加入房间
//             let peer_id = uuid::Uuid::new_v4().to_string();
//             room.peers.insert(peer_id, Arc::clone(&peer));

//             // 回传 Answer
//             let _ = socket.send(Message::Text(serde_json::to_string(&answer.sdp).unwrap())).await;
//         }
//     }
// }
