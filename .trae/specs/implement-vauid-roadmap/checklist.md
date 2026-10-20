# Checklist

> 每完成一个 Phase 的实现，逐项核验并勾选；全部通过方可进入下一 Phase（对应里程碑 M0–M6）。
> **贯穿性检查（每 Phase 均适用）**：每个切片先有测试（红）→ 实现（绿）；新增代码不回退既有切片测试。

## Phase 0 · 增量起步：互发回路边写边测（M0）

### 切片完成度
- [x] Slice 1：客户端连接后收到服务端主动下发的消息（双向通路确认），测试通过（`p2p_server_welcome`）
- [x] Slice 2：`MsgType`/`Message` round-trip 单测通过（seq/flags 边界覆盖），互发回路已切换为 `Message`（Chat）承载
- [x] Slice 3：双客户端 A↔B 经服务器双向转发、无串流，集成测试通过（`p2p_forward_a_to_b` / `p2p_forward_b_to_a`）
- [x] Slice 4：转发「目标不存在」返回 `SignalError`；`Room`/`Media` 骨架就位，`From` 转换单测通过

### 并行轨
- [x] DATAGRAM spike 有「可行/不可行」明确结论与往返实测记录（不可行则 D2b 回退路径已记录于 roadmap §8）✅ 结论：可行（D2a），`spike/datagram/README.md`
- [x] 弱网基线脚本可复现（±10%），5%/10% 丢包下互发回路延迟/吞吐数值已记录 ✅ `scripts/netem_baseline.sh` + `docs/baseline_20261020_144425.log`；弱网两遍偏差 4–5%；macOS 用应用层注入（dummynet 对回环不生效），附带修复事件循环定时器饥饿生产级 bug（详见 tasks.md Task 0.6）

### 门
- [x] `cargo build && cargo clippy -D warnings && cargo test` 全绿（含全部切片回归）✅ 2026-10-20：14 测试通过、clippy 0 警告

## Phase 1 · QUIC 传输底座（M1）

- [ ] 注册表生产化：3 并发客户端独立收发、无串流；连/断 100 次无残留
- [ ] 生命周期事件经 mpsc 正确上抛（connected/closed/idle_timeout）
- [ ] `QuicConf` 含 `keep_alive_interval`（TOML 默认值与单测），`build_config` 已接线
- [ ] `client_tls()` 透传 0-RTT 开关（无硬编码 `false` 分支），服务端开启会话票据
- [ ] 0-RTT 二次连接实测 < 20ms
- [ ] 连接迁移（UDP 换源地址）不中断、媒体不丢超 1 帧
- [ ] DATAGRAM 扩展 feature 门控可用：丢包不重传、乱序投递、吞吐验证通过
- [ ] `quic_connections` 指标可查，三类断连原因（超时/错误/主动）可归因

## Phase 2 · 消息协议与双通道（M2）

- [ ] 信令/文本经统一 `Message` 帧同通道收发，round-trip 单测通过（含二进制布局）
- [ ] 流优先级集成测试通过（信令 > 音频 > 文本次序生效）
- [ ] 积压场景下旧消息被丢弃、新消息延迟不涨
- [ ] 同一房间内 QUIC 与 WS 客户端消息互通
- [ ] `quic_chat` 双终端聊天互见
- [ ] 浏览器经 WS 与 QUIC 客户端互通（p2p-test.html「QUIC 模式」开关）
- [ ] 弱网 5% 下 QUIC 信令延迟 < WS 的 60%（对照 Phase 0 基线数值）

## Phase 3 · 浏览器通路（M3）

- [ ] WebTransport 服务端 `/webtransport` 握手成功（D5a 成功，或 D5b 回退已记录）
- [ ] `Message` 帧经 WebTransport 双向流/单向流正常收发
- [ ] WebTransport 客户端与裸 QUIC 客户端互通
- [ ] 前端 JS SDK 雏形 `connect()/send()/on()` 可用
- [ ] 不支持 WebTransport 的浏览器自动降级 WS
- [ ] Chrome 双标签页经 WebTransport 完成信令握手并收发文本（不经 WS）

## Phase 4 · QUIC 音视频媒体承载 ⭐（M4）

- [ ] D7 视频编码器选型结论已记录（自研客户端编码 + 解码两侧）
- [ ] 音频链路端到端延迟 < 150ms
- [ ] 丢包 5% 下音频 MOS ≥ 3.5
- [ ] 视频链路 720p 30fps，延迟 < 300ms
- [ ] 丢包 5% 画面无撕裂，NACK/FEC/PLI 恢复 < 200ms
- [ ] 弱网自动降码率、不崩溃
- [ ] 双自研客户端经 QUIC 实时对讲 10 分钟稳定（无 WS/无 SRTP）
- [ ] 达标后 str0m/SRTP 基线降级为兜底（roadmap 标注更新）

## Phase 5 · P2P 与 SFU 混合拓扑（M5）

- [ ] 2 客户端 NAT 打洞直连成功，服务器零媒体带宽
- [ ] 4 人会议经 SFU 转发稳定 720p（含按订阅者码率降层）
- [ ] `publish/subscribe/track_published` 信令闭环可用
- [ ] 拓扑切换 FSM 状态迁移单测通过（含 2000ms 超时降级路径）
- [ ] 4 人小队 P2P↔SFU 自动切换无感，中断 < 300ms
- [ ] 弱网 5% 下 30 分钟稳定

## Phase 6 · 生产化与生态（M6）

- [ ] BBR/反馈参数弱网收敛：5% 丢包持续稳定
- [ ] 0-RTT 重放防护、证书轮换、ABR 访问控制安全审计通过
- [ ] `media_latency_ms`/`loss_recovery_total`/`switch_total`/`datagram_dropped_total` 等指标可视化
- [ ] WS 信令下线：无 WS 进程，7 天灰度无事故
- [ ] `<VauidRoom>` 一行接入可用
- [ ] A/B 报告：QUIC 在延迟/重连/弱网三项全面优于 WS/WebRTC
- [ ] str0m WebRTC 基线对照测试完成，与 QUIC 线互证
