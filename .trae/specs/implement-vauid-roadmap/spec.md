# vauid 路线图阶段化实施 Spec

## Why

roadMap.md（v0.2）已确立「QUIC 全面替代 WebSocket 与 WebRTC 传输」的 7-Phase / 30 周技术路线，但尚未转化为可逐项执行与验收的开发任务。本 spec 基于当前代码进度审计，将路线图落成统一的 tasks / checklist 体系，作为后续实施工作的执行入口。

## What Changes

- 建立覆盖 Phase 0–6 的任务清单（tasks.md）与验收门检查表（checklist.md）

- **增量演进原则（核心方法论，用户指定）**：P2P Server 的构建从最简单的互相发送开始，一步步扩展；**每一步都可运行、有测试伴随**。协议帧、错误树、会话管理等基础设施**不预先备齐**，而是在增量演进需要时才引入——「边写边测试，而非先备料后施工」

- **粒度策略**：近期阶段（Phase 0/1）细化到文件与接口级、可直接开工；远期阶段（Phase 2–6）保持路线图粒度并标注前置门，进入该阶段时基于当时产出再细化

- Phase 0/1 任务按现状修正：已完成项移入基线、部分完成项调整为「补全」语义

- 不引入 roadmap 之外的新需求；D7（视频编码器选型）保留在 Phase 4 内决策

## Impact

- Affected specs: 无（首个 spec）

- Affected code:

  - `vauid-shared`：`src/error.rs`（随场景扩展）、`src/proto/message.rs`（增量引入）、`src/conf/quic.rs`（扩展）

  - `vauid`：`src/core/quic/wrap/conf.rs`（keep-alive/0-RTT 接线）、`src/service/p2p/mod.rs`（增量演进主战场）、tquic DATAGRAM 扩展模块（新增）、`src/bin/`（quic\_chat、signal\_demo）、`web/p2p-test.html`

  - 远期新增系统：流调度与发送策略、WebTransport 端点、媒体管线、拓扑 FSM、指标模块

## 增量演进原则（贯穿全程）

| 原则   | 含义                                        |
| :--- | :---------------------------------------- |
| 垂直切片 | 每步交付「可运行的最小互发能力」，而非横向铺开基础设施               |
| 测试伴随 | 每个切片落地即有对应测试（单测或集成测试），红→绿才算完成             |
| 按需引入 | 协议帧/错误类别/配置项在演进到需要它的那一步才出现                |
| 雏形先行 | 完整实现（如多连接管理）先以最小雏形出现在早期切片，后续 Phase 补全为生产级 |

## 现状基线（2026-09-04 审计）

> 与 roadMap §1 盘点相比的增量核实。**已完成项不进入任务清单。**

| 项                           | 状态     | 证据                                                                                      |
| :-------------------------- | :----- | :-------------------------------------------------------------------------------------- |
| D1–D6 决策                    | ✅ 已定   | roadMap §3 各决策门标注「已定」；D7 留待 Phase 4                                                     |
| tracing 结构化日志 + request\_id | ✅ 已完成  | `vauid/src/log/mod.rs`（console + JSON 双输出、request\_span）                                |
| 错误分类树                       | ⚠️ 部分  | `vauid-shared/src/error.rs` 已有 Quic/IO/Serde/Config/Other；**缺 Signal/Room/Media**（按需引入） |
| 协议 schema                   | ⚠️ 部分  | `SignalMessage`/`ServerEvent`/Track 模型已有；**缺** **`MsgType`/`Message`** **帧协议**（按需引入）    |
| QuicConf 空闲超时               | ✅ 已有   | `conf/quic.rs` `max_idle_timeout`（wrap 已接线）；**缺 keep-alive**                            |
| 0-RTT                       | ⚠️ 未接线 | `TlsConf` 有 0-RTT 开关，但 `wrap/conf.rs` 客户端分支在无 tls 配置时硬编码 `false`                        |
| 传输底座（封装/socket/echo/客户端）    | ✅ 已完成  | 12 测试全绿、clippy 0 警告                                                                     |
| DATAGRAM spike              | ❌ 未做   | 无相关代码（独立并行轨道）                                                                           |
| 弱网测试基线                      | ❌ 未做   | 无脚本/报告（待互发回路可用后测量）                                                                      |
| 多连接管理                       | ❌ 未做   | `P2PHandler` 为无状态回显——**增量演进的起点**                                                        |

## 增量演进路线（Phase 0 Server 构建路径）

