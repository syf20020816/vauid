//! QUIC P2P 互发回路基准测量（弱网测试基线工具）。
//!
//! 链路与 `tests/p2p/client.rs` 的 `p2p_client_echo` 一致（线程模式，tquic
//! `Endpoint` 非 `Send`，服务器与客户端各自在独立线程的 current_thread
//! runtime 中独占运行）：服务器线程 `P2PServer::bind_with_conf` 绑定随机
//! 端口并经 mpsc 上报地址；客户端连接后执行 N 轮回显往返。
//!
//! 与 `QuicClient` 的取舍：`QuicClient::on_conn_established` 仅在握手完成
//! 时一次性发出 connect 传入的消息，无法支撑多轮发送；本示例在文件内
//! 自包含实现一个简易循环客户端（复制 `service/p2p/client.rs` 的
//! `QuicSocket` + `Endpoint` + 自定义 handler 模式），关键差异：
//! - 单连接多轮：握手仅一次，耗时单独统计并展示，不计入 RTT；
//! - 每轮新开一条双向流并以 fin 收尾：服务器按 fin 原样回显，客户端以
//!   fin 判定帧完整（帧可能跨多个 QUIC 包分片到达，逐片累计）；
//! - 严格请求-响应串行：收到上一轮回显才发出下一轮，RTT 语义纯净。
//!
//! 载荷经 `serde_json` 序列化（`Vec<u8>` 展开为 JSON 数字数组，帧大小
//! 约为 payload 的 3~4 倍），线路吞吐按 JSON 帧字节数计算。
//!
//! 用法：`cargo run --release --example p2p_bench -- [--rounds N] [--payload N]`

use std::cell::RefCell;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use tquic::{Connection, Endpoint, PacketInfo, TransportHandler};
use vauid::core::quic::socket::{QuicSocket, RecvFrom};
use vauid::core::quic::wrap::conf::QuicConfig;
use vauid::service::p2p::P2PServer;
use vauid_shared::conf::{ConfRW, QuicConf, TlsConf};
use vauid_shared::proto::{Message, MsgType};

/// 每轮载荷上限（字节）。JSON 封装约 4 倍膨胀，需低于服务端单流
/// 流控窗口（`initial_max_stream_data_bidi_remote` 默认 2 MiB）
const MAX_PAYLOAD: usize = 256 * 1024;
/// 服务器就绪等待上限
const SERVER_READY_TIMEOUT: Duration = Duration::from_secs(5);
/// 客户端单次测量总超时 = BASE + 每轮预留（弱网重传下逐轮可能秒级）
const RUN_TIMEOUT_BASE: Duration = Duration::from_secs(30);
const RUN_TIMEOUT_PER_ROUND: Duration = Duration::from_secs(2);

/// 命令行参数
struct Args {
    /// 测量轮数
    rounds: usize,
    /// 每轮载荷字节数
    payload: usize,
}

fn usage() {
    println!(
        "用法: p2p_bench [--rounds N] [--payload N]\n\
         \n\
         选项:\n\
           --rounds N    测量轮数（默认 100）\n\
           --payload N   每轮载荷字节数（默认 1024，上限 {MAX_PAYLOAD}）\n\
           -h, --help    显示本帮助"
    );
}

/// 手写参数解析：支持 `--key value` 与 `--key=value` 两种形式
fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        rounds: 100,
        payload: 1024,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let (key, inline_val) = match arg.split_once('=') {
            Some((k, v)) => (k.to_string(), Some(v.to_string())),
            None => (arg.clone(), None),
        };
        match key.as_str() {
            "-h" | "--help" => {
                usage();
                std::process::exit(0);
            }
            "--rounds" | "--payload" => {
                let val = match inline_val {
                    Some(v) => v,
                    None => it
                        .next()
                        .ok_or_else(|| format!("{key} 需要一个参数"))?,
                };
                let v: usize = val
                    .parse()
                    .map_err(|_| format!("{key} 需要非负整数，得到 {val:?}"))?;
                if key == "--rounds" {
                    args.rounds = v;
                } else {
                    args.payload = v;
                }
            }
            other => return Err(format!("未知参数: {other}（试试 --help）")),
        }
    }
    if args.rounds == 0 {
        return Err("--rounds 至少为 1".to_string());
    }
    if args.payload > MAX_PAYLOAD {
        return Err(format!(
            "--payload 上限为 {MAX_PAYLOAD} 字节（JSON 封装约 4 倍膨胀，需低于服务端单流流控窗口）"
        ));
    }
    Ok(args)
}

