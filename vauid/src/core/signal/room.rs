//! # 房间管理
//!
//! 内存态房间（P2P 阶段）：管理房间内客户端集合，并提供定向/广播发送能力。
//! 服务器在此阶段仅作为"介绍人"中继 SDP/ICE，媒体流不经过服务器。

use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::mpsc;

use vauid_shared::proto::{ClientId, RoomId, ServerEvent};

/// 加入房间失败的原因
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JoinError {
    /// 客户端 ID 在房间内已存在
    ClientIdTaken,
}

/// 单个房间。
///
/// `peers` 维护 `client_id -> 该客户端的信令事件通道`。
/// 通道发送失败（对端断开/已离开）时静默丢弃，由对端自身的清理逻辑负责。
pub struct Room {
    /// 房间 ID（预留：拓扑决策与可观测性指标使用）
    #[allow(dead_code)]
    id: RoomId,
    peers: DashMap<ClientId, mpsc::UnboundedSender<ServerEvent>>,
}

impl Room {
    /// 创建房间
    pub fn new(id: RoomId) -> Self {
        Self {
            id,
            peers: DashMap::new(),
        }
    }

    /// 返回房间 ID（预留：后续拓扑切换/指标使用）
    #[allow(dead_code)]
    pub fn id(&self) -> &RoomId {
        &self.id
    }

    /// 客户端加入房间。
    ///
    /// 成功时返回 join 前房间内已有的其他客户端 ID 列表（用于给新客户端发 `joined`）。
    /// 若客户端 ID 已存在则返回 `JoinError::ClientIdTaken`。
    pub fn join(
        &self,
        client: &ClientId,
        tx: mpsc::UnboundedSender<ServerEvent>,
    ) -> Result<Vec<ClientId>, JoinError> {
        let existing: Vec<ClientId> = self
            .peers
            .iter()
            .map(|e| e.key().clone())
            .collect();

        if self.peers.insert(client.clone(), tx).is_some() {
            return Err(JoinError::ClientIdTaken);
        }
        Ok(existing)
    }

    /// 客户端离开房间。返回房间是否已空（用于触发房间清理）。
    pub fn leave(&self, client: &ClientId) -> bool {
        self.peers.remove(client);
        self.peers.is_empty()
    }

    /// 定向发送给某个客户端。目标不存在或通道已关闭时静默丢弃。
    pub fn send_to(&self, to: &ClientId, evt: ServerEvent) {
        if let Some(tx) = self.peers.get(to) {
            let _ = tx.send(evt);
        }
    }

    /// 向房间内除 `skip` 外的所有客户端广播。
    pub fn broadcast_except(&self, skip: &ClientId, evt: ServerEvent) {
        for entry in &self.peers {
            if entry.key() != skip {
                let _ = entry.value().send(evt.clone());
            }
        }
    }

    /// 当前客户端数量
    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }
}

/// 房间注册表：`room_id -> Room` 的全局映射。
///
/// 单机阶段用内存 `DashMap`，后续可替换为 redis 实现（预留 trait 抽象）。
#[derive(Clone, Default)]
pub struct RoomRegistry {
    rooms: Arc<DashMap<RoomId, Arc<Room>>>,
}

impl RoomRegistry {
    /// 创建空注册表
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取或创建房间
    pub fn get_or_create(&self, id: &RoomId) -> Arc<Room> {
        self.rooms
            .entry(id.clone())
            .or_insert_with(|| Arc::new(Room::new(id.clone())))
            .clone()
    }

    /// 若房间已空则将其从注册表中移除（清理时调用）
    pub fn cleanup(&self, id: &RoomId) {
        self.rooms
            .remove_if(id, |_, room| room.peer_count() == 0);
    }

    /// 当前活跃房间数（预留：可观测性指标使用）
    #[allow(dead_code)]
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }
}