```
现状（回显：客户端发 → 原样回写）
  │
  ├─ Slice 1 双向互发：服务端可主动向客户端下发消息（不再只是被动回显）
  │            每步测试：连接后收到服务端主动消息；客户端发→服务端日志可查
  │
  ├─ Slice 2 带类型的消息：互发内容需要区分 → 引入 Message 帧（MsgType 先 Signal/Chat）
  │            每步测试：帧 round-trip 单测 + 互发回路跑通 Chat 帧
  │
  ├─ Slice 3 双客户端转发：A 发 → server 转发 → B 收（最小会话注册雏形：HashMap<id, conn>）
  │            每步测试：A↔B 双向互通、无串流（2 客户端）
  │
  ├─ Slice 4 错误树生长：转发中真实出现的错误逐类入树
  │            （目标不存在→Signal、房间满→Room、解析失败→已有 Serde）
  │
  ├─ 并行轨 A：DATAGRAM spike（RFC 9221 可行性，独立分支，不阻塞主线）
  └─ 并行轨 B：弱网基线（Slice 1 完成后对互发回路测量）
```

Phase 1 在此雏形上补全生产要素：并发多连接（注册表生产化 + 事件上抛）、0-RTT、连接迁移、Datagram 落地、指标。

## 技术方案要点（Phase 0/1）

### T1 错误树扩展（随 Slice 4 生长）

保持 `vauid-shared/src/error.rs` 现有 `Error` 顶层枚举不变，按需新增：

```rust
pub enum Error {
    // ...现有：Other/Quic/IO/Serde/Config
    Signal(SignalError),   // 目标 client 不存在、非 join 前置消息、join 冲突
    Room(RoomError),       // 房间不存在、满员（>4）、创建失败
    Media(MediaError),     // 编解码、RTP 封装/解析、JB 溢出（Phase 4 前仅骨架）
}
```

原则：每个错误类别在**真实场景触发时**才入树并补 `From` 转换单测；现有信令模块（`core/signal/`）字符串错误随迁移。

### T2 `Message` 帧协议（Slice 2 引入，Phase 2 完善）

`vauid-shared/src/proto/message.rs`，对齐 roadMap §4.1：

- `MsgType { Signal, Chat, Media, Control }`——**Slice 2 只用到 Signal/Chat**，Media/Control 预留取值

- `Message { ty: MsgType, seq: u64, flags: u8, payload: Vec<u8> }`

- Phase 0：serde derive + round-trip 单测 + 接入互发回路（payload 承载 Chat 文本）

- Phase 2 落地流上传输时再定紧凑二进制布局（定长头 + payload），避免过早设计

- 传输映射（Phase 2 起生效）：信令/文本 → 可靠流；音频/非参考帧 → Datagram；关键帧/RTCP → 最高优先级

### T3 会话注册雏形 → 多连接管理（Slice 3 雏形，Phase 1 生产化）

约束（来自 `service/p2p/mod.rs` 文档注释）：tquic 事件循环同步驱动、内部 `Rc<RefCell<..>>` 非 `Send`，`P2PServer` 不可跨线程共享。

- **Slice 3（雏形）**：`P2PHandler` 持有 `Rc<RefCell<HashMap<ConnKey, SessionState>>>`，`on_conn_established` 注册、`on_conn_closed` 移除；转发 = 查表 + 定向 `stream_write`。键选型（tquic Connection ID 获取方式）在实现时确认

- **Phase 1（生产化）**：加 `mpsc::UnboundedSender<ConnEvent>` 生命周期事件上抛（connected/closed/idle\_timeout）、断连清理与并发验证（3 客户端）

### T4 DATAGRAM 扩展（并行轨 A：spike → Phase 1 落地）

tquic 1.6 无 DATAGRAM 帧 API（roadMap §3 D2 已核实）。路径：

1. spike（独立分支，2 人日硬上限）：tquic frame 层增加 RFC 9221 `DATAGRAM`（0x30/0x31）收发 + 传输参数 `max_datagram_frame_size` 协商，验证往返
2. 可行 → Phase 1 以 feature 门控（`vauid/datagram`）落地为独立扩展模块
3. 不可行 → 回退 D2b（纯 STREAM：短流 + `fin` 边界 + 过期丢弃），更新 roadmap §8 风险矩阵

### T5 keep-alive / 0-RTT 接线（Phase 1）

- `QuicConf` 新增 `keep_alive_interval: u64`（秒，0 = 禁用），`build_config` 接线（tquic API 存在性以文档为准，缺失则经 handler 定时驱动）

- 0-RTT：`client_tls()` 透传 `TlsConf` 开关（替换硬编码 `false`）；服务端开启会话票据

- 验收以实测为准：二次连接握手 < 20ms

## ADDED Requirements

### Requirement: Phase 0 增量起步——互发回路边写边测

