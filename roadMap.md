# `vauid` 技术实现文档与研发周期规划

> 版本：v0.1 · 2026-08-10
> 关联文档：[README.md](./README.md)（项目愿景与架构）、[TODO.md](./TODO.md)（基础任务清单）
> 本文档把 README 的设计愿景拆解为**可执行的技术决策、模块契约、状态机与分周期交付计划**。

---

## 0. 文档目的与范围

本文档面向 `vauid` 的实现工程，回答四个问题：

1. **现状**：当前代码库到哪一步了？哪些是空壳？
2. **决策**：README 选型与实际依赖冲突时怎么收口？关键岔路怎么选？
3. **契约**：每个模块的边界、关键类型、信令协议 schema 是什么？
4. **节奏**：分几个周期、每个周期交付什么、验收标准与风险是什么？

范围覆盖：信令面、P2P Mesh、SFU 引擎、Make-Before-Break 拓扑切换、生产加固、QUIC 实验通道、前端 SDK 联动。不含：商业化的计费/多租户/水平扩缩容（留待后续 v0.2）。

---

## 1. 现状评估（Codebase Audit）

| 维度 | 现状 | 差距 |
| :--- | :--- | :--- |
| 工作区 | `vauid` + `vauid-shared` 两 crate，`resolver = "3"` | OK |
| 入口 | [main.rs](./vauid/src/main.rs) 仅 `/ws` echo | 无信令协议、无路由、无状态 |
| 模块树 | `core/{room, rtc, signal}` + `signal/{room, participant, sdp, ice}` 已声明 | **全部空文件**，仅有模块注释 |
| 依赖 | `axum 0.8`、`tokio`、`webrtc-rs 0.17`、`tokio-tungstenite`、`thiserror`、`toml` | 与 README 选型 `str0m`/`quinn` **不一致**，需收口 |
| 共享层 | `vauid-shared` 仅占位 `Error::Other(String)` + 空 `conf` | 无协议类型、无配置加载 |
| 测试 / CI | 无 | 无单测、无集成测、无 CI |
| 可观测性 | 仅 `log` crate | 无 tracing、无 metrics、无结构化日志 |

**结论**：项目处于"地基已挖坑、未浇筑"阶段。可以、也应该在动媒体面之前先把信令协议、配置、错误体系、可观测性补齐——否则后续每个模块都会重复造胶水。

---

## 2. 关键技术决策（Decision Gates）

### 2.1 【必须先拍】WebRTC 栈：`str0m` vs `webrtc-rs`

| 维度 | `webrtc-rs 0.17`（当前 Cargo 已锁） | `str0m`（README 选型） |
| :--- | :--- | :--- |
| API 风格 | 回调式 `RTCPeerConnection`，仿 libwebrtc | 单线程 `Rtc` 实例 + 手动 `handle_input`/`poll_output` 事件循环 |
| SFU 适配 | 偏向"一客户端一 PeerConnection"，资源重 | 天然为 SFU 设计：一个 `Rtc` 承载多路 track，轻量 |
| RTCP 细粒度控制 | 暴露层级较浅，PLI/FIR/Simulcast 控制需绕路 | 直接构造 `Rtcp` 消息注入，**Make-Before-Break 的 PLI 强制关键帧依赖此能力** |
| Simulcast | 支持但 API 繁琐 | 原生三层 encoding 支持，readable |
| 生产成熟度 | 已有产出案例 | 已有产出案例，社区活跃 |
| 学习曲线 | 略低（API 像 JS WebRTC） | 略高（需理解事件驱动模型） |

**建议：改用 `str0m`。** 理由：

