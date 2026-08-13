//! RTC 协议相关结构体
//! 包含了 RTC 协议中使用的结构体，如会话描述、ICE Candidate 等
//! 参考了Livekit定义，但在此基础上进行了修改和优化
//! tid: 统一为 track_id 表示轨道ID
//! sid: 统一为 server_id 表示服务器ID
//! cid: 统一为 client_id 表示客户端ID
//! pid: 统一为 participant_id 表示参与者ID

use std::collections::HashMap;

use crate::proto::{
    ClientId, DisconnectReason,
    models::{
        BackupCodecPolicy, DataTrackInfo, Encryption, PacketTrailerFeature, ParticipantTrack,
        Track, TrackInfo, VideoLayer, VideoLayerMode, VideoQuality,
    },
};

pub type IceCandidate = String;

/// SDP 会话描述
pub struct SessionDescription {
    /// 会话描述类型
    pub ty: SessionDescType,
    /// 完整的 SDP 文本字符串
    pub sdp: String,
    /// 会话ID
    pub id: String,
    /// MID (Media ID) 到 Track ID 的映射表，用于在 Simulcast 场景下关联媒体描述与轨道
    pub mid_to_track_id: HashMap<String, String>,
}

/// SDP 会话描述类型
pub enum SessionDescType {
    /// SDP offer
    Offer,
    /// SDP answer
    Answer,
    /// SDP pre-answer
    PrAnswer,
    /// SDP rollback
    Rollback,
}

/// ICE Candidate 结构体
pub struct TrickleRequest {
    /// ICE Candidate 序列化字符串
    pub candidate: IceCandidate,
    /// 目标会话ID
    pub target: SignalTarget,
    /// 是否为最后一个 ICE Candidate
    pub is_final: bool,
}

pub enum SignalTarget {
    /// Publisher 会话
    Publisher,
    /// Subscriber 会话
    Subscriber,
}

/// # 添加新轨道请求
/// ## 发布标准摄像头轨道
/// ```
/// cid: "my-camera-1"
/// name: "camera"
/// type: VIDEO
/// width: 1920
/// height: 1080
/// source: CAMERA
/// encryption: GCM
/// stream: "main-stream"
/// ```
/// ## 发布 Simulcast 轨道（多层级编码）
/// ```
/// cid: "my-camera-2"
/// type: VIDEO
/// simulcast_codecs: [
///   { codec: "VP8", layers: [{quality: HIGH, width: 1920, height: 1080, bitrate: 3000000},
///                            {quality: MEDIUM, width: 1280, height: 720, bitrate: 1500000}] },
///   { codec: "H264", layers: [{quality: HIGH, width: 1920, height: 1080, bitrate: 2500000}] }
/// ]
/// backup_codec_policy: SIMULCAST
/// ```
pub struct AddTrackRequest {
    /// 轨道的客户端 ID，用于在接收到 RTC 轨道时进行匹配
    pub cid: ClientId,
    /// 服务端已分配的轨道 ID。当需要向 已存在的轨道
    /// 发布新的编解码器（如添加 Simulcast 层）时使用，
    /// 而非创建全新轨道
    pub sid: String,
    /// 轨道的名称，用于在会话描述中引用该轨道
    pub name: String,
    /// 轨道类型: Data, Audio, Video
    pub ty: Track,
    /// 单轨道多空间层配置 — 为传统 Simulcast 方式，
    /// 在一条轨道上定义多个空间/时间编码层（不同分辨率）
    pub layers: Vec<VideoLayer>,
    /// 多编解码器 Simulcast — 支持同一轨道使用不同编解码器（如 VP8 + H264），
    /// 每个 SimulcastCodec 指定 codec 、 cid 、 layers 和 video_layer_mode
    pub simulcast_codecs: Vec<SimulcastCodec>,
    /// 备用编解码器策略，出现无法支持轨道主编解码器时的处理策略
    pub backup_codec_policy: BackupCodecPolicy,
    /// 端到端加密类型
    pub encryption: Encryption,
    /// 流分组标识 — 将相关轨道归为同一组（如摄像头+麦克风一组、屏幕共享+屏幕音频一组）。
    /// 不填时服务器会根据 source 自动推断分组
    pub stream: String,
    /// 数据包 Trailer 特性 — 用于在数据包末尾添加额外信息，如校验和、序列号等
    /// 用于在 E2EE 场景下为每个 RTP 包附加可识别的尾部数据
    pub packet_trailer_features: Vec<PacketTrailerFeature>,
}