系统 SHALL 以增量切片方式构建 P2P Server：从双向互发开始，逐步引入带类型消息、双客户端转发、随场景生长的错误树；**每个切片交付时必须伴随可通过的测试**，基础设施（协议帧/错误类别）不得先于其被需要的切片而实现。

#### Scenario: 切片完成定义

- **WHEN** 任一切片（如「双客户端转发」）实现完成

- **THEN** 对应测试（round-trip 单测或集成测试）先红后绿，且此前所有切片测试保持绿色

#### Scenario: Phase 0 验收

- **WHEN** 执行 `cargo build && cargo clippy -D warnings && cargo test`

- **THEN** 全绿；双客户端经服务器互发消息可用；DATAGRAM spike 有「可行/不可行」结论；弱网基线数值可复现

### Requirement: Phase 1 QUIC 传输底座补全

系统 SHALL 在 Phase 0 雏形上补全生产要素：多连接管理（注册表生产化 + 生命周期事件上抛）、0-RTT（二次连接 < 20ms）、连接迁移（换源地址不中断）、DATAGRAM 扩展（特性门控）与连接级指标。

#### Scenario: 3 并发客户端

- **WHEN** 3 个客户端同时连接并各自收发

- **THEN** 互不串流、独立稳定

#### Scenario: DATAGRAM 传输

- **WHEN** 经 Datagram 发送数据帧

- **THEN** 丢包不重传、乱序投递，吞吐验证通过

### Requirement: Phase 2 消息协议与双通道信令（门控：Phase 1 验收通过后细化）

系统 SHALL 完善 `Message` 帧协议并与 `SignalMessage` 打通，提供流优先级调度与发送策略，实现 QUIC 信令与 WS 信令并行互通；弱网 5% 下 QUIC 信令延迟 < WS 的 60%。

### Requirement: Phase 3 浏览器通路 WebTransport（门控：Phase 2 验收通过后细化）

系统 SHALL 提供 `/webtransport` 端点（优先 tquic 自研，受阻 2 周切 quinn+wtransport），消息协议适配双向流，前端 SDK 雏形可用，不支持时自动降级 WS；Chrome 双标签页不经 WS 完成信令握手与文本收发。

### Requirement: Phase 4 QUIC 音视频媒体承载（门控：Phase 3 验收通过后细化，含 D7 决策）

系统 SHALL 以 RTP over QUIC 承载音视频：音频端到端 < 150ms（丢包 5% MOS ≥ 3.5）、720p30 视频 < 300ms、丢包恢复 < 200ms、BBR 码率自适应；双自研客户端无 WS/SRTP 对讲 10 分钟稳定（M4）。

### Requirement: Phase 5 P2P 与 SFU 混合拓扑（门控：Phase 4 验收通过后细化）

系统 SHALL 支持 QUIC P2P 直连（NAT 打洞、服务器零媒体带宽）、SFU 转发（按订阅者降层）、publish/subscribe 信令闭环、Make-Before-Break 拓扑切换（中断 < 300ms，弱网 5% 30 分钟稳定）。

### Requirement: Phase 6 生产化与生态（门控：Phase 5 验收通过后细化）

系统 SHALL 完成弱网调优、安全加固（0-RTT 重放防护/证书轮换）、指标面板、WS 下线（7 天灰度无事故）、`<VauidRoom>` SDK 一行接入，并产出 QUIC 全面优于 WS/WebRTC 的 A/B 报告（M6）。

## MODIFIED Requirements

（无——首个 spec，无既有 spec 需修改）

## REMOVED Requirements

（无）

### 与 roadMap 的映射关系

| roadMap 条目                        | 本 spec 处理                                       |
| :-------------------------------- | :---------------------------------------------- |
| §5 Phase 0「tracing 日志」            | 已完成，移入现状基线，不建任务                                 |
| §5 Phase 0「D1–D6 确认」              | 已在 §3 标注「已定」，不建任务                               |
| §5 Phase 0「错误树」                   | **改为随 Slice 4 场景生长**（见 T1），非预先备齐                |
| §5 Phase 0「Message 骨架」            | **改为 Slice 2 按需引入**（见 T2），非预先备齐                 |
| §5 Phase 1「多连接管理重构」               | **改为：Slice 3 先落雏形，Phase 1 生产化**（见 T3）           |
| §5 Phase 1「0-RTT/keep-alive/空闲超时」 | 修正为「新增 keep-alive；0-RTT 修客户端接线 + 服务端会话票据」（见 T5） |
| §3 D7 编码器决策                       | 转化为 Phase 4 首个任务（spike）                         |
| §5 Phase 2–6 各表                   | 保留任务级粒度 + 前置门，进入阶段时细化                           |