1. README 已明确选型，且选型理由（"极适合定制 SFU 逻辑"）成立；
2. Make-Before-Break 的 Phase 2 需要服务器**主动、精确、批量**向所有 Sender 注入 PLI/FIR，str0m 的 `Rtc` 直接 `handle_input` RTCP 包即可，webrtc-rs 需要走 callback 间接路径，时序不可控；
3. 一个 `Rtc` 实例承载多 publisher/subscriber，SFU 内存占用显著低于 webrtc-rs 的"每客户端一 PC"模型；
4. Simulcast 在 str0m 中是一等公民，Phase 3 的 Overlap 降级发送低分辨率层会更顺。

**动作项（Phase 0）**：

- [ ] 从 [vauid/Cargo.toml](./vauid/Cargo.toml) 移除 `webrtc = "0.17.1"`，引入 `str0m = "0.X"`；
- [ ] 删除 [main.rs](./vauid/src/main.rs) 中注释掉的 webrtc-rs 示例代码（避免误导）；
- [ ] 评估 `str0m` 最新稳定版本与 MSRV，写入 `rust-toolchain.toml`。

> 若团队评估后坚持 webrtc-rs，需在 Phase 3 额外预留 1 周做 PLI/FIR 注入的可行性验证，风险登记见 §8。

### 2.2 信令传输：`axum::ws` vs `tokio-tungstenite`

当前两个都在依赖里。**建议**：统一用 `axum::ws`（与 axum 路由/提取器天然集成，连接生命周期由 axum 管理），从 Cargo 移除 `tokio-tungstenite`，减少一层抽象。QUIC 信令通道在 Phase 5 由 `quinn` 独立承担，不走 WS。

### 2.3 房间状态存储：内存 `DashMap` 起步，`redis` 留接口

单机阶段用 `DashMap<RoomId, Room>`，但 `RoomStore` trait 抽象好，Phase 4 之后可平滑替换为 `redis` 实现，支持多节点。**不要**在 MVP 阶段引入 redis，徒增运维负担。

### 2.4 配置与错误体系

- 配置：`vauid-shared::conf` 用 `toml` + `serde`，启动时加载 `config.toml`，支持环境变量覆盖。结构：`[server]`、`[rtc]`、`[turn]`、`[log]`。
- 错误：把 `Error::Other(String)` 扩成分类错误树（Signal/Rtc/Room/Config/Store），实现 `From<T>` 转换，统一在信令层映射为 WS close code / JSON error payload。

---

## 3. 系统架构与模块契约

### 3.1 目标 crate 结构

```
vauid-shared/          # 协议类型、配置、错误——前后端可共享
  src/
    conf/              # 配置加载
    error.rs           # 统一错误树
    proto/             # 信令协议 schema（新增）
      mod.rs
      signal.rs        # SignalMessage 枚举
      event.rs         # ServerEvent 枚举
      topology.rs      # TopologyState、SwitchPhase
    ids.rs             # RoomId/ClientId/TrackId newtype（新增）

vauid/
  src/
    main.rs            # 入口：加载配置、启动 axum
    log/               # tracing 初始化
    core/
      mod.rs
      signal/          # 信令面
        mod.rs
        server.rs      # WS 升级、消息分发（新增）
        room.rs        # 房间生命周期
        participant.rs # 参与者状态机
        sdp.rs         # SDP 解析/改写（mid 协商、codec 限定）
        ice.rs         # ICE candidate 中继
      rtc/             # 媒体面
        mod.rs
        engine.rs      # str0m Rtc 实例池（新增）
        publisher.rs   # 上行 track 管理
        subscriber.rs  # 下行 track 路由
        forwarder.rs   # RTP 转发核心循环（新增）
        rtcp.rs        # PLI/FIR/NACK/REMB 注入（新增）
        simulcast.rs   # 三层 encoding 选择（新增）
      topology/        # 拓扑决策引擎（新增）
        mod.rs
        fsm.rs         # Make-Before-Break 状态机
        coordinator.rs # 阈值检测、广播切换事件
      store/           # RoomStore trait + 内存实现（新增）
      turn/            # STUN/TURN 配置（新增）
```

