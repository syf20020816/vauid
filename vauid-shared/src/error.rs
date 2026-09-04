use std::net::SocketAddr;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Other(String),
    #[error("QUIC ERROR: {0}")]
    Quic(QuicError),
    #[error("IO ERROR: {0}")]
    IO(std::io::Error),
    #[error("serde error: {0}")]
    Serde(serde_json::Error),
    #[error("config error: {0}")]
    Config(String),
    #[error("SIGNAL ERROR: {0}")]
    Signal(SignalError),
    #[error("ROOM ERROR: {0}")]
    Room(RoomError),
    #[error("MEDIA ERROR: {0}")]
    Media(MediaError),
}

#[derive(Debug, Error)]
pub enum QuicError {
    #[error("send to addr not found: {addr}, buf: {buf:?})")]
    SendAddrNotFound {
        addr: SocketAddr,
        buf: Option<Vec<u8>>,
    },
    #[error("recv from addr not found: {addr}, buf: {buf:?})")]
    RecvAddrNotFound {
        addr: SocketAddr,
        buf: Option<Vec<u8>>,
    },
    #[error("config error: {0}")]
    Config(String),
}

/// 信令面错误（Slice 4 起接入 TargetNotFound；其余为 Phase 2 双通道信令预留）
#[derive(Debug, Error)]
pub enum SignalError {
    /// 转发目标不存在（连接已断开或未注册）。
    /// Slice 4 起已在服务端转发路径接入（连接从注册表消失后仍待冲洗出站消息）。
    #[error("target not found: {target}")]
    TargetNotFound { target: String },
    /// 未加入房间即发送消息（Phase 2 双通道信令预留）
    #[error("message before join: {reason}")]
    NotJoined { reason: String },
    /// join 时 client id 冲突（Phase 2 双通道信令预留）
    #[error("join conflict, client already exists: {client}")]
    JoinConflict { client: String },
}

/// 房间面错误（Phase 2 房间模型接入前为骨架）
#[derive(Debug, Error)]
pub enum RoomError {
    /// 房间不存在（加入/查询不存在的房间）
    #[error("room not found: {room}")]
    NotFound { room: String },
    /// 满员（小队场景上限 4 人）
    #[error("room full: {room}, max {max}")]
    Full { room: String, max: usize },
    /// 创建房间失败（重名/资源不足等）
    #[error("room create failed: {reason}")]
    CreateFailed { reason: String },
}

/// 媒体面错误（Phase 4 前仅骨架）
#[derive(Debug, Error)]
pub enum MediaError {
    /// 编码失败（编解码器错误）
    #[error("encode failed: {0}")]
    EncodeFailed(String),
    /// 解码失败（帧损坏/不完整）
    #[error("decode failed: {0}")]
    DecodeFailed(String),
    /// 非法 RTP 包（头部/长度校验失败）
    #[error("invalid rtp packet: {0}")]
    RtpPacketInvalid(String),
    /// 抖动缓冲区溢出（乱序/迟到包过多）
    #[error("jitter buffer overflow")]
    JitterBufferOverflow,
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::IO(value)
    }
}

impl From<QuicError> for Error {
    fn from(value: QuicError) -> Self {
        Self::Quic(value)
    }
}

impl From<SignalError> for Error {
    fn from(value: SignalError) -> Self {
        Self::Signal(value)
    }
}

impl From<RoomError> for Error {
    fn from(value: RoomError) -> Self {
        Self::Room(value)
    }
}

