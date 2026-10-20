use std::collections::BinaryHeap;
use std::cmp::Reverse;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::core::Result;
use dashmap::DashMap;
use slab::Slab;
use tokio::net::UdpSocket;
use tquic::{PacketInfo, PacketSendHandler};
use vauid_shared::error::{Error, QuicError};

// ---------------------------------------------------------------- 弱网注入 --

/// 弱网注入配置（Task 0.6 基线用；仅作用于收包路径，等价单向网络损伤）。
///
/// 延迟通过「到达暂存 + 到期重注入」实现：包到达后按交付时刻入队，
/// `recv_from` 按期弹出，突发包不会被串行 sleep 拉长。
#[derive(Clone, Copy, Debug)]
pub struct ChaosConfig {
    /// 丢包率，范围 [0, 1)
    pub loss_rate: f64,
    /// 单向延迟（收包路径；双端同进程共享配置时 RTT 增加约 2 倍）
    pub delay: Duration,
}

static CHAOS: OnceLock<ChaosConfig> = OnceLock::new();

/// 设置进程级弱网注入（须在端点启动前调用；双端同进程共享）
pub fn set_chaos(cfg: ChaosConfig) {
    let _ = CHAOS.set(cfg);
}

fn chaos_current() -> Option<ChaosConfig> {
    CHAOS.get().copied()
}

/// 轻量 xorshift64* 伪随机（非加密安全，仅测试注入用）
fn fast_random() -> f64 {
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut s = STATE.load(Ordering::Relaxed);
    if s == 0 {
        s = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64 | 1)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
    }
    s ^= s >> 12;
    s ^= s << 25;
    s ^= s >> 27;
    STATE.store(s, Ordering::Relaxed);
    (s.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 11) as f64 / (1u64 << 53) as f64
}

/// 延迟重注入队列条目：(交付时刻, 序号, 数据, 远端地址)
type DelayedEntry = (Instant, u64, Vec<u8>, SocketAddr);

fn next_seq() -> u64 {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    SEQ.fetch_add(1, Ordering::Relaxed)
}

/// 用于 QUIC 的 UDP 套接字封装
/// 基于tokio的UdpSocket
pub struct QuicSocket {
    /// QUIC 端点底层的 UDP 套接字
    pub udp_sock: Slab<UdpSocket>,
    /// 本地地址与套接字标识符之间的映射
    pub addr_map: DashMap<SocketAddr, usize>,
    /// 初始化socket绑定的本地地址
    pub local_addr: SocketAddr,
    /// 弱网延迟重注入队列（min-heap，按交付时刻排序）
    delay_q: Mutex<BinaryHeap<Reverse<DelayedEntry>>>,
}

impl QuicSocket {
    pub async fn new(local: &SocketAddr) -> Result<Self> {
        let mut udp_sock = Slab::new();
        let addr_map = DashMap::new();

        let sock = UdpSocket::bind(*local).await?;
        // 预热：触发 IO 驱动注册并缓存可写就绪态。
        // 否则 tquic 的同步回调（PacketSendHandler::on_packets_send -> try_send_to）
        // 首次调用会因就绪态缓存为空直接返回 WouldBlock，导致 Initial 无法发出。
        let _ = sock.writable().await;
        let local_addr = sock.local_addr()?;

        let sid = udp_sock.insert(sock);
        // 插入本地地址与套接字标识符之间的映射
        addr_map.insert(local_addr, sid);
        udp_sock.get_mut(sid);

        Ok(Self {
            udp_sock,
            addr_map,
            local_addr,
            delay_q: Mutex::new(BinaryHeap::new()),
        })
    }

    pub async fn new_client_socket(ipv4: bool) -> Result<Self> {
        let local = if ipv4 {
            IpAddr::V4(Ipv4Addr::UNSPECIFIED)
        } else {
            IpAddr::V6(Ipv6Addr::UNSPECIFIED)
        };

        Self::new(&SocketAddr::new(local, 0)).await
    }

    fn sock_from_map(&self, src: SocketAddr, buf: Option<&[u8]>) -> Result<usize> {
        match self.addr_map.get(&src) {
            Some(sid) => Ok(*sid),
            None => Err(QuicError::SendAddrNotFound {
                addr: src,
                buf: buf.map(|buf| buf.to_vec()),
            }
            .into()),
        }
    }

    /// 向指定地址发送数据
    /// 返回发送数据的长度
    pub async fn send_to(&self, buf: &[u8], src: SocketAddr, dst: SocketAddr) -> Result<usize> {
        let sid = self.sock_from_map(src, Some(buf))?;

        match self.udp_sock.get(sid) {
            Some(sock) => Ok(sock.send_to(buf, dst).await?),
            None => Err(QuicError::SendAddrNotFound {
                addr: src,
                buf: Some(buf.to_vec()),
            }
            .into()),
        }
    }

