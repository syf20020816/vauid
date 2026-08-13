/// 轨道类型
/// - Data: 数据轨道，用于传输非音频和视频数据
/// - Audio: 音频轨道，用于传输音频数据
/// - Video: 视频轨道，用于传输视频数据
pub enum Track {
    Data,
    Audio(AudioTrack),
    Video(VideoTrack),
}

pub struct AudioTrack {
    /// 是否禁用 RED（冗余编码），用于音频抗丢包
    pub disable_red: bool,
    /// 轨道是否静音
    pub muted: bool,
    /// 轨道的来源
    pub source: TrackSource,
    pub audio_features: Vec<AudioTrackFeature>,
}

/// 音频轨道的特征
/// - STERED: 立体声编码
/// - NoDtx: 不使用 DTX 编码
/// - AutoGainControl: 自动增益控制
/// - EchoCancellation: 回声消除
/// - NoiseSuppression: 噪声抑制
/// - EnhanceNoiseCancellation: 增强回声消除
/// - PreconnectBuffer: 预连接缓冲区
pub enum AudioTrackFeature {
    STERED,
    NoDtx,
    AutoGainControl,
    EchoCancellation,
    NoiseSuppression,
    EnhanceNoiseCancellation,
    PreconnectBuffer,
}

pub struct VideoTrack {
    /// 视频轨道的高度
    pub height: u32,
    /// 视频轨道的宽度
    pub width: u32,
    /// 视频轨道是否静音
    pub muted: bool,
    /// 轨道的来源
    pub source: TrackSource,
}

/// 视频轨道的来源
/// - Unknown: 未知来源
/// - Camera: 相机
/// - Microphone: 麦克风
/// - ScreenShare: 屏幕分享
/// - ScreenShareAudio: 屏幕分享音频
pub enum TrackSource {
    Unknown,
    Camera,
    Microphone,
    ScreenShare,
    ScreenShareAudio,
}

pub struct VideoLayer {
    /// 视频轨道的编码质量
    pub quality: VideoQuality,
    /// 视频轨道的高度
    pub height: u32,
    /// 视频轨道的宽度  
    pub width: u32,
    /// 视频轨道的码率
    pub bitrate: u32,
    /// Synchronization Source，RFC‑3550 RTP 同步源标识
    /// RTP 协议核心标识，每一路独立视频流对应唯一 ssrc
    /// SFU、接收端依靠 ssrc 区分不同视频轨道、不同 simulcast 流、不同 SVC 空间层
    /// RTCP 反馈（RR、SR、NACK、PLI）全部使用 SSRC 指定针对哪一条流做反馈
    /// 接收端解复用 RTP 包：读到 rtp.ssrc，就能归属到对应的 VideoLayer
    pub ssrc: u32,
    /// 视频轨道的空间层编号
    /// 0: 最低质量空间层 （基础层必须解码）
    /// 1: 中质量空间层
    /// 2: 最高质量空间层
    pub spatial_layer: i32,
    /// 视频轨道的 RID
    pub rid: String,
    /// FEC / RTX 重传冗余流 SSRC
    /// 主视频流使用一个 ssrc， 单独开辟一条 RTX 重传流（repair stream），携带丢失数据包的重传包，拥有独立 repair_ssrc
    /// repair_ssrc 存放该视频层对应的重传冗余流的同步源 ID；
    /// SFU 需要转发重传包时，知道该往哪个 repair‑ssrc 发送；
    /// 接收端收到 RTCP‑NACK 丢包反馈，从 repair_ssrc 流获取丢失帧；
    /// 如果该视频层没有开启 RTX 重传，repair_ssrc 一般填 0
    pub repair_ssrc: u32,
}

/// 视频轨道的空间层模式
pub enum VideoLayerMode {
    /// 未使用视频层模式
    UnUsed,
    /// 每个视频流只使用一个空间层
    OneSpatialLayerPerStream,
    /// 多个视频流可以使用多个空间层
    MultipleSpatialLayersPerStream,
    /// 每个视频流可以使用一个空间层，但是空间层编号为 0 的视频层必须解码
    OneSpatialLayerPerStreamIncompleteRTCPSR,
}

/// 视频轨道的编码质量
pub enum VideoQuality {
    Low,
    Medium,
    High,
    Off,
}

