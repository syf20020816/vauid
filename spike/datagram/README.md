# tquic DATAGRAM 扩展 spike 报告

> RFC 9221（QUIC DATAGRAM 帧）可行性验证 · 2026-09-04
> 对应 roadMap §3 D2 决策门：媒体不可靠传输载体选型

## 结论

**可行（D2a 成立）。** 在 tquic 1.6.0 上以最小侵入补丁实现 DATAGRAM 帧收发，
双端握手后 1B / 64B / 1200B 三种载荷往返实测通过：

```
[client] echo received (1 bytes, recv queue len 1)
[client] echo received (64 bytes, recv queue len 0)
[client] echo received (1200 bytes, recv queue len 0)
----
echoed datagrams: [1, 64, 1200]
ROUND-TRIP OK (3 of 3 datagrams matched)
```

## tquic 补丁改动点

DATAGRAM 补丁现已**独立维护于根目录 `tquic-vauid/`**（独立 git 仓库，基于 tquic 1.6.0 fork，
引入方式为 path 依赖 `tquic = { path = "../../../tquic-vauid", package = "tquic-vauid" }`）。
改动点对照上游 tquic 1.6.0 源码集中在四处：

| 文件                                   | 改动                                                                                                                                                                                       |
| :----------------------------------- | :--------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tquic-vauid/src/frame.rs`                 | `Frame::Datagram { data: Bytes }` 变体（帧类型 0x30，变长长度前缀）+ 编解码 + qlog 分支                                                                                                                     |
| `tquic-vauid/src/trans_param.rs`           | `max_datagram_frame_size`（TP 0x28）字段、编解码与克隆；`None`/0 表示未启用                                                                                                                               |
| `tquic-vauid/src/lib.rs`（Config）           | `set_max_datagram_frame_size(v)`：双端需同时通告非零值才启用                                                                                                                                           |
| `tquic-vauid/src/connection/connection.rs` | `datagram_send(Bytes)` 入发送队列；`datagram_recv() -> Option<Bytes>` 供应用读取；组包路径 `try_write_datagram_frames` 填帧；`select_send_packet_type` 有排队 datagram 时触发 1-RTT 包；对端未通告参数时发送侧报 `InvalidState` |
| `TransportHandler`（trait）            | 新增 `on_datagram_received` 默认回调，不破坏现有 handler                                                                                                                                             |

## demo 结构与运行

- `spike/src/main.rs`：server + client 两个 OS 线程各自同步驱动 `Endpoint`（tquic 非 `Send`）；
  rcgen 自签证书（复用主项目测试模式）；client 握手后发送三种大小的 datagram，
  server 原样回投，client 校验内容一致。
- 依赖：`tquic` 通过 path 指向独立仓库 `tquic-vauid/`（crate 名 `tquic-vauid`，依赖重映射为 `tquic`）。

```bash
cd spike/datagram
cargo run --release -p spike
```

## 限制与未覆盖项

- **未做负协商测试**：对端不通告 `max_datagram_frame_size` 时仅实现发送侧报错路径，未集成测试

- **未验证弱网行为**：丢包不重传、乱序投递是协议语义推论（无 ACK/重传机制），需 Phase 1 在 `tc netem` 下实测吞吐与乱序率

- **拥塞控制交互未调优**：datagram 计入拥塞窗口但不受流控，与 BBR 的交互需 Phase 1 专项调参

- **PMTU 依赖**：>1200B 的 datagram 依赖 DPLPMTUD 探测完成，demo 中 1200B 恰好在安全线内

## 落地状态（2026-09-14 更新）

- DATAGRAM 补丁已从 spike**独立抽为根目录 `tquic-vauid/` 仓库**（独立 git 维护，含自身 CHANGELOG.vauid.md）
- spike 的 demo 通过 path 依赖直接引用 `tquic-vauid`，不再内嵌 tquic 源码
- 主项目 `vauid` 仍使用 crates.io `tquic = "1.6.0"`，切换到 tquic-vauid 留待 Phase 1 Task 1.4（feature 门控）

