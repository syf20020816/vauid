# `vauid` 技术实现与研发周期规划（v0.2 · 融合版）

> 版本：v0.2 · 2026-08-11
> 关联文档：[README.md](./README.md)（项目愿景与架构）、[TODO.md](./TODO.md)（任务清单与工期）、[quic.roadmap.md](./quic.roadmap.md)（QUIC 专项，已归档吸收）
> 本文件**融合**旧版 [roadMap.md](./roadMap.md)（总体规划）与 [quic.roadmap.md](./quic.roadmap.md)（QUIC 专项），确立最终技术路线；两旧文档保留备查，**本文件为唯一权威规划**。

---

## 0. 路线总览（最终确认）

**主线变更声明**：项目愿景由"WebRTC 服务器 + QUIC 实验增强"升级为——

> **演进主线：QUIC 全面替代 WebSocket 与 WebRTC/SRTP 传输** —— 信令与音视频媒体均由 QUIC 承载（浏览器经 WebTransport over HTTP/3，自研客户端经裸 QUIC），最终形态为"一处实现、两端互通"的纯 QUIC 实时通信栈。
>
> **并行基线：WebRTC（str0m）媒体面** —— 保留 P2P/SFU/拓扑切换能力作为 QUIC 未就绪阶段的兜底与 A/B 对照；QUIC 媒体面达成验收（Phase 4 末）后自动降级为保守基线。

两线**共用**的资产：信令协议 schema（`SignalMessage`/`ServerEvent`）、房间管理、拓扑切换 FSM（Make-Before-Break）、前端 SDK、可观测性指标。

本文档回答四个问题：

| 问题 | 结论（详见于章节） |
| :--- | :--- |
| 1. 现状 | QUIC 传输底座已可用（配置/封装/socket/echo/客户端），WS 信令与浏览器 WebRTC 测试已通过；媒体面与协议层为空白（§1） |
| 2. 决策 | QUIC 为主线、WebRTC 为基线；浏览器走 WebTransport、自研走裸 QUIC；媒体不可靠传输需扩展 tquic DATAGRAM（§3） |
| 3. 契约 | 统一 `Message` 帧协议 + 信令 schema + 拓扑 FSM phase（§4） |
| 4. 节奏 | 7 个 Phase · 30 周 · 116 人日，M0–M6 里程碑（§5–§6） |

---

## 1. 现状盘点（Codebase Audit）

| 维度 | 现状 | 差距 |
| :--- | :--- | :--- |
| 工作区 | `vauid` + `vauid-shared` 两 crate，`resolver="3"`，`serde` 已开 `derive` | OK |
| 配置层 | [conf/mod.rs](./vauid-shared/src/conf/mod.rs)：`QuicConf`/`TlsConf`/`CcAlgorithm`，TOML 自动生成，单测 2 个 ✅ | 缺 0-RTT/keep-alive/空闲超时项 |
| QUIC 封装 | [wrap/conf.rs](./vauid/src/core/quic/wrap/conf.rs)：`QuicConfig` → tquic `Config`（server/client），单测 4 个 ✅ | OK |
| QUIC 收发 | [socket.rs](./vauid/src/core/quic/socket.rs)：`QuicSocket` + `PacketSendHandler`（含 tokio 注册预热修复）✅ | OK |
| QUIC 服务器 | [p2p/mod.rs](./vauid/src/service/p2p/mod.rs)：`P2PServer` 回显模式 ✅ | 无多连接管理、无房间转发 |
| QUIC 客户端 | [p2p/client.rs](./vauid/src/service/p2p/client.rs)：`QuicClient` + 事件循环 ✅ | 单连接、一次一发 |
| 交互工具 | [bin/quic_chat.rs](./vauid/src/bin/quic_chat.rs) 终端一问一答 ✅；[bin/signal_demo.rs](./vauid/src/bin/signal_demo.rs) WS 信令 ✅ | 均为单连接演示 |
| 信令面 | [signal/server.rs](./vauid/src/core/signal/server.rs) + [room.rs](./vauid/src/core/signal/room.rs)：join/offer/answer/ice 中继、房间管理，测试 5 个 ✅ | 仅 WS 通道 |
| 协议 schema | [proto/mod.rs](./vauid-shared/src/proto/mod.rs)：`SignalMessage`/`ServerEvent` ✅ | 无媒体/控制帧类型 |
| 浏览器测试 | [web/p2p-test.html](./vauid/web/p2p-test.html) 双标签页 WebRTC P2P 实测通过 ✅ | 仅 WebRTC，无 QUIC/WebTransport |
| 媒体面 | `str0m 0.22` 在依赖 | 未接入业务 |
| 测试 | 12 个测试全绿、clippy 0 警告 ✅ | 无弱网/压测/多客户端 |

