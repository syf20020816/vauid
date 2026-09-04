//! QUIC P2P 客户端集成测试。
//!
//! 链路：自生成自签名证书 → 启动 P2P 服务器（单会话回显 / 多会话转发，
//! 连接建立后主动下发 welcome）→ 客户端连接并发消息 → 校验回显或转发结果。
//!
//! tquic 的 `Endpoint` 内部使用 `Rc<RefCell<..>>`（非 `Send`），
//! 服务器与客户端各自在独立线程的 `current_thread` runtime 中独占运行。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use vauid::service::p2p::client::QuicClient;
use vauid::service::p2p::P2PServer;
use vauid_shared::conf::{ConfRW, QuicConf, TlsConf};
use vauid_shared::proto::{Message, MsgType};

/// 生成自签名证书与 quic 配置文件（写入临时目录），返回 (证书路径, 私钥路径, 配置文件路径)。
/// 临时目录按进程 + 自增序号区分，避免同一测试二进制内的并行测试互相覆盖。
fn setup_conf() -> (PathBuf, PathBuf, PathBuf) {
    static SEQ: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "vauid-p2p-test-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // 自签名证书（客户端 verify=false，无需 CA 信任链）
    let key_pair = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::default();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "vauid".to_string());
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName(
            rcgen::Ia5String::try_from("localhost".to_string()).unwrap(),
        ),
        rcgen::SanType::IpAddress(IpAddr::V4(Ipv4Addr::LOCALHOST)),
    ];
    let cert = params.self_signed(&key_pair).unwrap();

    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    let conf_path = dir.join("quic.conf.toml");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

    let conf = QuicConf {
        tls: Some(TlsConf {
            cert_file: Some(cert_path.to_string_lossy().into_owned()),
            key_file: Some(key_path.to_string_lossy().into_owned()),
            alpn: vec!["vauid".into()],
            ..Default::default()
        }),
        ..Default::default()
    };
    conf.save(&conf_path).unwrap();

    (cert_path, key_path, conf_path)
}

/// 客户端连接 QUIC 服务器，发送消息并校验服务器回显（Message 帧 JSON 字节原样返回）
#[test]
fn p2p_client_echo() {
    let (_, _, conf_path) = setup_conf();
    // 客户端发送的文本载荷，实际发送时会被包成 Chat 类型的 Message 帧再序列化
    let msg = b"hello vauid".to_vec();
    let expected = Message {
        ty: MsgType::Chat,
        seq: 0,
        flags: 0,
        payload: msg.clone(),
    };
    let expected_bytes = serde_json::to_vec(&expected).expect("serialize expected frame");

    // 服务器线程：绑定随机端口后上报地址，随后进入事件循环（回显）
    let (addr_tx, addr_rx) = mpsc::channel();
    let srv_conf = conf_path.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listen: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let mut server = P2PServer::bind_with_conf(listen, &srv_conf)
                .await
                .expect("server bind");
            let _ = addr_tx.send(server.socket.local_addr);
            server.run().await.expect("server run");
        });
    });
    let server_addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server addr");

    // 客户端线程：连接 + 发送消息 + 等待回显
    let (result_tx, result_rx) = mpsc::channel();
    let cli_conf = conf_path;
    let client_msg = msg.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut client = QuicClient::connect(server_addr, &cli_conf, client_msg)
                .await
                .expect("client connect");
            client.run().await.expect("client run");
            let _ = result_tx.send(client.state.borrow().received.clone());
        });
    });

    let received = result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("client echo timeout");
    assert_eq!(
        received, Some(expected_bytes),
        "echo 应原样返回 Message 帧的 JSON 序列化字节"
    );
}

