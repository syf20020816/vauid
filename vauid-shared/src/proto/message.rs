//! 统一消息帧协议（骨架）。
//! 信令/文本/媒体/控制同通道传输，按 `ty` 区分；Phase 0 仅使用 Signal/Chat，
//! Media/Control 为媒体面预留。

use serde::{Deserialize, Serialize};

/// 消息类型（对齐 roadMap §4.1）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MsgType {
    /// 信令（payload 为 SignalMessage 的 JSON）
    Signal,
    /// 文本聊天
    Chat,
    /// 媒体帧（Phase 4 起使用，RTP 载荷）
    Media,
    /// 控制消息（拓扑切换/心跳等）
    Control,
}

/// 统一消息帧
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    /// 消息类型
    pub ty: MsgType,
    /// 递增序列号（可靠性/去重）
    pub seq: u64,
    /// 标志位（fin/关键帧/冗余编码等，按位使用）
    pub flags: u8,
    /// 载荷：Chat 为 UTF-8 文本，Signal 为 JSON，Media 为 RTP 载荷
    pub payload: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MsgType 全部 4 个变体均可 JSON round-trip
    #[test]
    fn msg_type_round_trip_all_variants() {
        for ty in [
            MsgType::Signal,
            MsgType::Chat,
            MsgType::Media,
            MsgType::Control,
        ] {
            let bytes = serde_json::to_vec(&ty).expect("serialize MsgType");
            let back: MsgType = serde_json::from_slice(&bytes).expect("deserialize MsgType");
            assert_eq!(back, ty, "MsgType {:?} round-trip 应相等", ty);
        }
    }

    /// Message 覆盖 seq/flags/payload 边界组合，序列化后反序列化应相等
    #[test]
    fn message_round_trip_boundary_combinations() {
        for (seq, flags, payload) in [
            (0u64, 0u8, Vec::new()),
            (0, 0xFF, b"hello vauid".to_vec()),
            (u64::MAX, 0, b"welcome to vauid".to_vec()),
            (u64::MAX, 0xFF, Vec::new()),
        ] {
            let msg = Message {
                ty: MsgType::Chat,
                seq,
                flags,
                payload: payload.clone(),
            };
            let bytes = serde_json::to_vec(&msg).expect("serialize Message");
            let back: Message = serde_json::from_slice(&bytes).expect("deserialize Message");
            assert_eq!(back, msg, "Message (seq={seq}, flags={flags}) round-trip 应相等");
        }
    }
}