**结论**：QUIC 传输底座（Phase 0–1 主体）**已提前完成约 30%**。本规划从"协议层 + 媒体面 + 拓扑"继续，无需重做传输层。

---

## 2. 目标架构

```
                        ┌────────────────────────────────────────────┐
                        │           vauid QUIC 服务端（tquic）          │
   浏览器 WebTransport ──┼─▶┌──────────────────────────────────────┐ │
   (HTTP/3 + wq 帧)      │  │ WebTransport 会话层（RFC 9220）        │ │
                        │  └──────────────────────────────────────┘ │
                        │  ┌──────────────────────────────────────┐ │
   自研客户端裸 QUIC ────┼─▶│ 传输层适配（裸 QUIC + DATAGRAM 扩展）  │ │
                        │  └──────────────────────────────────────┘ │
                        │  ┌──────────────────────────────────────┐ │
                        │  │ 应用协议层：Message 帧协议             │ │
                        │  │（信令/文本/媒体/控制，type+seq+flags） │ │
                        │  └──────────────────────────────────────┘ │
                        │  ┌────────────┬────────────┬───────────┐ │
                        │  │ 信令房间     │ 媒体引擎     │ 拓扑决策   │ │
                        │  │（复用现有）  │编码/RTP/JB │P2P↔SFU FSM│ │
                        │  └────────────┴────────────┴───────────┘ │
                        └────────────────────────────────────────────┘
  ─────────────────────────────────────────────────────────────────────
  基线通道（QUIC 未就绪期兜底）：浏览器 ↔ axum WS 信令 + str0m/SRTP 媒体
```

分层原则：
1. **传输层**（tquic）只做字节与帧；**应用协议层**统一 `Message`（信令/媒体/控制同通道、按 type 区分），同时服务 WebTransport 与裸 QUIC；
2. **媒体层**复用标准 RTP 语义（载荷/序列号/RTCP 反馈），传输载体是 QUIC（Datagram/Stream）而非 SRTP/DTLS——不重复造编码器与 Jitter Buffer 轮子；
3. **拓扑层**保留 Make-Before-Break FSM（P2P↔SFU 无感切换），与传输载体解耦；
4. 关键约束（已核实）：浏览器无裸 QUIC API（仅 WebTransport）；tquic 1.6 无 DATAGRAM 帧 API、无 WebTransport 会话层——见 §3 决策。

---

## 3. 关键技术决策门（Decision Gates）

### D1 终端形态优先级：自研客户端先行，浏览器并行（已定）

- **路径 B（自研客户端先行）**：tquic 裸 QUIC 先行，最快验证"QUIC 承载音视频"核心假设，无需浏览器依赖；
- **路径 A（浏览器并行）**：WebTransport 通路与媒体面同步推进，复用 [web/p2p-test.html](./vauid/web/p2p-test.html) 联调。
- **决策**：**B 先行、A 并行**（Phase 3 起）。

### D2 媒体不可靠传输载体：DATAGRAM 扩展为主，纯 STREAM 兜底（已定）

| 方案 | 说明 | 结论 |
| :--- | :--- | :--- |
| D2a 扩展 tquic 实现 RFC 9221 DATAGRAM | 帧层加 `DATAGRAM` 收发 + 尽力而为调度，特性门控 | **主选**：先 spike（2 人日），落地后音频/视频尽力而为帧走 Datagram |
| D2b 纯 STREAM 承载媒体 | 每帧一条短流 + `fin` 边界，发送端过期丢弃 | **兜底**：D2a 受阻时切换，代价是弱网重传浪费带宽 |
| D2c 换栈 quinn | Datagram 开箱即用 | 弃：与既有 tquic 封装割裂、双栈维护成本高 |