impl From<MediaError> for Error {
    fn from(value: MediaError) -> Self {
        Self::Media(value)
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(value: Box<dyn std::error::Error>) -> Self {
        Self::Other(value.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(value: serde_json::Error) -> Self {
        Self::Serde(value)
    }
}

impl From<std::net::AddrParseError> for Error {
    fn from(value: std::net::AddrParseError) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<toml::de::Error> for Error {
    fn from(value: toml::de::Error) -> Self {
        Self::Config(value.to_string())
    }
}

impl From<toml::ser::Error> for Error {
    fn from(value: toml::ser::Error) -> Self {
        Self::Config(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// SignalError 每个变体的 Display 均应包含关键信息
    #[test]
    fn signal_error_display_contains_key_info() {
        let e = SignalError::TargetNotFound {
            target: "conn-7".to_string(),
        };
        assert!(e.to_string().contains("conn-7"), "TargetNotFound 应包含 target");

        let e = SignalError::NotJoined {
            reason: "send before join room-1".to_string(),
        };
        assert!(e.to_string().contains("before join"), "NotJoined 应包含原因关键词");

        let e = SignalError::JoinConflict {
            client: "alpha".to_string(),
        };
        assert!(e.to_string().contains("alpha"), "JoinConflict 应包含 client 名");
    }

    /// RoomError 每个变体的 Display 均应包含关键信息
    #[test]
    fn room_error_display_contains_key_info() {
        let e = RoomError::NotFound {
            room: "squad-1".to_string(),
        };
        assert!(e.to_string().contains("squad-1"), "NotFound 应包含 room 名");

        let e = RoomError::Full {
            room: "squad-2".to_string(),
            max: 4,
        };
        assert!(
            e.to_string().contains("squad-2") && e.to_string().contains("4"),
            "Full 应包含 room 名与 max 上限"
        );

        let e = RoomError::CreateFailed {
            reason: "duplicate".to_string(),
        };
        assert!(e.to_string().contains("duplicate"), "CreateFailed 应包含原因");
    }

    /// MediaError 每个变体的 Display 均应包含关键信息
    #[test]
    fn media_error_display_contains_key_info() {
        let e = MediaError::EncodeFailed("vp8".to_string());
        assert!(e.to_string().contains("vp8"), "EncodeFailed 应包含编解码器信息");

        let e = MediaError::DecodeFailed("bad frame".to_string());
        assert!(e.to_string().contains("bad frame"), "DecodeFailed 应包含原因");

        let e = MediaError::RtpPacketInvalid("bad header".to_string());
        assert!(e.to_string().contains("bad header"), "RtpPacketInvalid 应包含原因");

        let e = MediaError::JitterBufferOverflow;
        assert!(
            e.to_string().contains("jitter"),
            "JitterBufferOverflow 应包含 jitter 关键词"
        );
    }

    /// 三个子错误经 From 转换后应落到 Error 对应类别变体
    #[test]
    fn from_sub_errors_map_to_matching_variants() {
        let e: Error = SignalError::TargetNotFound {
            target: "conn-1".to_string(),
        }
        .into();
        assert!(
            matches!(e, Error::Signal(SignalError::TargetNotFound { .. })),
            "SignalError 应转换为 Error::Signal"
        );

        let e: Error = RoomError::Full {
            room: "r".to_string(),
            max: 4,
        }
        .into();
        assert!(
            matches!(e, Error::Room(RoomError::Full { .. })),
            "RoomError 应转换为 Error::Room"
        );

        let e: Error = MediaError::JitterBufferOverflow.into();
        assert!(
            matches!(e, Error::Media(MediaError::JitterBufferOverflow)),
            "MediaError 应转换为 Error::Media"
        );
    }

    /// Error 顶层 Display 应带类别前缀，便于日志归因
    #[test]
    fn error_display_has_category_prefix() {
        let e: Error = SignalError::NotJoined {
            reason: "r".to_string(),
        }
        .into();
        assert!(e.to_string().contains("SIGNAL ERROR"), "Signal 类别应带 SIGNAL ERROR 前缀");

        let e: Error = RoomError::NotFound {
            room: "r".to_string(),
        }
        .into();
        assert!(e.to_string().contains("ROOM ERROR"), "Room 类别应带 ROOM ERROR 前缀");

        let e: Error = MediaError::EncodeFailed("e".to_string()).into();
        assert!(e.to_string().contains("MEDIA ERROR"), "Media 类别应带 MEDIA ERROR 前缀");
    }
}