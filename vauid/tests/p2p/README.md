# P2P 测试 (Quic)

基于 tquic 的 QUIC P2P 链路测试。

- `client.rs`：客户端集成测试。自生成自签名证书，启动 P2P 服务器（回显转发），
  客户端连接后发送消息并校验回显内容。

运行：

```bash
cargo test --test p2p_client
```