### D3 媒体封装：RTP over QUIC（已定）

保留 RTP 载荷/序列号/RTCP（NACK/PLI/REMB）语义，传输层换 QUIC；参考 IETF `rtp-over-quic` 草案。好处：复用编码器/Jitter Buffer 生态，未来可与传统 WebRTC 互通。**不采用**自定义帧协议（解码器生态要自己搭）。

### D4 信令迁移节奏（已定）

**双通道并行**（QUIC 信令 + WS 信令，协议层复用 `SignalMessage`）→ **灰度切流**（新客户端默认 QUIC）→ **下线 WS**（Phase 6）。验收：弱网下 QUIC 信令重连时间 < WS 50%。

### D5 WebTransport 服务端：tquic 上自研为主，quinn+wtransport 兜底（已定）

基于 tquic h3 模块实现 RFC 9220（HTTP/3 CONNECT + `wq` 帧 0x41）；若 2 周内无法打通浏览器握手，切 quinn + `wtransport`，避免阻塞浏览器通路。

### D6 WebRTC 基线栈：str0m（沿用）

作为 QUIC 未就绪期的媒体兜底（P2P/SFU/切换语义沿用旧版 roadMap v0.1 §4 设计）；str0m 的 PLI 注入、Simulcast 能力保留，作为基线验证与 A/B 对照。**注意**：str0m 的 DTLS/SRTP/ICE 传输栈不参与 QUIC 主线。

### D7 视频编码器（Phase 4 内决策）

| 编码器 | 场景 | 备注 |
| :--- | :--- | :--- |
| 浏览器 WebCodecs/WebRTC 原生 | 浏览器→服务端 | 采集编码在浏览器完成 |
| ffmpeg/gstreamer 绑定 | 服务端转码/解码 | 成熟但体积大 |
| OpenH264 / x264 bindings | 自研客户端编码 | 注意 licensing |
| 纯 Rust 解码（dav1d 等） | 自研客户端解码 | 编码侧成熟度参差，需 spike |

---

## 4. 协议契约

### 4.1 统一 `Message` 帧协议（Phase 2 落地）

```rust
// vauid-shared/src/proto/message.rs（新增）
pub enum MsgType { Signal, Chat, Media, Control }   // 信令/文本/媒体/控制
pub struct Message {
    pub ty: MsgType,
    pub seq: u64,          // 递增序列号（可靠性/去重）
    pub flags: u8,         // fin / 关键帧 / 冗余编码等标志
    pub payload: Vec<u8>,  // 信令为 JSON（复用 SignalMessage），媒体为 RTP 载荷
}
```

传输映射：信令/文本 → 可靠流（高优先级）；音频/视频非参考帧 → Datagram（尽力而为）；视频关键帧/RTCP 反馈 → 可靠流或最高优先级 Datagram。

### 4.2 信令 schema（沿用）

`SignalMessage`/`ServerEvent` 维持 [proto/mod.rs](./vauid-shared/src/proto/mod.rs) 现状（join/offer/answer/ice/joined/peer_joined/peer_left）；Phase 5 增加 `publish/subscribe/track_published` 与 `topology_change{phase}`。

### 4.3 拓扑切换 FSM（沿用旧版 v0.1 设计）

`TopologyState { PureP2P, PreparingSwitch, OverlapDualCast, PureSFU }` + `SwitchPhase { Prepare, Overlap, Execute, Teardown, KeepP2P }`；Overlap 期 PLI 强制关键帧、Simulcast 低层降级、2000ms 超时降级路径——语义不变（沿用旧版 roadMap v0.1 §4 设计），仅传输载体在 QUIC 主线下替换。

---

## 5. 分阶段研发规划

> 周期单位：周（W）；工作量：人日（按 1 人满负荷，2 人并行时可压缩）。总计 **30 周 / 116 人日**。每 Phase 末有验收门。

