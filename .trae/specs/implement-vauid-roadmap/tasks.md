# Tasks

> 来源：roadMap.md v0.2 §5，按 2026-09-04 代码进度与用户指定的「边写边测试」增量方法修正。
> **核心方法**：P2P Server 从最简单的互相发送开始一步步扩展，每个切片先写测试（红）→ 实现（绿）→ 才算完成；基础设施（协议帧/错误类别/会话注册）按需引入，不预先备齐。
> 粒度策略：Phase 0/1 细化到文件与接口级；Phase 2–6 保持路线图粒度，进入该阶段时基于上游产出再细化。

## Phase 0 · 增量起步：互发回路边写边测（W1–W2）

### 主线：Server 增量切片（顺序执行，每步先测后写）

- [x] Task 0.1: Slice 1 双向互发——服务端主动下发（`src/service/p2p/mod.rs`）
  - [x] 先写测试（红）：客户端连接后应收到服务端主动发送的消息（如 welcome）；客户端发送消息后服务端可通过日志/回执观测到接收（现有回显保留）
  - [x] 实现：`P2PHandler::on_conn_established` 中向新连接 `stream_write` 一条欢迎消息；确认服务端→客户端方向通路
  - 验证：集成测试通过（`vauid/tests/`，沿用 `p2p_client.rs` 基建）✅ `p2p_server_welcome` 先红（E0609 无 server_msgs 字段）后绿

- [x] Task 0.2: Slice 2 带类型的消息——引入 `Message` 帧（`vauid-shared/src/proto/message.rs`，方案见 spec T2）
  - [x] 先写测试（红）：`MsgType`/`Message` round-trip 单测（seq 边界、flags 组合、空 payload）
  - [x] 实现：`MsgType { Signal, Chat, Media, Control }`（本切片只用 Signal/Chat）+ `Message { ty, seq, flags, payload }`；serde derive；`proto/mod.rs` 导出
  - [x] 接入互发回路：welcome 与客户端消息均以 `Message`（Chat 类型）承载，替代裸字节
  - 验证：round-trip 单测 + 双向互发集成测试均通过 ✅ 2 个 round-trip 单测 + echo/welcome 测试改帧断言

- [x] Task 0.3: Slice 3 双客户端转发——最小会话注册雏形（`src/service/p2p/mod.rs`，方案见 spec T3）
  - [x] 先写测试（红）：客户端 A 发消息 → 客户端 B 收到；B 发 → A 收到；互不串流
  - [x] 实现：`P2PHandler` 持有 `Rc<RefCell<HashMap<ConnKey, SessionState>>>`；`on_conn_established` 注册、`on_conn_closed` 移除；转发 = 查表定向 `stream_write`（ConnKey 选型实现时确认）
  - [x] 回显逻辑退化为转发逻辑的一部分（单客户端时退化为回显）
  - 验证：双客户端双向转发集成测试通过 ✅ ConnKey 选型为 `conn.index()`（endpoint slab 索引）；`p2p_forward_a_to_b`/`p2p_forward_b_to_a` 双向 + 无串流断言；`run_until` 谓词完成条件

- [x] Task 0.4: Slice 4 错误树生长（`vauid-shared/src/error.rs`，方案见 spec T1）
  - [x] 先写测试（红）：目标 client 不存在 → `SignalError`；（预留）房间满 → `RoomError`、媒体 → `MediaError` 骨架
  - [x] 实现：`Error` 新增 `Signal/Room/Media` 变体与子错误；`From` 转换单测
  - [x] 接入：Slice 3 转发的「目标不存在」路径改用 `SignalError`（返回错误消息或记日志，实现时定）
  - 验证：错误类别可归因，单测通过 ✅ 5 个单测；`flush_outboxes` 的 conn_get_mut None → `SignalError::TargetNotFound` 结构化日志；解析失败 → `Error::Serde` 归因日志

### 并行轨（与主线并行，不阻塞）

