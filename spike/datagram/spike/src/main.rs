//! DATAGRAM extension (RFC 9221) round-trip spike demo.
//!
//! Topology: server + client run on two OS threads, each driving a tquic
//! `Endpoint` with a synchronous loop over a `std::net::UdpSocket`.
//! TLS uses an rcgen-generated self-signed certificate written to a temp dir
//! (same pattern as the main project's `vauid/tests/p2p/client.rs`).
//!
//! Flow:
//!   1. client completes the handshake;
//!   2. client `datagram_send()`s N messages of various sizes (64B / 1200B);
//!   3. server echoes each received datagram back via `datagram_send()`;
//!   4. client validates that every echo matches what was sent.
//!
//! Success is signalled by printing `ROUND-TRIP OK`.

use std::cell::RefCell;
use std::net::{SocketAddr, UdpSocket as StdUdpSocket};
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tquic::{Config, Connection, Endpoint, PacketInfo, TlsConfig, TransportHandler};

const ALPN: &str = "vauid-datagram-spike";
const MAX_DATAGRAM_FRAME_SIZE: u64 = 65527;

/// Sizes of the datagram payloads sent by the client (bytes).
const SEND_SIZES: &[usize] = &[1, 64, 1200];

/// Overall deadline for the demo (handshake + PMTU discovery + echo).
const DEADLINE: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------- sockets --

/// A thin synchronous wrapper implementing tquic's send callback.
struct SyncUdpSocket {
    socket: StdUdpSocket,
    local_addr: SocketAddr,
}

impl SyncUdpSocket {
    fn bind(addr: &str) -> Rc<Self> {
        let socket = StdUdpSocket::bind(addr).expect("bind udp socket");
        socket
            .set_read_timeout(Some(Duration::from_millis(10)))
            .expect("set read timeout");
        let local_addr = socket.local_addr().expect("local addr");
        Rc::new(Self {
            socket,
            local_addr,
        })
    }
}

impl tquic::PacketSendHandler for SyncUdpSocket {
    fn on_packets_send(&self, pkts: &[(Vec<u8>, PacketInfo)]) -> tquic::Result<usize> {
        let mut sent = 0;
        for (pkt, info) in pkts {
            match self.socket.send_to(pkt, info.dst) {
                Ok(_) => sent += 1,
                Err(_) => break,
            }
        }
        Ok(sent)
    }
}

// --------------------------------------------------------------- handlers --

/// Shared state observed by the demo threads.
#[derive(Default)]
struct Shared {
    established: bool,
    /// (size, data) of every echo received by the client, in arrival order.
    echoed: Vec<(usize, Vec<u8>)>,
    closed: bool,
}

struct ServerHandler {
    shared: Rc<RefCell<Shared>>,
}

impl TransportHandler for ServerHandler {
    fn on_conn_created(&mut self, conn: &mut Connection) {
        println!("[server] connection created (trace {})", conn.trace_id());
    }

    fn on_conn_established(&mut self, _conn: &mut Connection) {
        println!("[server] connection established");
        self.shared.borrow_mut().established = true;
    }

    fn on_conn_closed(&mut self, _conn: &mut Connection) {
        println!("[server] connection closed");
        self.shared.borrow_mut().closed = true;
    }

    fn on_datagram_received(&mut self, conn: &mut Connection) {
        // Echo every received datagram back to the peer.
        while let Ok(Some(data)) = conn.datagram_recv() {
            println!(
                "[server] datagram received ({} bytes), echoing back",
                data.len()
            );
            if let Err(e) = conn.datagram_send(data) {
                println!("[server] failed to echo datagram: {:?}", e);
            }
        }
    }

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_readable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

struct ClientHandler {
    shared: Rc<RefCell<Shared>>,
    /// Payloads to send once the handshake completes.
    payloads: Vec<Vec<u8>>,
    sent: bool,
}

impl TransportHandler for ClientHandler {
    fn on_conn_created(&mut self, conn: &mut Connection) {
        println!("[client] connection created (trace {})", conn.trace_id());
    }