### 3.2 信令协议 v1（Phase 0 落地，JSON over WS）

**Client → Server**：

```jsonc
// 加入房间
{ "type": "join", "room": "room_xxx", "client": "client_a", "token": "..." }
// 离开
{ "type": "leave" }
// P2P Mesh 阶段：SDP/ICE 中继给房间内其他客户端
{ "type": "offer",  "to": "client_b", "sdp": "v=0..." }
{ "type": "answer", "to": "client_a", "sdp": "v=0..." }
{ "type": "ice",     "to": "client_b", "candidate": {...} }
// SFU 阶段：发布/订阅
{ "type": "publish",  "sdp": "...", "kind": "audio|video" }
{ "type": "subscribe","track": "track_xxx", "sdp": "..." }
// Make-Before-Break 握手
{ "type": "sfu_ready", "track": "track_xxx" }   // Phase 3 ACK
{ "type": "p2p_end_ack" }                        // Phase 5 ACK
```

**Server → Client**：

```jsonc
{ "type": "joined", "room": "...", "you": "client_a", "clients": ["client_b"] }
{ "type": "peer_joined", "client": "client_b" }
{ "type": "peer_left",   "client": "client_b" }
{ "type": "offer",  "from": "client_b", "sdp": "..." }   // P2P 介绍
{ "type": "answer", "from": "client_b", "sdp": "..." }
{ "type": "ice",    "from": "client_b", "candidate": {...} }
{ "type": "track_published", "track": "track_xxx", "owner": "client_b" }
// 拓扑切换（核心）
{ "type": "topology_change", "phase": "prepare",  "sfu_endpoint": "wss://..." }
{ "type": "topology_change", "phase": "overlap" }  // 触发双发 + 请求关键帧
{ "type": "topology_change", "phase": "execute" }  // 前端做 opacity 切换
{ "type": "topology_change", "phase": "teardown" } // 销毁 P2P PC
{ "type": "topology_change", "phase": "keep_p2p", "client": "client_c" } // 降级
```

**关键设计**：所有 `topology_change` 消息复用同一 `phase` 字段，前端用单一 handler 路由，降低协议面复杂度。

### 3.3 核心类型草图

```rust
// vauid-shared/src/proto/topology.rs
pub enum TopologyState {
    PureP2P,
    PreparingSwitch { pending_acks: HashSet<ClientId> },
    OverlapDualCast,
    PureSFU,
}

pub enum SwitchPhase {
    Prepare,    // 通知客户端预创建 SFU 接收通道
    Overlap,    // 双轨运行，服务器强制 PLI
    Execute,    // 前端 opacity 切换
    Teardown,   // 销毁 P2P
    KeepP2P,    // 降级：某客户端切换失败
}

// vauid/src/core/topology/fsm.rs
pub struct TopologyFsm {
    state: TopologyState,
    overlap_deadline: Instant,    // 默认 2000ms
    pending: HashSet<ClientId>,
}

impl TopologyFsm {
    pub fn on_client_join(&mut self, count: usize) -> Option<SwitchPhase>;  // count==5 → Prepare
    pub fn on_sfu_ready(&mut self, c: ClientId) -> Transition;              // 满 → Execute / 超时 → KeepP2P
    pub fn on_timeout(&mut self) -> Transition;
}
```

---

## 4. Make-Before-Break 实现方案（核心卖点落地）

README §"过继模式"描述的 5 阶段 FSM，在 `vauid` 侧落地为下述时序与注入点。**这是整个项目技术风险最高的一块，Phase 3 单独立项。**

### 4.1 服务器侧时序

