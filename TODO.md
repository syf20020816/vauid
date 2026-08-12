# `vauid` 任务清单与工期预估

> 版本：v0.2 · 2026-08-11
> 与 [roadMap.md](./roadMap.md)（v0.2 融合版）一一对应。工期单位：**人日**（按 1 人满负荷估算；2 人并行时 Phase 级可压缩）。
> 汇总：**7 个 Phase · 30 周 · 116 人日**。✅ = 已完成；空 = 待办。

---

## Phase 0 · 地基与决策门（W1–W2 · 8 人日）

- [ ] 确认 D1–D6 决策并写入 roadmap.md（0.5 人日）
- [ ] `tracing` 结构化日志 + request_id（1 人日）
- [ ] `vauid-shared::error` 分类错误树：Signal/Quic/Room/Media/Config + From 转换（1.5 人日）
- [ ] 协议 schema 补齐 `MsgType`/`Message` 骨架：`vauid-shared/src/proto/message.rs`（2 人日）
- [ ] **DATAGRAM 扩展 spike**：RFC 9221 帧收发最小验证（2 人日）⭐ 最高优先级
- [ ] 弱网测试基线：`tc netem` 5%/10% 丢包下 echo 延迟/吞吐基线（1 人日）

**验收**：`cargo build + clippy -D warnings + test` 全绿；DATAGRAM spike 出结论。

---

## Phase 1 · QUIC 传输底座补全（W3–W6 · 16 人日）

- [ ] 多连接管理：`ConnectionId → Connection` 映射、生命周期事件上抛，重构 `service/p2p/mod.rs`（5 人日）
- [ ] `QuicConf` 增加 0-RTT / keep-alive / 空闲超时配置项（`vauid-shared/src/conf/mod.rs`）（2 人日）
- [ ] 连接迁移验证：UDP 换源地址模拟 WiFi→蜂窝（3 人日）
- [ ] DATAGRAM 扩展落地（D2a）：tquic 扩展模块 + 特性门控（5 人日）
- [ ] 连接级指标：`quic_connections` / 断连原因（1 人日）

**验收**：3 客户端并发稳定；0-RTT 生效；Datagram 可用。

---

## Phase 2 · 消息协议与双通道信令（W7–W10 · 16 人日）

- [ ] `Message` 帧协议落地 + 与 `SignalMessage` 打通（3 人日）
- [ ] 流模型与调度：信令/文本/媒体流 + 优先级（3 人日）
- [ ] 发送策略：队列 / 背压 / 超时丢弃（2 人日）
- [ ] QUIC 信令与 WS 信令并行：`signal_demo` 增加 QUIC 变体（4 人日）
- [ ] `quic_chat` 升级多连接房间聊天（2 人日）
- [ ] `web/p2p-test.html` 支持"QUIC 模式"开关（2 人日）

**验收**：文本通道 QUIC 化；弱网 5% 下 QUIC 信令延迟 < WS 60%。

---

## Phase 3 · 浏览器通路 WebTransport（W11–W13 · 12 人日）

- [ ] WebTransport 服务端（D5a tquic 自研 / D5b quinn+wtransport 兜底）：`/webtransport` 端点（5 人日）
- [ ] 消息协议在 WebTransport 双向流/单向上适配（3 人日）
- [ ] 前端 JS SDK 雏形：`connect()/send()/on()`（2 人日）
- [ ] 兼容矩阵 + 不支持时自动降级 WS（2 人日）

**验收**：Chrome 双标签页经 WebTransport 完成信令握手并收发文本（不经 WS）。

---

## Phase 4 · QUIC 音视频媒体承载 ⭐（W14–W19 · 24 人日）

- [ ] 音频链路：Opus → RTP → Datagram → Jitter Buffer → 解码播放（5 人日）
- [ ] 视频链路：编码器接入（D7 决策）→ RTP → Datagram/Stream → JB → 解码（7 人日）
- [ ] 丢包恢复：NACK（可靠流回传）+ FEC + PLI 关键帧请求（5 人日）
- [ ] 码率/拥塞控制：BBR + 反馈自适应（4 人日）
- [ ] 双自研客户端 QUIC 对讲联调（无 WS/SRTP）（3 人日）

**验收（主里程碑 M4）**：双自研客户端经 QUIC 实时音视频对讲 10 分钟稳定；弱网 5% 稳定。达标后 str0m/SRTP 降级为基线。

---

## Phase 5 · P2P 与 SFU 混合拓扑（W20–W24 · 20 人日）

- [ ] QUIC P2P：NAT 打洞、ICE over QUIC、服务器仅介绍人（6 人日）
- [ ] QUIC SFU：Datagram 转发 + 按订阅者码率降层（6 人日）
- [ ] `publish/subscribe/track_published` 信令闭环（3 人日）
- [ ] 混合拓扑切换：P2P↔SFU Make-Before-Break FSM（5 人日）

**验收**：4 人小队 P2P↔SFU 自动切换无感（中断 < 300ms）；弱网 5% 30 分钟稳定。

---

## Phase 6 · 生产化与生态（W25–W30 · 20 人日）

- [ ] 拥塞控制调优：BBR/反馈参数弱网收敛（3 人日）
- [ ] 安全：0-RTT 重放防护、证书轮换、ABR 访问控制（2 人日）
- [ ] 可观测：`media_latency`/`loss_recovery` 等指标 + tracing span（3 人日）
- [ ] **WS 信令下线**：QUIC/WebTransport 全量接管（2 人日）
- [ ] 前端 SDK：`<VauidRoom>` 一行接入（5 人日）
- [ ] A/B 报告：QUIC vs WS/WebRTC（延迟/重连/弱网）（3 人日）
- [ ] WebRTC 基线并行验证：str0m P2P/SFU/切换对照（2 人日）

**验收（主里程碑 M6）**：SDK 一行接入；QUIC 三项全面优于基线；WS 下线。

---

## 工期汇总

| Phase | 周次 | 人日 | 主里程碑 |
| :--- | :--- | :---: | :--- |
| Phase 0 · 地基与决策门 | W1–W2 | 8 | M0 |
| Phase 1 · QUIC 传输底座 | W3–W6 | 16 | M1 |
| Phase 2 · 消息协议与双通道 | W7–W10 | 16 | M2 |
| Phase 3 · 浏览器通路 | W11–W13 | 12 | M3 |
| Phase 4 · QUIC 音视频 ⭐ | W14–W19 | 24 | **M4** |
| Phase 5 · P2P 与 SFU 拓扑 | W20–W24 | 20 | M5 |
| Phase 6 · 生产化与生态 | W25–W30 | 20 | **M6** |
| **合计** | **30 周** | **116 人日** | — |

> 说明：Phase 4–6 含 WebRTC（str0m）基线并行验证，可拆 1 名成员专职基线线，主线（QUIC）不阻塞。