    fn on_conn_established(&mut self, conn: &mut Connection) {
        println!("[client] connection established, sending datagrams");
        self.shared.borrow_mut().established = true;

        for payload in &self.payloads {
            match conn.datagram_send(Bytes::from(payload.clone())) {
                Ok(()) => println!(
                    "[client] datagram queued ({} bytes, send queue len {})",
                    payload.len(),
                    conn.datagram_send_queue_len()
                ),
                Err(e) => println!("[client] datagram_send failed: {:?}", e),
            }
        }
        self.sent = true;
    }

    fn on_conn_closed(&mut self, _conn: &mut Connection) {
        println!("[client] connection closed");
        self.shared.borrow_mut().closed = true;
    }

    fn on_datagram_received(&mut self, conn: &mut Connection) {
        while let Ok(Some(data)) = conn.datagram_recv() {
            println!(
                "[client] echo received ({} bytes, recv queue len {})",
                data.len(),
                conn.datagram_recv_queue_len()
            );
            self.shared.borrow_mut().echoed.push((data.len(), data.to_vec()));
        }
    }

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_readable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}
    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

// ------------------------------------------------------------------ drive --

/// Drive one endpoint: receive packets, process timeouts and connections.
fn drive_endpoint(endpoint: &mut Endpoint, sock: &SyncUdpSocket, buf: &mut [u8]) {
    match sock.socket.recv_from(buf) {
        Ok((n, src)) => {
            let info = PacketInfo {
                src,
                dst: sock.local_addr,
                time: Instant::now(),
            };
            let _ = endpoint.recv(&mut buf[..n], &info);
        }
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(e) => println!("[warn] udp recv error: {:?}", e),
    }

    let now = Instant::now();
    endpoint.on_timeout(now);
    if let Err(e) = endpoint.process_connections() {
        println!("[warn] process_connections: {:?}", e);
    }
}

fn server_thread(addr_tx: mpsc::Sender<SocketAddr>, cert_path: String, key_path: String) {
    let sock = SyncUdpSocket::bind("127.0.0.1:0");
    let _ = addr_tx.send(sock.local_addr);

    let shared = Rc::new(RefCell::new(Shared::default()));

    let mut tls_config =
        TlsConfig::new_server_config(&cert_path, &key_path, vec![ALPN.into()], false).unwrap();
    tls_config.set_ticket_key(&vec![0x73; 48]).unwrap();
    let mut config = Config::new().unwrap();
    config.set_tls_config(tls_config);
    // SPIKE_NO_TP=1 keeps the server from advertising the DATAGRAM extension,
    // for the negative negotiation test.
    if std::env::var("SPIKE_NO_TP").is_err() {
        config.set_max_datagram_frame_size(MAX_DATAGRAM_FRAME_SIZE);
    }

    let mut endpoint = Endpoint::new(
        Box::new(config),
        true,
        Box::new(ServerHandler {
            shared: shared.clone(),
        }),
        sock.clone(),
    );

    let mut buf = vec![0u8; 65535];
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        drive_endpoint(&mut endpoint, &sock, &mut buf);
        if shared.borrow().closed {
            // Let the client observe the close before tearing down.
            for _ in 0..50 {
                drive_endpoint(&mut endpoint, &sock, &mut buf);
            }
            break;
        }
    }
    println!("[server] loop done");
}