| T | 服务器动作 | str0m 落地点 |
| :--- | :--- | :--- |
| T0 | 房间 count == 4，PureP2P | 无 SFU 实例 |
| T1 | 第 5 人加入（count==5，触发阈值，因为第 5 人无法加入 4 人 mesh 而不破坏带宽） | `TopologyFsm` 返回 `Prepare` |
| T2 | 为每个客户端 lazily 创建 `Rtc`（接收方向 transceiver 预创建） | `engine.rs::ensure_rtc(client)` |
| T3 | 广播 `topology_change: prepare`，启动 overlap 定时器（2000ms） | signal 层 |
| T4 | 收到客户端 SFU Offer，完成 SDP 协商 | `sdp.rs` + `Rtc::accept_answer` |
| T5 | 进入 `OverlapDualCast`，**对每个 publisher 的 `Rtc` 注入 PLI** | `rtcp.rs::force_keyframe(publisher)` |
| T6 | 转发 SFU 下行流；客户端开始双收 | `forwarder.rs` |
| T7 | 收齐 `sfu_ready`（或超时） → 广播 `execute` | FSM |
| T8 | 前端 opacity 切换完成（~200ms） → 广播 `teardown` | FSM |
| T9 | 客户端销毁 P2P PC，`p2p_end_ack` | 状态归 PureSFU |

**阈值说明**：README 写的是"第 6 人加入"（count==6），但 4 人 mesh 下第 5 人加入时上行已是 4 路，再加一人会突破带宽预算。**建议阈值 = 5**（count > 4 即触发），与 README 的"≤4 人 P2P"语义一致。需与前端确认。

### 4.2 PLI 强制关键帧（绕过"绿屏延迟"）

```rust
// vauid/src/core/rtc/rtcp.rs（草图）
pub fn force_keyframe(rtc: &mut Rtc, mid: Mid) {
    let pli = Rtcp::pli(mid, /*sender_ssrc*/0, /*media_ssrc*/0);
    rtc.handle_input(Input::Rtcp(pli)).ok();
}
```

**注意**：str0m 的 PLI 注入需要正确的 SSRC 映射，依赖 `publisher.rs` 维护 `mid → ssrc` 表。这是 Phase 3 的实现细节难点，需在 str0m 侧做一次 spike 验证。

### 4.3 Overlap 期带宽缓解（Simulcast 降级）

Overlap 窗口内，每个 Sender 同时向 (N-1) 个 P2P peer + 1 个 SFU 发送。**策略**：协商时通知 Sender 向 SFU 仅发 `q=180p` 低层。Phase 4 切换完成后，SFU 再通过 `rid`/Simulcast 协商切换到高层。这与 README "Phase 2 仅发低分辨率层" 一致。

### 4.4 异常降级

- Overlap 超时（2000ms）未收到 `sfu_ready` 的客户端，标 `Switch_Failed`，单独发 `keep_p2p`，**不阻塞**其他客户端的 `execute`。
- 视频切换失败时，音频通道优先走 SFU（音频码率低、关键帧概念不存在，几乎不会失败）。
- 服务器侧记录 `switch_failed_total` 指标，用于运维监控。

---

## 5. 研发周期规划

> 周期单位：1 周 Cycle。总计 26 周（~6 个月）到 SDK 可用。每个 Cycle 末有 Demo + 验收门，过不去不进下一阶段。

### Phase 0 · 地基与决策门（W1–W2）

**目标**：把空壳填成"能跑信令协议的最小骨架"，并锁死所有技术决策。

| 任务 | 交付物 | 验收 |
| :--- | :--- | :--- |
| 锁定 str0m vs webrtc-rs（§2.1） | 决策记录写入本文件；Cargo 改完 | `cargo build` 通过 |
| 引入 `tracing`/`tracing-subscriber`，替换 `log` | 结构化日志 + request_id | 启动日志 JSON 输出 |
| `vauid-shared::conf` 实现 toml+env 加载 | `Config` 结构体、`config.toml` 样例 | 单测覆盖 env 覆盖 |
| `vauid-shared::proto` 落地 §3.2 协议 schema | `SignalMessage`/`ServerEvent` 枚举 | `serde` round-trip 单测 |
| `vauid-shared::ids` newtype | `RoomId`/`ClientId`/`TrackId` | 编译期防混用 |
| 扩展 `Error` 分类树 + `From` 转换 | `SignalError`/`RtcError`/`RoomError` | 编译通过 |
| `RoomStore` trait + 内存 `DashMap` 实现 | `store/mod.rs` | 单测：create/join/leave |
| CI：GitHub Actions跑 `fmt`+`clippy`+`test` | `.github/workflows/ci.yml` | PR 强制 CI |

