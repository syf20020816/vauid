//! # 信令协议 (v1)
//!
//! 客户端与 `vauid` 服务器之间通过 WebSocket 传输的 JSON 信令消息。
//!
//! **P2P 阶段**：服务器仅作为"介绍人"中继 SDP/ICE，媒体流完全走客户端之间的点对点连接。
//!
//! ## 协议约定
//! - 所有消息均为 UTF-8 JSON，顶层字段 `type` 用于区分消息类型。
//! - 客户端必须先发送 `join` 成功（收到 `joined`）后才能发送其他消息。
//! - `offer` / `answer` / `ice` 的 `to` 字段指定目标客户端 ID，由服务器定向中继。


mod room;
mod models;
mod rtc;


use serde::{Deserialize, Serialize};

/// 客户端 ID
pub type ClientId = String;
pub type ServerId = String;

/// 房间 ID
pub type RoomId = String;

/// SDP 会话描述
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionDescription {
    /// SDP 类型：offer / answer / pranswer
    #[serde(rename = "type")]
    pub sdp_type: String,
    /// SDP 文本内容
    pub sdp: String,
}

/// ICE Candidate（RTCIceCandidateInit 的 JSON 子集）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IceCandidate {
    /// candidate 字符串（含 ufrag），形如 "candidate:... "
    pub candidate: String,
    /// 关联的 m-line 的 mid
    #[serde(rename = "sdpMid")]
    pub sdp_mid: Option<String>,
    /// 关联的 m-line 的序号
    #[serde(rename = "sdpMLineIndex")]
    pub sdp_mline_index: Option<u32>,
    /// ICE username fragment
    pub username_fragment: Option<String>,
}

/// 客户端 → 服务器 信令消息
///
/// 注意：`offer` / `answer` / `ice` 的 SDP/ICE 载荷使用**嵌套对象**承载，
/// 避免载荷字段（如 SDP 的 `type`）与顶层消息类型标签 `type` 冲突。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SignalMessage {
    /// 加入房间。`client` 由客户端自报（需保证房间内唯一，重复则被拒）。
    Join {
        /// 目标房间
        room: RoomId,
        /// 客户端 ID
        client: ClientId,
    },
    /// 离开当前房间
    Leave,
    /// 向房间内某客户端发送 SDP Offer
    Offer {
        /// 目标客户端 ID
        to: ClientId,
        /// SDP 描述
        sdp: SessionDescription,
    },
    /// 向房间内某客户端发送 SDP Answer
    Answer {
        /// 目标客户端 ID
        to: ClientId,
        sdp: SessionDescription,
    },
    /// 向房间内某客户端中继 ICE Candidate
    Ice {
        /// 目标客户端 ID
        to: ClientId,
        candidate: IceCandidate,
    },
}

/// 服务器 → 客户端 信令事件
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerEvent {
    /// join 成功确认，携带房间内已有客户端列表
    Joined {
        /// 当前客户端 ID
        you: ClientId,
        /// join 时房间内已存在的其他客户端
        clients: Vec<ClientId>,
    },
    /// 房间内新客户端加入
    PeerJoined { client: ClientId },
    /// 房间内客户端离开
    PeerLeft { client: ClientId },
    /// 房间内某客户端发来的 SDP Offer
    Offer {
        /// 发送方客户端 ID
        from: ClientId,
        sdp: SessionDescription,
    },
    /// 房间内某客户端发来的 SDP Answer
    Answer {
        /// 发送方客户端 ID
        from: ClientId,
        sdp: SessionDescription,
    },
    /// 房间内某客户端中继的 ICE Candidate
    Ice {
        /// 发送方客户端 ID
        from: ClientId,
        candidate: IceCandidate,
    },
}

/// 断开连接的原因
pub enum DisconnectReason {
  UnknownReason,
  /// 客户端发起了断开连接
  ClientInitiated,
  /// 房间内已存在相同身份的客户端，无法加入
  DuplicateIdentity,
  /// 服务器实例正在关闭，无法继续服务
  ServerShutdown,
  /// 房间内客户端被移除，无法继续服务
  ParticipantRemoved,
  /// 房间已被删除，无法继续服务
  RoomDeleted,
  /// 客户端尝试恢复会话，但服务器未意识到
  StateMismatch,
  /// 客户端无法完全连接
  JoinFailure,
  /// 服务器请求将连接迁移到其他客户端
  Migration,
  /// 信令 WebSocket 已意外关闭
  SignalClose,
  /// 房间内所有客户端已离开
  RoomClosed,
  /// SIP callee 未及时响应
  UserUnavailable,
  /// SIP callee rejected the call (busy)
  UserRejected,
  /// SIP protocol failure or unexpected response
  SIPTrunkFailure,
  /// server timed out a participant session
  ConnectionTimeout,
  /// media stream failure or media timeout
  MediaFailure,
  /// agent encountered an error
  AgentError,
}