### Phase 0 · 地基与决策门（W1–W2 · 8 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| 确认 D1–D6 决策并写入本文件 | 决策记录 | 0.5 | 签字 √ |
| `tracing` 结构化日志 + request_id | 启动 JSON 日志 | 1 | 日志可解析 √ |
| `vauid-shared::error` 分类错误树（Signal/Quic/Room/Media/Config） | 错误模块 | 1.5 | From 转换单测 |
| 协议 schema 补齐 `MsgType`/`Message` 骨架 | proto/message.rs | 2 | serde round-trip 单测 |
| **DATAGRAM 扩展 spike**（RFC 9221 帧收发最小验证） | spike 报告 | 2 | Datagram 往返实测可行 |
| 弱网测试基线（`tc netem` 5%/10% 丢包） | 基线报告 | 1 | echo 延迟/吞吐基线数值 |

**验收门**：`cargo build + clippy -D warnings + test` 全绿；DATAGRAM spike 给出"可行/不可行"结论。

### Phase 1 · QUIC 传输底座补全（W3–W6 · 16 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| 多连接管理：`ConnectionId → Connection` 映射、生命周期事件上抛 | [p2p/mod.rs](./vauid/src/service/p2p/mod.rs) 重构 | 5 | 3 并发客户端独立 echo、无串流 |
| `QuicConf` 增加 0-RTT/keep-alive/空闲超时配置项 | [conf/mod.rs](./vauid-shared/src/conf/mod.rs) | 2 | 0-RTT 二次连接 < 20ms |
| 连接迁移验证（UDP 换源地址模拟 WiFi→蜂窝） | 集成测试 | 3 | 迁移不中断、媒体不丢超 1 帧 |
| DATAGRAM 扩展落地（D2a） | tquic 扩展模块（特性门控） | 5 | 丢包不重传、乱序投递，吞吐验证 |
| 连接级指标（`quic_connections`/断连原因） | 指标模块 | 1 | 断连可归因 |

**验收门**：3 客户端并发稳定；0-RTT 生效；Datagram 传输可用（音频/视频帧载体就绪）。

### Phase 2 · 消息协议与双通道信令（W7–W10 · 16 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| `Message` 帧协议落地 + 与 `SignalMessage` 打通 | proto/message.rs 实现 | 3 | 信令/文本同通道，round-trip 单测 |
| 流模型与调度：信令流/文本流/媒体流 + 优先级 | 流调度模块 | 3 | 优先级生效（音频先于文本，集成测试验证次序） |
| 发送策略：队列/背压/超时丢弃 | 发送调度器 | 2 | 积压时旧消息被丢弃、新消息延迟不涨 |
| QUIC 信令与 WS 信令并行（`signal_demo` 增加 QUIC 变体） | 双通道信令 | 4 | 同一房间 QUIC 与 WS 客户端互通 |
| `quic_chat` 升级多连接房间聊天 | [bin/quic_chat.rs](./vauid/src/bin/quic_chat.rs) | 2 | 双终端聊天互见 |
| 前端 [p2p-test.html](./vauid/web/p2p-test.html) 支持"QUIC 模式"开关 | 前端连接层 | 2 | 浏览器经 WS 与 QUIC 客户端互通 |

**验收门**：文本通道完成 QUIC 化；弱网 5% 下 QUIC 信令延迟 < WS 60%。

### Phase 3 · 浏览器通路（WebTransport）（W11–W13 · 12 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| WebTransport 服务端（D5a/D5b） | `/webtransport` 端点 | 5 | 浏览器 `new WebTransport(url)` 握手成功 |
| 消息协议在 WebTransport 双向流/单向上适配 | 传输适配层 | 3 | 信令/文本经 WebTransport 收发，与裸 QUIC 客户端互通 |
| 前端 JS SDK 雏形（`connect()/send()/on()`） | web/ JS 模块 | 2 | p2p-test.html "QUIC 模式"可用 |
| 兼容矩阵 + WS 兜底降级 | 特性探测 | 2 | 不支持 WebTransport 的浏览器自动降级 WS |

**验收门**：Chrome 双标签页经 WebTransport 完成信令握手并收发文本（不经过 WS）。