**风险**：str0m 学习曲线。**缓解**：W2 末做一次 str0m "echo SFU" spike（1 publisher → 1 subscriber 转发），验证 API 心智模型。

### Phase 1 · 信令与 P2P Mesh MVP（W3–W6）

**目标**：≤4 人房间，全 P2P Mesh，服务器**零媒体带宽**，仅做 SDP/ICE 中继。

| Cycle | 任务 | 交付 |
| :--- | :--- | :--- |
| W3 | `signal/server.rs`：WS 升级、消息分发、心跳/ping-pong | 客户端能连、能收 `joined` |
| W4 | `room.rs` + `participant.rs`：房间生命周期、参与者状态机 | create/join/leave/peer_joined/peer_left |
| W5 | `sdp.rs` + `ice.rs`：P2P 介绍人模式，offer/answer/ice 中继 | 2 客户端 P2P 视频通 |
| W6 | 3–4 人 Mesh 联调 + 断线清理 + 房间 GC | 4 人 Mesh 稳定 10 分钟 |

**验收门**：4 个浏览器标签页互连，服务器带宽监控接近 0（仅信令），断一个标签页其余 3 人自动重连 Mesh。

**风险**：SDP 改写（mid 冲突、codec 限定）。**缓解**：MVP 阶段不改写 SDP，纯透传。

### Phase 2 · SFU 引擎核心（W7–W10）

**目标**：str0m 驱动的 SFU，客户端 publish/subscribe，服务器 N→N 转发。

| Cycle | 任务 | 交付 |
| :--- | :--- | :--- |
| W7 | `rtc/engine.rs`：`Rtc` 实例池、DTLS/ICE 配置、STUN | 单客户端连上 SFU |
| W8 | `publisher.rs` + `subscriber.rs`：Track 生命周期、SSRC 映射表 | 1 publisher → 1 subscriber 转发 |
| W9 | `forwarder.rs`：RTP 转发循环、NACK 重传、序列号改写 | 1→N 转发稳定 |
| W10 | `publish`/`subscribe` 信令闭环 + 多 track（音+视） | 4 人 SFU 会议通 |

**验收门**：4 路客户端经 SFU 互看，720p 稳定，CPU 单核 < 60%。

**风险**：str0m 事件循环驱动模型（需自己 poll `Rtc::poll_output`）。**缓解**：参考 str0m 官方 `relay` 示例。

### Phase 3 · 拓扑决策与 Make-Before-Break（W11–W14）⭐

**目标**：实现 README §"过继模式" 5 阶段 FSM，无感切换。

| Cycle | 任务 | 交付 |
| :--- | :--- | :--- |
| W11 | `topology/fsm.rs` + `coordinator.rs`：阈值检测、状态机、广播 | `topology_change` 事件能发 |
| W12 | `rtcp.rs`：PLI/FIR 注入 + `mid→ssrc` 表维护（spike 落地） | Overlap 期关键帧 < 200ms 到达 |
| W13 | Overlap 双轨转发 + Simulcast 低层降级 + 超时降级路径 | 切换成功率 > 90% |
| W14 | 前端联动：opacity 切换、`sfu_ready` 上报、`teardown` 清理 | 端到端无感切换 |

**验收门**：4→5 人切换，肉眼无黑屏、无卡顿、无音频断；失败客户端走 `keep_p2p` 不影响其他人。