/// 客户端连接 QUIC 服务器后，校验服务端主动下发的欢迎消息（解析为 Message 帧）
#[test]
fn p2p_server_welcome() {
    let (_, _, conf_path) = setup_conf();
    let msg = b"hello vauid".to_vec();

    // 服务器线程：绑定随机端口后上报地址，随后进入事件循环（回显 + 下发 welcome）
    let (addr_tx, addr_rx) = mpsc::channel();
    let srv_conf = conf_path.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listen: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let mut server = P2PServer::bind_with_conf(listen, &srv_conf)
                .await
                .expect("server bind");
            let _ = addr_tx.send(server.socket.local_addr);
            server.run().await.expect("server run");
        });
    });
    let server_addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server addr");

    // 客户端线程：连接 + 发送消息 + 等待回显与 welcome 均到达
    let (result_tx, result_rx) = mpsc::channel();
    let cli_conf = conf_path;
    let client_msg = msg.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut client = QuicClient::connect(server_addr, &cli_conf, client_msg)
                .await
                .expect("client connect");
            client.run().await.expect("client run");
            let _ = result_tx.send(client.state.borrow().server_msgs.clone());
        });
    });

    let server_msgs = result_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("client welcome timeout");
    let welcome: Message = serde_json::from_slice(
        server_msgs
            .first()
            .expect("应至少收到一条服务端下发消息"),
    )
    .expect("首条服务端消息应为合法的 Message 帧");
    assert_eq!(welcome.ty, MsgType::Chat, "welcome 帧类型应为 Chat");
    assert_eq!(
        welcome.payload,
        b"welcome to vauid".to_vec(),
        "welcome 帧载荷应为欢迎文本"
    );
}

/// 双客户端测试中单个客户端的收包结果
struct ClientOutcome {
    /// 服务端主动下发的消息（服务端发起流上的原始字节，含 welcome 与转发消息）
    server_msgs: Vec<Vec<u8>>,
    /// 客户端发起流上的回显数据（None 表示未收到回显）
    received: Option<Vec<u8>>,
}

/// 双客户端转发通用流程（用 mpsc 通道保证时序，避免竞态）：
///
/// 1. 服务器线程启动；
/// 2. first 先连接（此时注册表仅有 first，其消息被服务器回显），
///    收到 welcome 即代表已注册，经通道通知主线程后继续等待转发消息；
/// 3. 主线程收到信号后 second 再连接（first 已注册，second 的消息被转发给
///    first，不回显给 second）；
/// 4. 双方各收齐结果后经通道发回。
///
/// 返回 (first 结果, second 结果)。全程 mpsc + recv_timeout 防挂起。
fn run_forward_pair(
    conf_path: &Path,
    first_msg: Vec<u8>,
    second_msg: Vec<u8>,
) -> (ClientOutcome, ClientOutcome) {
    // 服务器线程：绑定随机端口后上报地址，随后进入事件循环（转发）
    let (addr_tx, addr_rx) = mpsc::channel();
    let srv_conf = conf_path.to_path_buf();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let listen: SocketAddr = "127.0.0.1:0".parse().unwrap();
            let mut server = P2PServer::bind_with_conf(listen, &srv_conf)
                .await
                .expect("server bind");
            let _ = addr_tx.send(server.socket.local_addr);
            server.run().await.expect("server run");
        });
    });
    let server_addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server addr");

    // first 客户端线程：连接后消息被回显，收到 welcome 即视为已注册，
    // 通知主线程放行 second，随后继续等待 second 的消息被转发过来
    let (first_ready_tx, first_ready_rx) = mpsc::channel();
    let (first_tx, first_rx) = mpsc::channel();
    let first_conf = conf_path.to_path_buf();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut client = QuicClient::connect(server_addr, &first_conf, first_msg)
                .await
                .expect("first client connect");
            client
                .run_until(|s| !s.server_msgs.is_empty())
                .await
                .expect("first client welcome");
            let _ = first_ready_tx.send(());
            client
                .run_until(|s| s.server_msgs.len() >= 2)
                .await
                .expect("first client forwarded msg");
            let state = client.state.borrow();
            let _ = first_tx.send(ClientOutcome {
                server_msgs: state.server_msgs.clone(),
                received: state.received.clone(),
            });
        });
    });

    // 等 first 注册完成（收到 welcome）后再放行 second，保证转发语义确定
    first_ready_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("first client ready timeout");

    // second 客户端线程：连接后消息被转发给 first，自己只收到 welcome
    let (second_tx, second_rx) = mpsc::channel();
    let second_conf = conf_path.to_path_buf();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async move {
            let mut client = QuicClient::connect(server_addr, &second_conf, second_msg)
                .await
                .expect("second client connect");
            client
                .run_until(|s| !s.server_msgs.is_empty())
                .await
                .expect("second client welcome");
            let state = client.state.borrow();
            let _ = second_tx.send(ClientOutcome {
                server_msgs: state.server_msgs.clone(),
                received: state.received.clone(),
            });
        });
    });

    let first = first_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("first client result timeout");
    let second = second_rx
        .recv_timeout(Duration::from_secs(10))
        .expect("second client result timeout");
    (first, second)
}