/// 生成自签名证书与 quic 配置文件（写入临时目录），返回配置文件路径。
/// 复制自 tests/p2p/client.rs 的 `setup_conf`（示例进程单次使用，无需序号区分）。
fn setup_conf() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("vauid-p2p-bench-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("create temp dir");

    // 自签名证书（客户端 verify=false，无需 CA 信任链）
    let key_pair = rcgen::KeyPair::generate().expect("generate key pair");
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
    let cert = params.self_signed(&key_pair).expect("self-signed cert");

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
    conf.save(&conf_path).expect("save quic conf");
    conf_path
}

/// 客户端会话状态：handler 写入，事件循环据此判断全部轮次是否完成
struct BenchState {
    /// 连接是否已建立（握手完成）
    established: bool,
    /// 握手耗时（connect 发起 → on_conn_established）
    handshake: Option<Duration>,
    /// connect 发起时刻（握手计时起点）
    connect_at: Instant,
    /// 当前进行中的轮次（0-based）
    cur_round: usize,
    /// 当前轮次使用的双向流 id
    cur_stream: Option<u64>,
    /// 当前轮次已累计的回显字节
    echo_buf: Vec<u8>,
    /// 当前轮次写出时刻
    sent_at: Option<Instant>,
    /// 各轮 RTT（写出 → 回显收齐）
    rtts: Vec<Duration>,
    /// 首轮写出时刻
    bench_start: Option<Instant>,
    /// 末轮回显到达时刻
    bench_end: Option<Instant>,
    /// 全部轮次完成
    done: bool,
    /// 致命错误（帧不匹配 / 流写入失败等）
    error: Option<String>,
}

impl BenchState {
    fn new() -> Self {
        Self {
            established: false,
            handshake: None,
            connect_at: Instant::now(),
            cur_round: 0,
            cur_stream: None,
            echo_buf: Vec::new(),
            sent_at: None,
            rtts: Vec::new(),
            bench_start: None,
            bench_end: None,
            done: false,
            error: None,
        }
    }
}

/// 在 `round` 轮新开双向流写出预序列化帧（fin 收尾），并刷新计时状态。
/// 在 handler 回调（握手完成 / 上一轮回显）内调用。
fn start_round(
    conn: &mut Connection,
    st: &mut BenchState,
    frames: &[Vec<u8>],
    round: usize,
    now: Instant,
) -> Result<(), String> {
    let stream_id = conn
        .stream_bidi_new(0, false)
        .map_err(|e| format!("第 {} 轮打开双向流失败: {e}", round + 1))?;
    let frame = &frames[round];
    let written = conn
        .stream_write(stream_id, Bytes::copy_from_slice(frame), true)
        .map_err(|e| format!("第 {} 轮写出失败: {e}", round + 1))?;
    if written != frame.len() {
        return Err(format!(
            "第 {} 轮流写入不完整: {written}/{} 字节",
            round + 1,
            frame.len()
        ));
    }
    st.cur_round = round;
    st.cur_stream = Some(stream_id);
    st.echo_buf.clear();
    st.sent_at = Some(now);
    if st.bench_start.is_none() {
        st.bench_start = Some(now);
    }
    Ok(())
}

/// 基准客户端传输事件处理器：
/// - 握手完成：记录握手耗时，发出第 0 轮；
/// - 客户端发起流可读：累计回显分片，fin 判定完整后校验内容、记录 RTT，
///   随即发出下一轮（写在同一 `process_connections` 内被冲洗发出）；
/// - 服务端发起流可读（welcome 等）：读后丢弃，与测量无关。
struct BenchHandler {
    state: Rc<RefCell<BenchState>>,
    /// 预序列化的各轮 Message 帧（与轮次一一对应，序列化开销不计入 RTT）
    frames: Vec<Vec<u8>>,
}

impl TransportHandler for BenchHandler {
    fn on_conn_created(&mut self, _conn: &mut Connection) {}

    fn on_conn_established(&mut self, conn: &mut Connection) {
        eprintln!("[DBG] conn established");
        let now = Instant::now();
        let mut st = self.state.borrow_mut();
        st.established = true;
        st.handshake = Some(now - st.connect_at);
        match start_round(conn, &mut st, &self.frames, 0, now) {
            Ok(()) => eprintln!("[DBG] round 0 written on stream {:?}", st.cur_stream),
            Err(e) => {
                eprintln!("[DBG] round 0 failed: {e}");
                st.error = Some(e);
                st.done = true;
            }
        }
    }

