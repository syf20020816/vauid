use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use crate::core::Result;
use dashmap::DashMap;
use slab::Slab;
use tokio::net::UdpSocket;
use tquic::{PacketInfo, PacketSendHandler};
use vauid_shared::error::{Error, QuicError};

/// 用于 QUIC 的 UDP 套接字封装
/// 基于tokio的UdpSocket
pub struct QuicSocket {
    /// QUIC 端点底层的 UDP 套接字
    pub udp_sock: Slab<UdpSocket>,
    /// 本地地址与套接字标识符之间的映射
    pub addr_map: DashMap<SocketAddr, usize>,
    /// 初始化socket绑定的本地地址
    pub local_addr: SocketAddr,
}

impl QuicSocket {
    pub async fn new(local: &SocketAddr) -> Result<Self> {
        let mut udp_sock = Slab::new();
        let addr_map = DashMap::new();

        let sock = UdpSocket::bind(*local).await?;
        let local_addr = sock.local_addr()?;

        let sid = udp_sock.insert(sock);
        // 插入本地地址与套接字标识符之间的映射
        addr_map.insert(local_addr, sid);
        udp_sock.get_mut(sid);

        Ok(Self {
            udp_sock,
            addr_map,
            local_addr,
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
            Some(sock) => Ok(sock.try_send_to(buf, dst)?),
            None => Err(QuicError::SendAddrNotFound {
                addr: src,
                buf: Some(buf.to_vec()),
            }
            .into()),
        }
    }

    /// 从指定地址接收数据
    /// 返回接收数据的长度、本地地址、远程地址
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

        match socket.recv_from(buf).await {
            Ok((len, remote)) => Ok(RecvFrom {
                len,
                local: self.local_addr,
                remote,
            }),
            Err(e) => Err(Error::IO(e)),
        }
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