/// 发布者处理无法支持轨道主编解码器（primary codec）的订阅者的策略
pub enum BackupCodecPolicy {
    /// 默认行为：轨道优先回退到备用编解码器，所有订阅者将接收备用编解码器；
    /// SFU 会尝试进行编解码器回退，但不保证一定成功
    PreferRegression,
    /// 同时编码/发送主编解码器和备用编解码器
    Simulcast,
    /// 强制轨道回退到备用编解码器；此选项适用于视频会议场景，或发布者带宽/编码算力有限的情况
    Regression,
}

/// 端到端加密类型
pub enum Encryption {
    /// 不使用端到端加密
    None,
    /// 使用 GCM 加密
    GCM,
    /// 自定义加密
    Custom(String),
}

/// 数据包 Trailer 特性 — 用于在数据包末尾添加额外信息，如校验和、序列号等
/// 用于在 E2EE 场景下为每个 RTP 包附加可识别的尾部数据
pub enum PacketTrailerFeature {
    /// 用户时间戳
    PTFUserTimestamp,
    /// 帧 ID
    /// 用于在 E2EE 场景下为每个 RTP 包附加可识别的帧 ID
    /// 用于在 E2EE 场景下为每个 RTP 包附加可识别的用户数据
    PTFFrameId,
    /// 用户数据
    PTFUserData,
}

pub struct ParticipantTrack {
    /// 轨迹所属参与者的参与者 ID
    pub pid: String,
    /// track id
    pub tid: String,
}

pub struct SimulcastCodecInfo {
    /// 编码器的 MIME 类型
    pub mime_type: String,
    pub mid: String,
    /// 编码器的客户端 ID
    pub cid: String,
    pub layers: Vec<VideoLayer>,
    pub video_layer_mode: VideoLayerMode,
    // cid（用于追踪的客户端 ID）在信令（AddTrackRequest）
    // 与 SDP offer 之间可能不同。仅当两者不同时才会填充该字段，
    // 以避免冗余并保持表达简洁。
    pub sdp_cid: String,
}

/// 轨道的版本信息
pub struct TimedVersion {
    /// 轨道的版本时间戳（微秒级）
    pub unix_micro: u64,
    /// 轨道的版本时间戳（毫秒级）
    pub ticks: u64,
}

pub struct TrackInfo {
    /// 轨道的 ID
    pub tid: String,
    /// 轨道的类型
    pub ty: Track,
    pub name: String,
    pub mime_type: String,
    pub mid: String,
    /// 编码器信息
    pub codecs: Vec<SimulcastCodecInfo>,
    /// 端到端加密类型
    pub encryption: Encryption,
    pub stream: String,
    /// 轨道的版本信息
    pub version: TimedVersion,
    pub backup_codec_policy: BackupCodecPolicy,
    pub packet_trailer_features: Vec<PacketTrailerFeature>,
}

/// 数据轨道的帧编码类型
pub enum DataTrackFrameEncoding {
    UnSpecified,
    ROS1,
    CDR,
    Protobuf,
    FlatBuffer,
    CBOR,
    MsgPack,
    Json,
    Custom(String),
}

/// 数据轨道的 schema 编码类型
pub enum DataTrackSchemaEncoding {
    UnSpecified,
    Protobuf,
    FlatBuffer,
    ROS1Msg,
    ROS2Msg,
    ROS2Idl,
    OMGIdl,
    Json,
    Custom(String),
}

pub struct DataTrackSchemaId {
    pub name: String,
    pub encoding: DataTrackSchemaEncoding,
}

pub struct DataTrackInfo {
    pub sid: String,
    pub name: String,
    /// 由客户端指定的 16 位标识符，将附加到由发布者发送的数据包上。
    pub pub_handle: u64,
    pub encryption: Encryption,
    /// 数据轨道的帧编码类型
    pub frame_encoding: Option<DataTrackFrameEncoding>,
    /// 数据轨道的 schema ID
    pub schema: Option<DataTrackSchemaId>,
}

pub enum DataBlobKey {
    Generic(String),
    Schema(DataTrackSchemaId)
}

pub struct DataBlob {
    pub data: Vec<u8>,
    pub key: DataBlobKey
}