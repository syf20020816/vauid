1. 信令服务（必须）

- [ ] WebSocket 连接接入
- [ ] 客户端鉴权（可选）
- [ ] 房间创建 / 加入 / 离开
- [ ] 参与者加入 / 离开
- [ ] SDP Offer/Answer 交换
- [ ] ICE Candidate 交换

1. WebRTC 对接（必须）

- [ ] 创建 PeerConnection
- [ ] 配置 ICE Server (STUN)
- [ ] 监听 OnTrack
- [ ] 监听 OnIceCandidate
- [ ] 处理客户端 Offer → 生成 Answer

2. SFU 核心（必须）

- [ ] 房间内媒体流分发
- [ ] Track 发布（Publish）
- [ ] Track 订阅（Subscribe）
- [ ] RTP 包转发（核心逻辑）

3. 基础维护（必须）

- [ ] 连接断开处理
- [ ] 房间自动清理
- [ ] 参与者超时踢出

4. 进阶（生产需要）

- [ ] 带宽估计 BWE / GCC
- [ ] Simulcast 分层转发
- [ ] 音频混音（可选）
- [ ] 录制（可选）
- [ ] 数据通道 DataChannel 转发