**风险（项目最高风险）**：PLI 注入时序、str0m Simulcast API 细节、前端 DOM 复用与后端状态对齐。**缓解**：W11–W12 预留 buffer，必要时把 W14 前端联动拆到 Phase 6 SDK 一起做（后端先用 mock 客户端验证 FSM）。

### Phase 4 · 生产加固（W15–W18）

**目标**：从"能跑"到"能上线"。

| Cycle | 任务 |
| :--- | :--- |
| W15 | BWE/GCC：`remb`/`twcc` 反馈、下行码率自适应 |
| W16 | Simulcast 全量：三层 encoding、按订阅者带宽选层 |
| W17 | 韧性：连接断开处理、参与者超时踢出、房间空闲 GC、TURN fallback |
| W18 | 可观测性：`metrics`（房间数/track 数/切换成功率/PLI 次数）+ Prometheus exporter + `tracing` span |

**验收门**：弱网（丢包 5%）下 6 人会议持续 30 分钟无崩溃；切换成功率指标可视化。

### Phase 5 · QUIC 实验通道（W19–W22）

**目标**：README §2 路径 A——QUIC 替代 WS 信令 + WebTransport 替代 DataChannel。媒体仍走 SRTP。

| Cycle | 任务 |
| :--- | :--- |
| W19 | `quinn` endpoint 起建、ALPN、0-RTT 配置 |
| W20 | WebTransport 信令通道（与 axum WS 并行，协议层复用 `SignalMessage`） |
| W21 | DataChannel-over-QUIC：前端虚拟布局状态同步（坐标/层级/聚焦） |
| W22 | A/B 对比：WS vs QUIC 信令在弱网下的延迟、重连表现 |

**验收门**：弱网下 QUIC 信令重连时间 < WS 的 50%；布局协同状态同步无丢帧。

**风险**：浏览器 WebTransport 兼容性（Chrome/Edge OK，Safari/Firefox 滞后）。**缓解**：保留 WS 兜底，QUIC 为 enhancement。

### Phase 6 · SDK 与生态闭环（W23–W26）

**目标**：开箱即用的前端 SDK，与"虚拟布局组件库"深度绑定。

| Cycle | 任务 |
| :--- | :--- |
| W23 | TS SDK：连接管理、信令收发、`topology_change` 事件封装 |
| W24 | SDK：P2P/SFU PeerConnection 生命周期、`sfu_ready` 上报、opacity 切换 helper |
| W25 | SDK：虚拟布局引擎集成（DOM 复用、transform 计算、0 帧策略） |
| W26 | E2E 测试（Playwright + 真实多浏览器）+ Demo 站 + 文档站 |

**验收门**：前端开发者 `<VauidRoom room="x" />` 一行接入，4→6 人切换无感。

---

## 6. 里程碑总览

| 里程碑 | 周次 | 标志 |
| :--- | :--- | :--- |
| **M1 · P2P MVP** | W6 末 | 4 人 Mesh，零服务器媒体带宽 |
| **M2 · SFU Ready** | W10 末 | 4 人 SFU 会议，720p 稳定 |
| **M3 · 无感切换** ⭐ | W14 末 | Make-Before-Break 端到端可用 |
| **M4 · 生产可用** | W18 末 | 弱网 6 人 30 分钟稳定 + 指标可视 |
| **M5 · QUIC 增强** | W22 末 | WebTransport 信令 + DataChannel |
| **M6 · SDK 闭环** | W26 末 | 一行接入 + Demo 上线 |

---

## 7. 验收与测试策略

| 层级 | 工具 | 覆盖 |
| :--- | :--- | :--- |
| 单元测试 | `cargo test` | FSM 状态迁移、协议 serde、SDP 改写、SSRC 映射 |
| 集成测试 | `tokio` + mock 客户端 | 房间生命周期、SFU 转发、切换 FSM |
| 端到端 | Playwright + headless Chrome | 多浏览器真实 WebRTC、4→5 切换 |
| 压测 | 自研 load gen（str0m 做假客户端） | 单核转发路数目标：≥200 路 720p |
| 弱网 | `tc netem` / Clumsy | 丢包 5%/10%、延迟 200ms、抖动 |
| 混沌 | 随机杀客户端 | 断线清理、降级路径 |