### Phase 4 · QUIC 音视频媒体承载 ⭐（W14–W19 · 24 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| 音频链路：Opus → RTP → Datagram → Jitter Buffer → 解码播放 | 媒体管线（音频） | 5 | 端到端延迟 < 150ms；丢包 5% 音频 MOS ≥ 3.5 |
| 视频链路：编码器接入（D7）→ RTP → Datagram/Stream → JB → 解码 | 媒体管线（视频） | 7 | 720p 30fps，延迟 < 300ms |
| 丢包恢复：NACK（可靠流回传）+ FEC + PLI 关键帧请求 | 恢复模块 | 5 | 丢包 5% 画面无撕裂，恢复 < 200ms |
| 码率/拥塞控制：发送端码率自适应（BBR + 反馈） | 码率控制 | 4 | 弱网自动降码率、不崩溃 |
| 双自研客户端 QUIC 对讲联调（无 WS/SRTP） | 端到端 demo | 3 | 实时对讲 10 分钟稳定 |

**验收门（主里程碑 M4）**：两个自研客户端经 QUIC 完成音视频实时对讲，无 WS/无 SRTP；弱网 5% 稳定。**达标后：str0m/SRTP 基线降级为兜底。**

### Phase 5 · P2P 与 SFU 混合拓扑（W20–W24 · 20 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| QUIC P2P：客户端直连（NAT 打洞、ICE over QUIC，服务器仅介绍人） | P2P 连接模块 | 6 | 2 客户端打洞直连，服务器零媒体带宽 |
| QUIC SFU：服务器 Datagram 转发 + 按订阅者码率降层 | 转发器 | 6 | 4 人会议经服务器转发稳定 720p |
| `publish/subscribe/track_published` 信令闭环 | 协议扩展 | 3 | publish→subscribe 通路可用 |
| 混合拓扑切换：P2P↔SFU Make-Before-Break（overlap 双发→execute→teardown） | 拓扑 FSM | 5 | 切换无感、中断 < 300ms |

**验收门**：4 人小队在 P2P 与 SFU 间自动切换无感；弱网 5% 30 分钟稳定。

### Phase 6 · 生产化与生态（W25–W30 · 20 人日）

| 任务 | 交付物 | 工期 | 验收 |
| :--- | :--- | :---: | :--- |
| 拥塞控制调优（BBR/反馈参数在弱网收敛） | 调优报告 | 3 | 5% 丢包持续稳定 |
| 安全：0-RTT 重放防护、证书轮换、ABR 访问控制 | 安全清单 | 2 | 安全审计通过 |
| 可观测：`media_latency`/`loss_recovery` 等指标 + tracing span | 指标面板 | 3 | 指标可视化 |
| **WS 信令下线**（QUIC/WebTransport 全量接管） | 迁移报告 | 2 | 无 WS 进程，7 天灰度无事故 |
| 前端 SDK：`<VauidRoom>` 一行接入 | TS SDK | 5 | 一行接入可用 |
| A/B 报告：QUIC vs WS/WebRTC（延迟/重连/弱网） | 对比报告 | 3 | 三项全面优于基线 |
| WebRTC 基线并行验证（str0m P2P/SFU/切换） | 基线对照测试 | 2 | 与 QUIC 线互证 |

**验收门（主里程碑 M6）**：SDK 一行接入；QUIC 在延迟/重连/弱网全面优于 WS/WebRTC；WS 下线。

---

## 6. 里程碑总览

| 里程碑 | 周次 | 标志 | 关联验收 |
| :--- | :--- | :--- | :--- |
| M0 · 决策与 DATAGRAM 可行 | W2 | D1–D6 签字 + Datagram spike 通过 | Phase 0 |
| M1 · QUIC 传输底座 | W6 | 多连接 + 0-RTT + 连接迁移 + Datagram 可用 | Phase 1 |
| M2 · 消息协议与双通道 | W10 | 信令/文本全走 QUIC，与 WS 互通 | Phase 2 |
| M3 · 浏览器通路 | W13 | WebTransport 双标签页互通 | Phase 3 |
| **M4 · QUIC 音视频** ⭐ | W19 | 自研客户端实时音视频，无 WS/SRTP | Phase 4 |
| M5 · 混合拓扑 | W24 | 4 人 P2P↔SFU 无感切换 | Phase 5 |
| **M6 · 生产可用** | W30 | WS 下线 + SDK 一行接入 + A/B 全面胜出 | Phase 6 |