- [ ] Task 0.5: DATAGRAM 扩展 spike（独立分支，RFC 9221 最小验证，硬上限 2 人日，方案见 spec T4）
  - [ ] tquic frame 层加 `DATAGRAM`（0x30/0x31）帧编解码 + 收发路径 + `max_datagram_frame_size` 传输参数协商
  - [ ] 最小示例往返验证
  - [ ] 结论记录：可行（→ Task 1.4 走 D2a）/ 不可行（→ D2b 回退），更新 roadmap §8 风险矩阵
  - 验证：Datagram 往返实测记录 + 明确结论

- [ ] Task 0.6: 弱网测试基线（依赖 Slice 1 完成，才有可测量对象）
  - [ ] 测量脚本：`tc netem` 5%/10% 丢包 + 200ms 延迟下互发回路延迟/吞吐
  - [ ] 记录基线数值（Phase 2 验收「QUIC 信令延迟 < WS 60%」对照组）
  - 验证：脚本重跑数值可复现（±10%）

- [ ] Task 0.7: Phase 0 验收门
  - [ ] `cargo build && cargo clippy -D warnings && cargo test` 全绿
  - [ ] 所有切片测试保持绿色（回归确认）
  - [ ] checklist.md Phase 0 段全部勾选

## Phase 1 · QUIC 传输底座补全（W3–W6）

> 在 Phase 0 的注册表雏形与转发骨架上补全生产要素，不做推倒重构。

- [ ] Task 1.1: 多连接管理生产化（`src/service/p2p/mod.rs`，方案见 spec T3）
  - [ ] 先写测试（红）：3 并发客户端独立收发、无串流；连/断 100 次无注册表残留
  - [ ] 生命周期事件上抛：`mpsc::UnboundedSender<ConnEvent>`（connected/closed/idle_timeout），异步侧消费
  - [ ] 断连清理：连接关闭移除注册表项并通知
  - 验证：并发集成测试通过

- [ ] Task 1.2: `QuicConf` 补全与接线（`vauid-shared/src/conf/quic.rs` + `src/core/quic/wrap/conf.rs`，方案见 spec T5）
  - [ ] 先写测试（红）：keep-alive 配置解析；0-RTT 二次连接 < 20ms
  - [ ] 新增 `keep_alive_interval: u64`（秒，0 = 禁用，TOML 默认值）；`build_config` 接线（tquic API 缺失则经 handler 定时驱动，记录取舍）
  - [ ] 0-RTT：`client_tls()` 透传 `TlsConf` 开关（替换硬编码 `false` 分支）；服务端开启会话票据
  - 验证：0-RTT 二次连接实测 < 20ms

- [ ] Task 1.3: 连接迁移验证
  - [ ] 先写测试（红）：客户端换 UDP 源地址后会话不中断
  - [ ] 集成测试：模拟 WiFi→蜂窝；记录迁移期丢包/中断
  - 验证：迁移不中断、媒体不丢超 1 帧

- [ ] Task 1.4: DATAGRAM 扩展落地（D2a；依赖 Task 0.5 结论为「可行」，方案见 spec T4）
  - [ ] 先写测试（红）：丢包不重传、乱序投递、吞吐达标
  - [ ] 扩展模块以 feature 门控（`vauid/datagram`）并入主线，不侵入默认路径；尽力而为调度
  - 验证：Datagram 通路可用（音频/视频帧载体就绪）；受阻则执行 D2b 回退并记录

- [ ] Task 1.5: 连接级指标
  - [ ] `quic_connections`（gauge）+ 断连原因分类（idle_timeout/错误码/主动关闭）
  - 验证：三类断连各自递增、可归因

- [ ] Task 1.6: Phase 1 验收门
  - [ ] checklist.md Phase 1 段全部勾选（3 客户端并发稳定 / 0-RTT 生效 / Datagram 可用）

## Phase 2 · 消息协议与双通道信令（W7–W10）〔进入时细化〕