    /// 同步发送数据（非阻塞）。
    ///
    /// 用于 tquic 的 [`PacketSendHandler`] 回调（同步 trait 方法，无法 await）。
    /// socket 缓冲区满时返回 `WouldBlock` 错误。
    pub fn try_send_to(&self, buf: &[u8], src: SocketAddr, dst: SocketAddr) -> Result<usize> {
        let sid = self.sock_from_map(src, Some(buf))?;

        match self.udp_sock.get(sid) {
            Some(sock) => sock.try_send_to(buf, dst).map_err(Into::into),
            None => Err(QuicError::SendAddrNotFound {
                addr: src,
                buf: Some(buf.to_vec()),
            }
            .into()),
        }
    }

    /// 从指定地址接收数据
    /// 返回接收数据的长度、本地地址、远程地址
    ///
    /// 弱网注入启用时：丢包按概率吞掉；延迟包按交付时刻暂存到期重注入
    pub async fn recv_from(&self, buf: &mut [u8], src: SocketAddr) -> Result<RecvFrom> {
        let sid = self.sock_from_map(src, Some(buf))?;

        let socket = match self.udp_sock.get(sid) {
            Some(sock) => sock,
            None => {
                return Err(QuicError::RecvAddrNotFound {
                    addr: src,
                    buf: Some(buf.to_vec()),
                }
                .into());
            }
        };

        loop {
            // 1. 先交付延迟队列中到期包（可能多个突发包同时到期，逐次弹出）
            if let Some(entry) = self.pop_due() {
                let (_, _, data, remote) = entry;
                let n = data.len().min(buf.len());
                buf[..n].copy_from_slice(&data[..n]);
                return Ok(RecvFrom {
                    len: data.len(),
                    local: self.local_addr,
                    remote,
                });
            }

            let chaos = chaos_current();
            let next_due = self.next_due();

            // 2. 新到包与「等到期」竞速：注入延迟时包先入队，由下一轮循环交付。
            //    注意 tokio::select! 的分支表达式无条件求值，队列空时用
            //    pending() 兜底（永不完成），不能用 precondition + unwrap。
            let due_wait = async {
                match next_due {
                    Some(due) => tokio::time::sleep_until(tokio::time::Instant::from_std(due)).await,
                    None => std::future::pending::<()>().await,
                }
            };
            tokio::select! {
                res = socket.recv_from(buf) => {
                    let (len, remote) = res.map_err(Error::IO)?;
                    match chaos {
                        Some(c) if c.loss_rate > 0.0 && fast_random() < c.loss_rate => {
                            continue; // 丢包：吞掉继续收
                        }
                        Some(c) if !c.delay.is_zero() => {
                            let data = buf[..len].to_vec();
                            let mut q = self.delay_q.lock().unwrap();
                            q.push(Reverse((Instant::now() + c.delay, next_seq(), data, remote)));
                            continue;
                        }
                        _ => {
                            // 数据已由 recv_from 写入 buf，直接返回
                            return Ok(RecvFrom {
                                len,
                                local: self.local_addr,
                                remote,
                            });
                        }
                    }
                }
                // 到期后循环头部负责弹出
                _ = due_wait => {
                    continue;
                }
            }
        }
    }

    /// 最近一个待交付包的到期时刻
    fn next_due(&self) -> Option<Instant> {
        self.delay_q.lock().unwrap().peek().map(|Reverse(e)| e.0)
    }

    /// 弹出一个已到期包（无到期包返回 None）
    fn pop_due(&self) -> Option<DelayedEntry> {
        let mut q = self.delay_q.lock().unwrap();
        if let Some(Reverse(e)) = q.peek()
            && e.0 <= Instant::now()
        {
            return q.pop().map(|Reverse(e)| e);
        }
        None
    }
}

/// 接收数据结构体
/// 包含接收数据的长度、本地地址、远程地址
#[derive(Debug)]
pub struct RecvFrom {
    pub len: usize,
    /// 本地地址
    pub local: SocketAddr,
    /// 远程地址
    pub remote: SocketAddr,
}

/// 实现 tquic 的发送回调：把 QUIC 协议栈产出的数据包通过 UDP 发送出去。
///
/// tquic 的事件循环是同步驱动的，因此这里使用非阻塞 `try_send_to`：
/// 返回的发送数小于 `pkts.len()` 时，tquic 会在后续 tick 重试剩余包。
impl PacketSendHandler for QuicSocket {
    fn on_packets_send(&self, pkts: &[(Vec<u8>, PacketInfo)]) -> tquic::Result<usize> {
        let mut sent = 0;
        for (buf, info) in pkts {
            match self.try_send_to(buf, info.src, info.dst) {
                // 完整发送才算一个包
                Ok(n) if n == buf.len() => sent += 1,
                // 部分发送 / 缓冲区满 / 其他错误：停止本轮，交还 tquic 重试
                _ => break,
            }
        }
        Ok(sent)
    }
}