---

## 7. 测试与验收策略

| 层级 | 工具 | 覆盖 |
| :--- | :--- | :--- |
| 单元 | `cargo test` | Message serde、FSM 迁移、错误树、流调度 |
| 集成 | tokio + mock QUIC 客户端 | 多连接、0-RTT、连接迁移、双通道互通 |
| 端到端 | 双自研客户端 + Playwright | QUIC 音视频对讲、WebTransport 双标签页 |
| 弱网 | `tc netem` / Clumsy | 丢包 5%/10%、延迟 200ms、抖动 |
| 压测 | 自研 load gen（tquic 假客户端） | 单核转发 ≥ 100 路 720p |
| A/B | 同场景 WS/WebRTC 对照 | QUIC 收益量化 |

**关键 SLO**（M4 起度量）：QUIC 音视频端到端延迟 < 300ms（P95）；弱网 5% 丢包下音频 MOS ≥ 3.5；拓扑切换中断 < 300ms；QUIC 信令重连 < WS 50%。

---

## 8. 风险矩阵

| 风险 | 概率 | 影响 | 缓解 | 触发 |
| :--- | :--- | :--- | :--- | :--- |
| tquic DATAGRAM 扩展受阻 | 中 | 高 | Phase 0 spike 先行；回退 D2b（纯 STREAM 策略） | W2 |
| tquic 上 WebTransport 无法打通 | 中 | 高 | 切 D5b（quinn + wtransport） | W13 |
| 视频编码器生态不满足实时性 | 高 | 中 | 浏览器 WebCodecs；服务端 ffmpeg 绑定；D7 spike | W14 |
| 浏览器 WebTransport 兼容性 | 高 | 中 | WS 兜底保留至 Phase 6；特性探测降级 | W11 |
| 媒体延迟预算不达标 | 中 | 高 | 逐环节压测（编码/JB/网络/解码） | W16 |
| 双栈（tquic+quinn）并存维护 | 中 | 中 | D2/D5 尽量收敛单栈 | W13 |
| 0-RTT 重放与迁移安全 | 低 | 中 | 服务端令牌 + 重放缓存 | W4 |

---

## 9. 可观测性指标清单（M4 起强制）

| 指标 | 类型 | 用途 |
| :--- | :--- | :--- |
| `quic_connections` / `wt_connections` | gauge | 通路分布（QUIC vs WebTransport vs WS） |
| `media_latency_ms` | histogram | 端到端媒体延迟 P95 |
| `loss_recovery_total` | counter | NACK/FEC 恢复压力 |
| `switch_total{result}` / `switch_duration_ms` | counter/histogram | 拓扑切换成功率与中断时长 |
| `rooms_active` / `tracks_published` | gauge | 房间与上行 track 规模 |
| `datagram_dropped_total` | counter | 尽力而为帧丢弃（发送端过期策略） |

---

## 10. 与旧文档的对齐

| 旧文档 | 本文件的吸收方式 |
| :--- | :--- |
| [roadMap.md](./roadMap.md)（v0.1） | 拓扑切换 FSM、P2P/SFU 语义、生产化/SDK 规划、测试与指标 → 吸收至 §4.3/§5（Phase 4–6） |
| [quic.roadmap.md](./quic.roadmap.md) | QUIC 主线、决策门 D1–D5、双通路架构、媒体承载 → 吸收至 §2/§3/§5（Phase 1–4） |
| [TODO.md](./TODO.md) | 任务分解与工期（各节点人日预估）→ 独立文件，与本文件 Phase 一一对应 |
| [README.md](./README.md) | 愿景与架构 → 同步更新选型与路线（QUIC 主线） |

---

## 11. 立即行动项（Phase 0 W1）

- [ ] 签字确认 §3 D1–D6 决策
- [ ] tquic DATAGRAM 扩展 spike 立项（2 人日，W2 末交付结论）
- [ ] 落地 `vauid-shared::proto::message`（`MsgType`/`Message` 骨架）
- [ ] 建立弱网测试基线（`tc netem`）
- [ ] 多连接管理重构立项（Phase 1 首任务）