> 前置门：Phase 1 验收通过。细化依据：Task 1.4 的 Datagram 形态、Phase 0 `Message` 帧实测反馈。延续边写边测方法。

- [ ] Task 2.1: `Message` 帧协议完善 + 与 `SignalMessage`/`ServerEvent` 打通（依赖 Phase 0 Task 0.2）
  - [ ] 细化点：流上紧凑二进制布局（定长头 + payload）、信令 payload 复用现有 schema 编码路径
  - 验证：信令/文本同通道 round-trip 单测
- [ ] Task 2.2: 流模型与调度：信令/文本/媒体流 + 优先级（信令 > 音频 > 文本）
  - 验证：优先级次序集成测试
- [ ] Task 2.3: 发送策略：队列 / 背压 / 超时丢弃
  - 验证：积压时旧消息被丢弃、新消息延迟不涨
- [ ] Task 2.4: QUIC 信令与 WS 信令并行：`signal_demo` 增加 QUIC 变体（协议层复用 `SignalMessage`）
  - 验证：同一房间 QUIC 与 WS 客户端互通
- [ ] Task 2.5: `quic_chat` 升级多连接房间聊天（依赖 Task 1.1）
  - 验证：双终端聊天互见
- [ ] Task 2.6: `web/p2p-test.html` 支持「QUIC 模式」开关
  - 验证：浏览器经 WS 与 QUIC 客户端互通
- [ ] Task 2.7: Phase 2 验收门
  - 验证：文本通道 QUIC 化；弱网 5% 下 QUIC 信令延迟 < WS 60%（对照 Task 0.6 基线）

## Phase 3 · 浏览器通路 WebTransport（W11–W13）〔进入时细化〕

> 前置门：Phase 2 验收通过。细化依据：Task 3.1 握手可行性（决定 D5a/D5b 走向）。

- [ ] Task 3.1: WebTransport 服务端（优先 D5a tquic h3 自研；2 周未打通浏览器握手 → 切 D5b quinn+wtransport 并记录）
  - 验证：浏览器 `new WebTransport(url)` 握手成功
- [ ] Task 3.2: `Message` 协议适配 WebTransport 双向流/单向流（依赖 Task 3.1 + 2.1）
  - 验证：信令/文本经 WebTransport 收发，与裸 QUIC 客户端互通
- [ ] Task 3.3: 前端 JS SDK 雏形：`connect()/send()/on()`（`web/` JS 模块）
  - 验证：p2p-test.html「QUIC 模式」可用
- [ ] Task 3.4: 兼容矩阵 + WS 兜底降级（特性探测）
  - 验证：不支持 WebTransport 的浏览器自动降级 WS
- [ ] Task 3.5: Phase 3 验收门：Chrome 双标签页经 WebTransport 完成信令握手并收发文本（不经 WS）

## Phase 4 · QUIC 音视频媒体承载 ⭐（W14–W19）〔进入时细化〕

> 前置门：Phase 3 验收通过。细化依据：D7 选型结论、Datagram 实测表现。延续边写边测：媒体链路按「采集→编码→发送→接收→解码→播放」逐环节切片。

- [ ] Task 4.0: D7 视频编码器决策 spike（候选：WebCodecs / ffmpeg・gstreamer 绑定 / OpenH264・x264 / 纯 Rust dav1d 等）
  - 验证：选型结论（自研客户端编码 + 解码两侧）写入 roadmap §3 D7
- [ ] Task 4.1: 音频链路：Opus → RTP → Datagram → Jitter Buffer → 解码播放（依赖 Task 1.4）
  - 验证：端到端 < 150ms；丢包 5% MOS ≥ 3.5
- [ ] Task 4.2: 视频链路：编码器接入（依赖 Task 4.0）→ RTP → Datagram/Stream → JB → 解码
  - 验证：720p 30fps，延迟 < 300ms
- [ ] Task 4.3: 丢包恢复：NACK（可靠流回传）+ FEC + PLI
  - 验证：丢包 5% 画面无撕裂，恢复 < 200ms