    fn on_conn_closed(&mut self, _conn: &mut Connection) {}

    fn on_stream_created(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_readable(&mut self, conn: &mut Connection, stream_id: u64) {
        // 读取该流当前全部可读数据
        let mut data = Vec::new();
        let mut fin = false;
        loop {
            let mut buf = [0u8; 4096];
            let (n, f) = match conn.stream_read(stream_id, &mut buf) {
                Ok(v) => v,
                // 暂无更多数据（Done）或流已错误：结束本轮
                Err(_) => break,
            };
            if n > 0 {
                data.extend_from_slice(&buf[..n]);
            }
            fin = f;
            if f {
                break;
            }
        }

        let mut st = self.state.borrow_mut();
        if stream_id & 3 == 1 {
            // 服务端发起的下行流（welcome）：读后丢弃
            eprintln!("[DBG] srv-stream {stream_id} readable: {} bytes fin={fin}", data.len());
            return;
        }
        // 客户端发起流：仅处理当前轮次（旧流残余事件忽略）
        if st.done || st.error.is_some() || Some(stream_id) != st.cur_stream {
            eprintln!("[DBG] echo stream {stream_id} ignored (cur={:?} done={} err={:?})", st.cur_stream, st.done, st.error);
            return;
        }
        eprintln!("[DBG] round {} echo chunk: {} bytes fin={fin} (accum {}/{})", st.cur_round + 1, data.len(), st.echo_buf.len() + data.len(), self.frames[st.cur_round].len());
        st.echo_buf.extend_from_slice(&data);
        let expected = &self.frames[st.cur_round];
        if !fin && st.echo_buf.len() < expected.len() {
            // 帧跨多个 QUIC 包分片到达，继续累计
            return;
        }
        if st.echo_buf.as_slice() != expected.as_slice() {
            st.error = Some(format!(
                "第 {} 轮回显内容与发送帧不一致（收到 {} 字节，期望 {} 字节）",
                st.cur_round + 1,
                st.echo_buf.len(),
                expected.len()
            ));
            st.done = true;
            return;
        }
        // 回显收齐：记录 RTT 并发出下一轮
        let now = Instant::now();
        if let Some(sent_at) = st.sent_at {
            st.rtts.push(now - sent_at);
        }
        st.bench_end = Some(now);
        let next = st.cur_round + 1;
        if next >= self.frames.len() {
            st.done = true;
            return;
        }
        if let Err(e) = start_round(conn, &mut st, &self.frames, next, now) {
            st.error = Some(e);
            st.done = true;
        }
    }

    fn on_stream_writable(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_stream_closed(&mut self, _conn: &mut Connection, _stream_id: u64) {}

    fn on_new_token(&mut self, _conn: &mut Connection, _token: Vec<u8>) {}
}

/// 客户端测量结果（Send，跨线程回传主线程统计打印）
struct BenchOutcome {
    /// 握手耗时（毫秒）
    handshake_ms: Option<f64>,
    /// 各轮 RTT（毫秒）
    rtts_ms: Vec<f64>,
    /// 基准总耗时（毫秒，首轮写出 → 末轮回显）
    elapsed_ms: Option<f64>,
    /// 各轮 JSON 帧字节数之和（单方向线路字节）
    wire_bytes: u64,
    /// 轮数 × payload 字节数（单方向应用载荷字节）
    payload_bytes: u64,
}

/// 单次基准测量（客户端线程内运行）：
/// 连接 → 逐轮回显 → 全部轮次完成后返回统计。
async fn bench_once(
    server_addr: SocketAddr,
    conf_path: &PathBuf,
    rounds: usize,
    payload: usize,
) -> Result<BenchOutcome, String> {
    // 预序列化各轮 Message 帧（严格请求-响应，逐轮 seq 递增）
    let payload_bytes: Vec<u8> = (0..payload).map(|i| (i % 251) as u8).collect();
    let mut frames: Vec<Vec<u8>> = Vec::with_capacity(rounds);
    for i in 0..rounds {
        let msg = Message {
            ty: MsgType::Chat,
            seq: i as u64,
            flags: 0,
            payload: payload_bytes.clone(),
        };
        frames.push(serde_json::to_vec(&msg).map_err(|e| format!("序列化失败: {e}"))?);
    }
    let wire_bytes: u64 = frames.iter().map(|f| f.len() as u64).sum();

    let config = QuicConfig::client(conf_path)
        .map_err(|e| format!("加载客户端配置失败: {e}"))?
        .into_inner();
    let socket = Rc::new(
        QuicSocket::new_client_socket(true)
            .await
            .map_err(|e| format!("创建客户端 socket 失败: {e}"))?,
    );

    let state = Rc::new(RefCell::new(BenchState::new()));
    let handler = Box::new(BenchHandler {
        state: state.clone(),
        frames,
    });
    let mut endpoint = Endpoint::new(Box::new(config), false, handler, socket.clone());
    // 发起握手（Initial 在 process_connections 中真正发出）
    endpoint
        .connect(socket.local_addr, server_addr, None, None, None, None)
        .map_err(|e| format!("发起连接失败: {e}"))?;
    endpoint
        .process_connections()
        .map_err(|e| format!("驱动连接失败: {e}"))?;

    // 事件循环：收包 → 喂给 QUIC 栈 → 驱动连接事件，直至全部轮次完成
    let mut recv_buf = vec![0u8; 64 * 1024];
    loop {
        {
            let st = state.borrow();
            if st.done {
                if let Some(e) = &st.error {
                    return Err(e.clone());
                }
                break;
            }
        }
        let RecvFrom { len, local, remote, .. } = socket
            .recv_from(&mut recv_buf, socket.local_addr)
            .await
            .map_err(|e| format!("接收失败: {e}"))?;
        let info = PacketInfo {
            src: remote,
            dst: local,
            time: Instant::now(),
        };
        endpoint
            .recv(&mut recv_buf[..len], &info)
            .map_err(|e| format!("QUIC 接收失败: {e}"))?;
        endpoint
            .process_connections()
            .map_err(|e| format!("QUIC 处理失败: {e}"))?;
    }

    let st = state.borrow();
    Ok(BenchOutcome {
        handshake_ms: st.handshake.map(|d| d.as_secs_f64() * 1000.0),
        rtts_ms: st.rtts.iter().map(|d| d.as_secs_f64() * 1000.0).collect(),
        elapsed_ms: st
            .bench_end
            .zip(st.bench_start)
            .map(|(end, start)| (end - start).as_secs_f64() * 1000.0),
        wire_bytes,
        payload_bytes: (rounds * payload) as u64,
    })
}

/// 就近秩（nearest-rank）分位数：p ∈ (0, 100]，返回排序后样本的对应秩
fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 0 {
        return f64::NAN;
    }
    let rank = (p * n as f64 / 100.0).ceil() as usize;
    let idx = rank.saturating_sub(1).min(n - 1);
    sorted[idx]
}

/// 打印人类可读统计与脚本采集用单行 SUMMARY
fn print_report(args: &Args, outcome: &BenchOutcome) {
    let mut sorted = outcome.rtts_ms.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let ms = |v: f64| format!("{v:.3}");
    let rtt_min = sorted.first().copied().unwrap_or(f64::NAN);
    let rtt_max = sorted.last().copied().unwrap_or(f64::NAN);
    let rtt_avg = if sorted.is_empty() {
        f64::NAN
    } else {
        outcome.rtts_ms.iter().sum::<f64>() / sorted.len() as f64
    };
    let rtt_p50 = percentile(&sorted, 50.0);
    let rtt_p95 = percentile(&sorted, 95.0);
    let elapsed = outcome.elapsed_ms.unwrap_or(f64::NAN);
    let per_round_frame = if args.rounds > 0 {
        outcome.wire_bytes as f64 / args.rounds as f64
    } else {
        0.0
    };
    // 吞吐：单方向应用载荷 / 线路 JSON 帧字节 ÷ 基准总耗时（严格串行请求-响应）
    let secs = elapsed / 1000.0;
    let payload_bps = if secs > 0.0 {
        outcome.payload_bytes as f64 / secs
    } else {
        f64::NAN
    };
    let wire_bps = if secs > 0.0 {
        outcome.wire_bytes as f64 / secs
    } else {
        f64::NAN
    };

    println!("================ QUIC P2P 回显基准 ================");
    println!("轮数              : {}", args.rounds);
    println!("payload 大小      : {} 字节", args.payload);
    println!(
        "JSON 帧大小(线路) : {:.0} 字节/轮（Vec<u8> 序列化为数字数组，约 {:.1} 倍膨胀）",
        per_round_frame,
        if args.payload > 0 {
            per_round_frame / args.payload as f64
        } else {
            0.0
        }
    );
    println!(
        "握手耗时          : {} ms（单连接一次握手，不计入 RTT）",
        outcome
            .handshake_ms
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!("---------------------------------------------------");
    println!("RTT min / avg     : {} / {} ms", ms(rtt_min), ms(rtt_avg));
    println!(
        "RTT p50 / p95     : {} / {} ms",
        ms(rtt_p50),
        ms(rtt_p95)
    );
    println!("RTT max           : {} ms", ms(rtt_max));
    println!("---------------------------------------------------");
    println!(
        "基准总耗时        : {} ms（首轮写出 → 末轮回显，严格串行请求-响应）",
        ms(elapsed)
    );
    println!("吞吐(payload)     : {:.0} 字节/秒", payload_bps);
    println!("吞吐(线路 JSON 帧): {:.0} 字节/秒", wire_bps);
    println!("===================================================");
    // 单行摘要：供 scripts/netem_baseline.sh grep 采集
    println!(
        "SUMMARY rounds={} payload={} frame_bytes={} handshake_ms={} rtt_min_ms={} rtt_avg_ms={} rtt_p50_ms={} rtt_p95_ms={} rtt_max_ms={} elapsed_ms={} payload_bytes_per_s={:.0} wire_bytes_per_s={:.0}",
        args.rounds,
        args.payload,
        outcome.wire_bytes,
        outcome
            .handshake_ms
            .map(|v| format!("{v:.3}"))
            .unwrap_or_else(|| "nan".to_string()),
        ms(rtt_min),
        ms(rtt_avg),
        ms(rtt_p50),
        ms(rtt_p95),
        ms(rtt_max),
        ms(elapsed),
        payload_bps,
        wire_bps,
    );
}

fn main() {
    let args = match parse_args() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("错误: {e}");
            usage();
            std::process::exit(2);
        }
    };
    let conf_path = setup_conf();