pub struct SimulcastCodec {
    /// 编解码器名称，如 "VP8" 或 "H264"
    pub codec: String,
    /// 编解码器对应的轨道 ID
    pub tid: String,
    /// 编解码器对应的视频层配置
    pub layers: Vec<VideoLayer>,
    /// 编解码器对应的视频层模式，见 VideoLayerMode
    pub video_layer_mode: VideoLayerMode,
}

pub struct MuteTrackRequest {
    /// 轨道 ID
    pub tid: String,
    /// 是否静音
    pub muted: bool,
}

pub struct UpdateSubscription {
    /// 要订阅的轨道 ID 列表
    pub tids: Vec<String>,
    /// 是否订阅
    pub subscribe: bool,
    pub participant_tracks: Vec<ParticipantTrack>,
}

pub struct UpdateTrackSettings {
    /// 要更新的轨道 ID 列表
    pub tids: Vec<String>,
    /// 是否禁用轨道
    pub disabled: bool,
    /// 视频质量
    pub quality: VideoQuality,
    pub height: u32,
    pub width: u32,
    /// 视频轨道的帧率
    pub fps: u32,
    /// 订阅优先级。1 为最高优先级（0 表示未设置）
    /// 若未设置，服务器将根据订阅顺序分配优先级
    /// 服务器将按以下方式使用优先级：
    /// 1. 当订阅的轨道数量超过每个参与者的订阅限制时，服务器将
    ///    暂停优先级最低的轨道
    /// 2. 当网络拥塞时，服务器将优先为高优先级轨道分配可用带宽；
    ///    优先级最低的轨道可能会被暂停
    pub priority: u32,
}

pub struct RegionSettings {
    pub region: String,
    pub url: String,
    pub distance: i64,
}

pub struct LeaveRequest {
    pub reason: DisconnectReason,
    pub action: LeaveAction,
    pub regions: Vec<RegionSettings>,
}

pub enum LeaveAction {
    /// 断开会话: 会话将被立即断开，所有轨道将被移除
    /// - 本地调用 leave() 方法
    /// - 远程调用 leave() 方法
    /// - 网络问题导致无法再次Resume/Reconnect
    Disconnect,
    /// 恢复会话: 会话恢复到断开前的状态，链路没断、只是媒体暂停，恢复收发
    /// PeerConnection 连接状态正常、传输通路存活，只是视频 / 音频轨道被暂停
    /// 常见触发：
    /// - 本地调用 track.enabled = false 关闭摄像头 / 静音
    /// - 浏览器后台休眠、页面挂起，媒体流暂时暂停
    /// - SFU 临时暂停向下游客户端转发视频（省电、带宽控制）
    /// 底层行为：
    /// ICE 连接完好，Transport 没有断开，UDP/TCP 通路依旧连通
    /// - 不需要重新做 ICE 收集、候选对协商、DTLS 握手
    /// - 只需要重启轨道发送 RTP 包、开启解码器输出
    /// - 信令几乎不需要交互，或者仅一条简单通知「恢复流」
    Resume,
    /// 重新连接会话: 传输链路已经断开，重新建立连接、ICE 重协商
    /// 触发原因:
    /// - 网络切换：Wi‑Fi → 蜂窝、路由器断连、IP 地址变更
    /// - ICE 连接超时、UDP 端口被 NAT 回收、防火墙干掉会话
    /// - 长时间网络抖动丢包严重，iceConnectionState = disconnected / failed
    /// - 浏览器网络休眠把 UDP socket 销毁
    /// 底层行为:
    /// - ICE 传输通道已经失效，旧的候选对不可用
    /// - WebRTC 触发 ICE 重启（ICE‑restart），重新收集本机 ICE 候选
    /// - 通过信令交换新的 SDP，重新协商 DTLS 握手、传输参数
    /// - 重建 RTP 传输上下文，重置 SRTP 密钥、SSRC 上下文
    /// > 严重的时候会新建底层 RTCTransport
    Reconnect,
}