fn client_thread(server_addr: SocketAddr, cert_path: String) -> Vec<(usize, Vec<u8>)> {
    let sock = SyncUdpSocket::bind("127.0.0.1:0");

    // Deterministic payloads: byte i of each payload is (size ^ i) & 0xff.
    let payloads: Vec<Vec<u8>> = SEND_SIZES
        .iter()
        .map(|&size| (0..size).map(|i| (size ^ i) as u8).collect())
        .collect();

    let shared = Rc::new(RefCell::new(Shared::default()));

    let tls_config = TlsConfig::new_client_config(vec![ALPN.into()], false).unwrap();
    let mut config = Config::new().unwrap();
    config.set_tls_config(tls_config);
    config.set_max_datagram_frame_size(MAX_DATAGRAM_FRAME_SIZE);

    let handler = ClientHandler {
        shared: shared.clone(),
        payloads,
        sent: false,
    };
    let payload_count = SEND_SIZES.len();

    let mut endpoint = Endpoint::new(
        Box::new(config),
        false,
        Box::new(handler),
        sock.clone(),
    );

    endpoint
        .connect(sock.local_addr, server_addr, None, None, None, None)
        .expect("connect");

    let mut buf = vec![0u8; 65535];
    let deadline = Instant::now() + DEADLINE;
    while Instant::now() < deadline {
        drive_endpoint(&mut endpoint, &sock, &mut buf);
        if shared.borrow().echoed.len() >= payload_count {
            // Let ACKs drain, then close gracefully.
            for _ in 0..100 {
                drive_endpoint(&mut endpoint, &sock, &mut buf);
            }
            break;
        }
        if shared.borrow().closed {
            break;
        }
    }

    println!("[client] loop done, {} echoes collected", shared.borrow().echoed.len());
    let result = shared.borrow().echoed.clone();
    // Silence unused warnings for variables kept for clarity.
    let _ = cert_path;
    result
}

fn main() {
    // Generate a self-signed certificate into a temp dir.
    let dir = std::env::temp_dir().join(format!("tquic-datagram-spike-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let key_pair = rcgen::KeyPair::generate().unwrap();
    let mut params = rcgen::CertificateParams::default();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "vauid".to_string());
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName(
            rcgen::Ia5String::try_from("localhost".to_string()).unwrap(),
        ),
        rcgen::SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST)),
    ];
    let cert = params.self_signed(&key_pair).unwrap();
    let cert_path = dir.join("cert.pem");
    let key_path = dir.join("key.pem");
    std::fs::write(&cert_path, cert.pem()).unwrap();
    std::fs::write(&key_path, key_pair.serialize_pem()).unwrap();

    println!("== DATAGRAM (RFC 9221) spike: tquic 1.6.0 + patch ==");
    println!("payload sizes: {:?} bytes", SEND_SIZES);

    // Start the server thread and wait for its listen address.
    let (addr_tx, addr_rx) = mpsc::channel();
    let server_cert = cert_path.to_string_lossy().into_owned();
    let server_key = key_path.to_string_lossy().into_owned();
    let server = std::thread::spawn(move || server_thread(addr_tx, server_cert, server_key));
    let server_addr = addr_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("server did not start");
    println!("server listening on {}", server_addr);

    // Run the client and collect the echoes.
    let client_cert = cert_path.to_string_lossy().into_owned();
    let client = std::thread::spawn(move || client_thread(server_addr, client_cert));
    let echoed = client.join().expect("client thread panicked");
    let _ = server.join();

    // Validate: every sent payload must be echoed back byte-for-byte.
    let expected: Vec<Vec<u8>> = SEND_SIZES
        .iter()
        .map(|&size| (0..size).map(|i| (size ^ i) as u8).collect())
        .collect();
    let mut failures = Vec::new();
    for exp in &expected {
        let found = echoed
            .iter()
            .any(|(_, data)| data == &**exp);
        if !found {
            failures.push(exp.len());
        }
    }

    println!("----");
    println!("echoed datagrams: {:?}", echoed.iter().map(|(n, _)| *n).collect::<Vec<_>>());
    if failures.is_empty() {
        println!("ROUND-TRIP OK ({} of {} datagrams matched)", echoed.len(), expected.len());
        let _ = std::fs::remove_dir_all(&dir);
        std::process::exit(0);
    } else {
        println!(
            "ROUND-TRIP FAILED: missing echoes for sizes {:?}",
            failures
        );
        let _ = std::fs::remove_dir_all(&dir);
        std::process::exit(1);
    }
}
