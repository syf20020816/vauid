# Quic实现P2P消息通信转发

## 1. WebTransport 浏览器环境

WebTransport 是直接在 QUIC 之上构建的浏览器 API，设计目标就是替代 WebSocket 并解决其痛点。

```js
// 浏览器端
const transport = new WebTransport("https://example.com:4433/wt");
await transport.ready;

// 发送数据（单向流）
const stream = await transport.createUnidirectionalStream();
const writer = stream.getWriter();
await writer.write(new TextEncoder().encode("消息"));
await writer.close();

// 接收服务端推送的流
const reader = transport.incomingUnidirectionalStreams.getReader();
const { value: stream } = await reader.read();
```

相比 WebSocket 的优势：

- 多路复用：一个连接上可以同时跑多个独立流，互不阻塞（WebSocket 所有消息共享一个 TCP 连接，队头阻塞）
- 0-RTT 连接建立：重复连接时无需握手延迟
- 更细粒度的控制：可以创建双向流、单向流、或者 datagram（不可靠但低延迟，类似 UDP）
- 原生基于 QUIC/UDP：绕过中间件对 TCP 长连接的干扰

## 2. 直接使用 QUIC 非浏览器环境下

在服务器之间或 IoT/移动端，可以直接用 QUIC 库（如 tquic）实现消息中转：

客户端 A ──QUIC Stream 1──→ 服务端 ──QUIC Stream 2──→ 客户端 B

服务端作为转发器，将 A 的消息通过独立的 QUIC 流转发给 B。每个客户端维护一个 QUIC 连接，服务端内部做流映射。

| 特性       | WebSocket over TCP  | QUIC / WebTransport            |
| ---------- | ------------------- | ------------------------------ |
| 连接建立   | 1-RTT + TLS         | 0-RTT（重连）                  |
| 队头阻塞   | 存在（TCP 层）      | 无（流独立）                   |
| 多路复用   | 单通道，需自己分帧  | 原生多流                       |
| 跨网络穿透 | 易被防火墙/代理阻断 | UDP 可能被限，但端口跳跃更容易 |
| 浏览器支持 | universal           | WebTransport 现代浏览器已支持  |