- [ ] Task 4.4: 码率/拥塞控制：发送端自适应（BBR + 反馈）
  - 验证：弱网自动降码率、不崩溃
- [ ] Task 4.5: 双自研客户端 QUIC 对讲联调（无 WS/SRTP）——**主里程碑 M4**
  - 验证：实时对讲 10 分钟稳定；达标后 str0m/SRTP 基线降级为兜底

## Phase 5 · P2P 与 SFU 混合拓扑（W20–W24）〔进入时细化〕

> 前置门：Phase 4（M4）验收通过。

- [ ] Task 5.1: QUIC P2P：NAT 打洞、ICE over QUIC、服务器仅介绍人
  - 验证：2 客户端打洞直连，服务器零媒体带宽
- [ ] Task 5.2: QUIC SFU：Datagram 转发 + 按订阅者码率降层
  - 验证：4 人会议经服务器转发稳定 720p
- [ ] Task 5.3: `publish/subscribe/track_published` 信令闭环（schema 扩展见 roadMap §4.2）
  - 验证：publish → subscribe 通路可用
- [ ] Task 5.4: 混合拓扑切换 FSM：Make-Before-Break（Prepare → Overlap 双发 → Execute → Teardown，2000ms 超时降级；Overlap 期 PLI 强制关键帧 + Simulcast 低层降级）
  - 验证：切换无感、中断 < 300ms；4 人弱网 5% 30 分钟稳定

## Phase 6 · 生产化与生态（W25–W30）〔进入时细化〕

> 前置门：Phase 5 验收通过。

- [ ] Task 6.1: 拥塞控制调优（BBR/反馈参数弱网收敛）
  - 验证：5% 丢包持续稳定
- [ ] Task 6.2: 安全加固：0-RTT 重放防护（服务端令牌 + 重放缓存）、证书轮换、ABR 访问控制
  - 验证：安全清单审计通过
- [ ] Task 6.3: 可观测指标：`media_latency_ms`/`loss_recovery_total`/`switch_total`/`datagram_dropped_total` 等（roadMap §9）+ tracing span
  - 验证：指标可视化
- [ ] Task 6.4: WS 信令下线（依赖 Phase 3 + 4 全量验证）：QUIC/WebTransport 全量接管 + 灰度切流
  - 验证：无 WS 进程，7 天灰度无事故
- [ ] Task 6.5: 前端 SDK `<VauidRoom>` 一行接入
  - 验证：一行接入可用
- [ ] Task 6.6: A/B 报告：QUIC vs WS/WebRTC（延迟/重连/弱网）
  - 验证：三项全面优于基线——**主里程碑 M6**
- [ ] Task 6.7: WebRTC 基线并行验证（str0m P2P/SFU/切换对照）
  - 验证：与 QUIC 线互证

# Task Dependencies

- Phase 间：Phase N 验收门通过后方可进入 Phase N+1
- Phase 0 主线内：Slice 严格顺序（0.1 → 0.2 → 0.3 → 0.4），每步先测后写
- Phase 0 并行轨：Task 0.5（spike）随时可并行；Task 0.6 依赖 Task 0.1
- Phase 1 内：
  - Task 1.4 依赖 Task 0.5（结论为「可行」）
  - Task 1.1 依赖 Task 0.3（注册表雏形）
- Phase 2+ 内：Task 2.1 依赖 Phase 0 Task 0.2；Task 2.5 依赖 Task 1.1；Task 3.2 依赖 Task 3.1 与 2.1；Task 3.3 依赖 Task 3.2；Task 4.1 依赖 Task 1.4；Task 4.2 依赖 Task 4.0；Task 5.4 依赖 Task 5.1/5.2/5.3；Task 6.4 依赖 Phase 3 + 4 全量验证
- 可并行：Task 0.1–0.4 主线 ∥ Task 0.5；Task 1.2 ∥ Task 1.3 ∥ Task 1.4；Task 6.5 ∥ Task 6.6 ∥ Task 6.7