pub struct TrackPermission {
    /// 轨道所属参与者的参与者 ID
    pub pid: String,
    pub all_tracks: bool,
    pub tids: Vec<String>,
}

pub struct SubscriptionPermission {
    pub all_participants: bool,
    pub track_permissions: Vec<TrackPermission>,
}

pub struct TrackPublishedResponse {
    pub cid: String,
    pub track: TrackInfo,
}

pub struct DataChannelInfo {
    pub id: String,
    pub target: SignalTarget,
    pub msg: String,
}

/// 数据通道接收状态
pub struct DataChannelReceiveState {
    /// 发布者的数据通道 ID
    pub id: String,
    /// 最后接收的序列号
    pub last_seq: u64,
}

pub struct PublishDataTrackResponse {
    pub info: DataTrackInfo,
}

pub struct DataTrackSubscriptionOptions {
    pub fps: Option<u64>,
}

pub struct UpdateDataSubscription {
    pub tid: String,
    pub subscribe: bool,
    pub options: Vec<DataTrackSubscriptionOptions>,
}

pub struct SyncState {
    pub answer: SessionDescription,
    pub subscription: UpdateSubscription,
    pub publish_tracks: Vec<TrackPublishedResponse>,
    pub data_channels: DataChannelInfo,
    pub offer: SessionDescription,
    pub tids_disabled: Vec<String>,
    pub datachannel_receive_states: Vec<DataChannelReceiveState>,
    pub publish_data_tracks: Vec<PublishDataTrackResponse>,
    pub data_subscriptions: Vec<UpdateDataSubscription>,
}

/// 客户端 → 服务端信令
pub enum SignalRequest {
    /// Publisher 发起 Offer
    Offer(SessionDescription),
    /// Subscriber 应答 Answer
    Answer(SessionDescription),
    /// ICE Candidate 结构体
    Trickle(TrickleRequest),
    /// 添加新轨道
    AddTrack(AddTrackRequest),
    /// 静音轨道
    MuteTrack(MuteTrackRequest),
    /// 更新订阅轨道
    Subscription(UpdateSubscription),
    /// 更新轨道设置
    TrackSettings(UpdateTrackSettings),
    /// 离开会话
    Leave(LeaveRequest),
    /// 更新订阅权限
    SubscriptionPermission(SubscriptionPermission),
    /// 同步状态
    SyncState(SyncState),
    /// 模拟场景
    SimulateScenario(SimulateScenario),
    /// Ping 请求 - 客户端触发了向服务器发送的 Ping 请求
    Ping(i64),
    /// 更新参与者元数据
    UpdateMetadata(UpdateParticipantMetadata),
    PingReq(PingRequest),
    /// 更新音频轨道
    UpdateLocalAudioTrack(UpdateLocalAudioTrack),
    /// 更新视频轨道
    UpdateLocalVideoTrack(UpdateLocalVideoTrack),
    /// 发布数据轨道
    PublishDataTrackRequest(PublishDataTrackRequest),
    /// 取消发布数据轨道
    UnpublishDataTrackRequest(UnpublishDataTrackRequest),
    /// 更新数据轨道订阅
    UpdateDataSubscription(UpdateDataSubscription),
    /// 存储数据轨道 blob
    StoreDataBlobRequest(StoreDataBlobRequest),
    /// 获取数据轨道 blob
    GetDataBlobRequest(GetDataBlobRequest),
}
