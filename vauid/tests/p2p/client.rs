//! QUIC P2P 客户端集成测试。
//!
//! 链路：自生成自签名证书 → 启动 P2P 服务器（回显）→ 客户端连接并发消息 →
//! 服务器回显 → 客户端收到后校验内容。
//!
//! tquic 的 `Endpoint` 内部使用 `Rc<RefCell<..>>`（非 `Send`），
//! 服务器与客户端各自在独立线程的 `current_thread` runtime 中独占运行。

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use vauid::service::p2p::client::QuicClient;
use vauid::service::p2p::P2PServer;
use vauid_shared::conf::{ConfRW, QuicConf, TlsConf};

/// 生成自签名证书与 quic 配置文件（写入临时目录），返回 (证书路径, 私钥路径, 配置文件路径)
fn setup_conf() -> (PathBuf, PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!("vauid-p2p-test-{}", std::process::id()));
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

/// 客户端连接 QUIC 服务器，发送消息并校验服务器回显
#[test]
fn p2p_client_echo() {
    let (_, _, conf_path) = setup_conf();
    let msg = b"hello vauid".to_vec();

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
    assert_eq!(received, Some(msg), "echo 内容应原样返回");
}