**关键 SLO**（M3 后度量）：

- 切换成功率 ≥ 99%
- 切换中断时间 < 300ms（P95）
- 首帧渲染 < 200ms（PLI 后）
- 弱网（丢包 5%）下音频 MOS ≥ 3.5

---

## 8. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 | 触发周期 |
| :--- | :--- | :--- | :--- | :--- |
| str0m PLI/FIR 注入时序不达预期 | 中 | 高 | Phase 0 spike + Phase 3 W12 buffer；必要时回退 webrtc-rs 重评 | W2 / W12 |
| Make-Before-Break 前后端状态对齐 bug | 中 | 高 | 协议字段单一 `phase`；mock 客户端先验后端 FSM | W14 |
| str0m 事件循环驱动模型踩坑 | 中 | 中 | 参考官方 relay 示例，W7 预留学习时间 | W7 |
| 浏览器 WebTransport 兼容性 | 高 | 中 | WS 兜底，QUIC 为 enhancement，不阻塞主流程 | W19 |
| Simulcast 三层协商复杂度 | 中 | 中 | Phase 2 先单层，Phase 3 仅 Overlap 期用低层，Phase 4 全量 | W9 / W13 |
| 团队 Rust/WebRTC 经验 | — | — | Phase 0 spike + 官方示例优先；不造轮子 | 全程 |
| 阈值 4 vs 5 与前端预期不一致 | 低 | 低 | Phase 0 与前端确认，写入决策记录 | W1 |

---

## 9. 可观测性指标清单（M4 起强制）

| 指标 | 类型 | 用途 |
| :--- | :--- | :--- |
| `rooms_active` | gauge | 当前房间数 |
| `participants_active` | gauge | 当前在线人数 |
| `tracks_published` | gauge | 上行 track 数 |
| `switch_total{result=ok\|failed}` | counter | 拓扑切换成功率 |
| `switch_duration_ms` | histogram | 切换中断时长 P95 |
| `pli_sent_total` | counter | PLI 注入频率（异常高=解码频繁失败） |
| `rtp_packets_forwarded` | counter | 转发吞吐 |
| `nack_sent_total` | counter | 重传压力 |
| `ws_connections` / `quic_connections` | gauge | 信令通道分布 |

---

## 10. 与 README / TODO 的对齐说明

- **README §8 Roadmap** 的 4 个 Phase 保留为高层语义，本文档将其展开为 6 个 Phase + 26 周的可执行计划，并补齐了 Make-Break-Break（README "过继模式"章节）的落地方案。
- **TODO.md** 的信令/WebRTC/SFU/基础维护清单映射到 Phase 0–2；进阶项（BWE/Simulcast/DataChannel）映射到 Phase 4–5；录制/混音未纳入本轮 26 周计划，留待 v0.2。
- **README 选型冲突**（str0m vs webrtc-rs）在 §2.1 给出决策，需在 Phase 0 W1 确认签字。

---

## 11. 立即行动项（Phase 0 W1）

- [ ] 与项目负责人确认 §2.1 str0m 决策（本项目最高优先级决策门）
- [ ] 与前端确认切换阈值（count == 5 触发）
- [ ] 改 [vauid/Cargo.toml](./vauid/Cargo.toml)：移除 `webrtc`、`tokio-tungstenite`，引入 `str0m`、`tracing`、`dashmap`、`uuid`
- [ ] 落地 `vauid-shared::proto` 协议 schema（§3.2）
- [ ] 搭 CI（`fmt` + `clippy -D warnings` + `test`）
- [ ] str0m "echo SFU" spike 立项（W2 末交付）
