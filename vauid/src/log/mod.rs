//! 结构化日志模块。
//!
//! 统一封装 `tracing` / `tracing-subscriber`，负责全局日志初始化与请求链路追踪：
//! - **控制台输出**：人类可读格式 `[level]::[timestamp]::target: message`；
//! - **JSON 文件输出**：按 [`LogConf`] 配置，仅 `output_enabled` 为 `true` 时写入
//!   `output` 路径，不输出到控制台；
//! - **级别控制**：缺省取 [`LogConf::level`]，`RUST_LOG` 环境变量可覆盖；
//! - **请求链路追踪**：`request_id`（UUID v4）串联一次请求跨模块的完整日志链。
//!
//! 使用方式：
//! ```rust,no_run
//! use vauid_shared::conf::LogConf;
//! vauid::log::init_with_conf(&LogConf::default());
//!
//! let span = vauid::log::request_span("handle_signal");
//! let _guard = span.enter(); // 本 span 内所有事件自动携带 request_id 与 name
//! tracing::info!(msg = "handle", "handle signal");
//! ```

use std::{
    fs, io,
    sync::{Arc, Mutex},
};

use tracing::level_filters::LevelFilter;
use tracing::{Event, Subscriber};
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::time::FormatTime;
use tracing_subscriber::fmt::{
    self, FmtContext, FormatEvent, FormatFields, MakeWriter, format::Writer,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use vauid_shared::Result;
use vauid_shared::conf::{LogConf, LogLevel};

/// 控制台日志格式：`[level]::[timestamp]::target: message`
#[derive(Debug, Default)]
struct ConsoleFormat;

impl<S, N> FormatEvent<S, N> for ConsoleFormat
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        write!(writer, "[{}]::[", meta.level())?;
        fmt::time::SystemTime.format_time(&mut writer)?;
        write!(writer, "]::{}: ", meta.target())?;
        ctx.field_format().format_fields(writer.by_ref(), event)?;
        writeln!(writer)
    }
}

/// 按 [`LogConf`] 初始化全局日志订阅器。
///
/// - 控制台：始终输出 `[level]::[timestamp]::target: message`；
/// - 文件：`output_enabled` 为 `true` 时将 JSON 逐行写入 `output` 路径；
/// - 级别：缺省 `LogConf::level`，`RUST_LOG` 环境变量可覆盖。
pub fn init(conf: Option<LogConf>) -> Result<()> {
    let conf = conf.unwrap_or(LogConf::default());
    let filter = EnvFilter::builder()
        .with_default_directive(level_filter(conf.level).into())
        .from_env_lossy();

    // 控制台层：人类可读格式
    let console = fmt::layer().event_format(ConsoleFormat);

    // JSON 文件层：仅 output_enabled 为 true 时启用，不输出到控制台
    let json = if conf.output_enabled {
        if let Some(parent) = conf.output.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let file = fs::File::create(&conf.output)?;
        Some(
            fmt::layer()
                .json()
                // 事件字段平铺到 JSON 顶层，避免多层嵌套、便于查询
                .flatten_event(true)
                .with_writer(SharedFileWriter(Arc::new(Mutex::new(file)))),
        )
    } else {
        None
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(console)
        .with(json)
        .init();
    Ok(())
}

/// `LogLevel` → `LevelFilter` 映射
fn level_filter(level: LogLevel) -> LevelFilter {
    match level {
        LogLevel::Trace => LevelFilter::TRACE,
        LogLevel::Debug => LevelFilter::DEBUG,
        LogLevel::Info => LevelFilter::INFO,
        LogLevel::Warn => LevelFilter::WARN,
        LogLevel::Error => LevelFilter::ERROR,
    }
}

/// 多线程安全的 JSON 文件写入器：适配 tracing fmt layer 的 [`MakeWriter`]。
///
/// `Arc<Mutex<File>>` 不满足 `MakeWriter`（要求 `&W: io::Write`，而 `&Mutex<File>` 不是），
/// 故自定义包装：每次 `make_writer` 克隆 Arc，写入时加锁保证事件间不交错。
#[derive(Clone)]
struct SharedFileWriter(Arc<Mutex<fs::File>>);

impl<'a> MakeWriter<'a> for SharedFileWriter {
    type Writer = SharedFileGuard;

    fn make_writer(&'a self) -> Self::Writer {
        SharedFileGuard(self.0.clone())
    }
}

struct SharedFileGuard(Arc<Mutex<fs::File>>);

impl io::Write for SharedFileGuard {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap().flush()
    }
}

/// 生成一个请求级追踪 ID（UUID v4）。
pub fn request_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 创建带 `request_id` 的请求级 span，用于串联一次请求的完整链路。
///
/// span 名固定为 `request`（便于按类型聚合），业务类型记录在 `name` 字段中：
/// ```rust,no_run
/// let span = vauid::log::request_span("signal");
/// let _guard = span.enter();
/// tracing::info!("handle signal"); // JSON 输出含 "request_id": "xxxx", "name": "signal"
/// ```
pub fn request_span(name: &'static str) -> tracing::Span {
    // 注意：span 名必须是编译期常量（callsite 静态注册），业务名放字段而非 span 名
    tracing::info_span!(parent: None, "request", name = %name, request_id = %request_id())
}

#[cfg(test)]
#[allow(unused_imports)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing() {
        init(None).expect("log init");
        let span = request_span("handle_signal");
        let _guard = span.enter();
        tracing::info!("handle signal");
    }
}