    // 服务器线程：绑定随机端口后上报地址（或错误），随后进入事件循环。
    // 与集成测试一致：服务器线程随进程退出回收（main 返回即退出）。
    let (addr_tx, addr_rx) = mpsc::channel();
    let srv_conf = conf_path.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build server runtime");
        rt.block_on(async move {
            let listen: SocketAddr = "127.0.0.1:0".parse().unwrap();
            match P2PServer::bind_with_conf(listen, &srv_conf).await {
                Ok(mut server) => {
                    let _ = addr_tx.send(Ok(server.socket.local_addr));
                    // 事件循环永不返回；进程退出时线程随之结束
                    let _ = server.run().await;
                }
                Err(e) => {
                    let _ = addr_tx.send(Err(format!("服务器 bind 失败: {e}")));
                }
            }
        });
    });
    let server_addr: SocketAddr = match addr_rx.recv_timeout(SERVER_READY_TIMEOUT) {
        Ok(Ok(addr)) => addr,
        Ok(Err(e)) => {
            eprintln!("错误: {e}");
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("错误: 等待服务器就绪超时（{SERVER_READY_TIMEOUT:?}）");
            std::process::exit(1);
        }
    };

    // 客户端线程：current_thread runtime 内完成连接与 N 轮回显测量
    let (result_tx, result_rx) = mpsc::channel();
    let cli_conf = conf_path.clone();
    let rounds = args.rounds;
    let payload = args.payload;
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build client runtime");
        let timeout = RUN_TIMEOUT_BASE + RUN_TIMEOUT_PER_ROUND * rounds as u32;
        let outcome = rt.block_on(async move {
            match tokio::time::timeout(
                timeout,
                bench_once(server_addr, &cli_conf, rounds, payload),
            )
            .await
            {
                Ok(r) => r,
                Err(_) => Err(format!(
                    "总超时（{}s）：{} 轮未能完成，可能丢包率过高或网络异常",
                    timeout.as_secs(),
                    rounds
                )),
            }
        });
        let _ = result_tx.send(outcome);
    });

    match result_rx.recv_timeout(SERVER_READY_TIMEOUT + RUN_TIMEOUT_BASE + RUN_TIMEOUT_PER_ROUND * rounds as u32) {
        Ok(Ok(outcome)) => print_report(&args, &outcome),
        Ok(Err(e)) => {
            eprintln!("错误: {e}");
            let _ = std::fs::remove_dir_all(conf_path.parent().unwrap_or(std::path::Path::new("")));
            std::process::exit(1);
        }
        Err(_) => {
            eprintln!("错误: 等待客户端测量结果超时");
            std::process::exit(1);
        }
    }

    // 清理临时证书与配置目录（best-effort）
    let _ = std::fs::remove_dir_all(conf_path.parent().unwrap_or(std::path::Path::new("")));
}