/// 双客户端转发（A → B 方向）：B 先连接（消息被回显），A 后连接，
/// A 的消息经服务器转发给 B，不回显给 A
#[test]
fn p2p_forward_a_to_b() {
    let (_, _, conf_path) = setup_conf();
    // B 先连（注册表仅 B，b"from B" 被回显）；A 后连，b"from A" 被转发给 B
    let (first, second) = run_forward_pair(&conf_path, b"from B".to_vec(), b"from A".to_vec());
    let (b_msgs, b_received) = (first.server_msgs, first.received);
    let (a_msgs, a_received) = (second.server_msgs, second.received);

    // B 首条服务端消息为 welcome
    let welcome: Message = serde_json::from_slice(
        b_msgs.first().expect("B 应至少收到一条服务端下发消息"),
    )
    .expect("B 首条服务端消息应为合法的 Message 帧");
    assert_eq!(
        welcome.payload,
        b"welcome to vauid".to_vec(),
        "B 首条服务端消息应为 welcome"
    );

    // B 第二条服务端消息为 A 的消息被转发过来（Chat 类型 Message 帧）
    let forwarded: Message = serde_json::from_slice(
        b_msgs.get(1).expect("B 应收到转发消息"),
    )
    .expect("B 收到的转发消息应为合法的 Message 帧");
    assert_eq!(forwarded.ty, MsgType::Chat, "转发消息帧类型应为 Chat");
    assert_eq!(
        forwarded.payload,
        b"from A".to_vec(),
        "B 收到的转发消息载荷应为 A 的消息"
    );

    // B 在单会话期间自己的消息被回显（Message 帧 JSON 字节原样返回）
    let expected_echo = serde_json::to_vec(&Message {
        ty: MsgType::Chat,
        seq: 0,
        flags: 0,
        payload: b"from B".to_vec(),
    })
    .expect("serialize expected echo frame");
    assert_eq!(
        b_received,
        Some(expected_echo),
        "B 单会话期间自身消息应被回显"
    );

    // A 只收到 welcome，自己的消息不回流
    assert_eq!(a_msgs.len(), 1, "A 应只收到 welcome（自己的消息不回流）");
    // 存在其他会话时 A 的消息不回显
    assert_eq!(a_received, None, "A 存在其他会话时不应收到回显");
}

/// 双客户端转发（B → A 方向）：A 先连接（消息被回显），B 后连接，
/// B 的消息经服务器转发给 A，不回显给 B（与上一测试互为镜像，两个方向各覆盖一次）
#[test]
fn p2p_forward_b_to_a() {
    let (_, _, conf_path) = setup_conf();
    // A 先连（注册表仅 A，b"from A" 被回显）；B 后连，b"from B" 被转发给 A
    let (first, second) = run_forward_pair(&conf_path, b"from A".to_vec(), b"from B".to_vec());
    let (a_msgs, a_received) = (first.server_msgs, first.received);
    let (b_msgs, b_received) = (second.server_msgs, second.received);

    // A 首条服务端消息为 welcome
    let welcome: Message = serde_json::from_slice(
        a_msgs.first().expect("A 应至少收到一条服务端下发消息"),
    )
    .expect("A 首条服务端消息应为合法的 Message 帧");
    assert_eq!(
        welcome.payload,
        b"welcome to vauid".to_vec(),
        "A 首条服务端消息应为 welcome"
    );

    // A 第二条服务端消息为 B 的消息被转发过来（Chat 类型 Message 帧）
    let forwarded: Message = serde_json::from_slice(
        a_msgs.get(1).expect("A 应收到转发消息"),
    )
    .expect("A 收到的转发消息应为合法的 Message 帧");
    assert_eq!(forwarded.ty, MsgType::Chat, "转发消息帧类型应为 Chat");
    assert_eq!(
        forwarded.payload,
        b"from B".to_vec(),
        "A 收到的转发消息载荷应为 B 的消息"
    );

    // A 在单会话期间自己的消息被回显（Message 帧 JSON 字节原样返回）
    let expected_echo = serde_json::to_vec(&Message {
        ty: MsgType::Chat,
        seq: 0,
        flags: 0,
        payload: b"from A".to_vec(),
    })
    .expect("serialize expected echo frame");
    assert_eq!(
        a_received,
        Some(expected_echo),
        "A 单会话期间自身消息应被回显"
    );

    // B 只收到 welcome，自己的消息不回流
    assert_eq!(b_msgs.len(), 1, "B 应只收到 welcome（自己的消息不回流）");
    // 存在其他会话时 B 的消息不回显
    assert_eq!(b_received, None, "B 存在其他会话时不应收到回显");
}
