mod algorithm;
mod quic;
mod server;
mod tls;

pub use algorithm::CcAlgorithm;
pub use quic::{QuicConf, QUIC_CONF_PATH};
pub use server::*;
pub use tls::TlsConf;

use crate::Result;
use std::{
    fs,
    path::{Path, PathBuf},
};

/// 默认配置目录（相对应用根目录）
pub const QUIC_CONF_DIR: &str = "conf";

/// 配置读写抽象：从 TOML 加载或创建默认配置，并可序列化保存。
///
/// 类型需满足 `Serialize + DeserializeOwned + Default`：`Default` 提供常规默认值，
/// `serde(default)` 保证配置文件中未填写的字段自动回落默认值。
pub trait ConfRW
where
    Self: serde::Serialize + serde::de::DeserializeOwned + Sized + Default,
{
    /// 默认配置文件路径（相对应用根目录）
    const DEFAULT_PATH: &str;

    /// 加载配置。
    ///
    /// - 文件已存在且非空：读取并解析 TOML，未填写的字段使用默认值；
    /// - 文件不存在或为空（0 字节/纯空白，如历史遗留的空文件）：
    ///   使用默认值创建该配置文件并写入，再返回默认配置。
    ///
    /// 无状态关联函数，无需先构造实例：`QuicConf::new("conf/quic.conf.toml")`。
    fn new<P>(path: Option<P>) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.map_or(PathBuf::from(Self::DEFAULT_PATH), |p| {
            p.as_ref().to_path_buf()
        });
        if path.is_file() {
            let content = fs::read_to_string(&path)?;
            // 空文件按缺失处理：重新生成默认配置，避免空文件被静默读为默认值后永不修复
            if !content.trim().is_empty() {
                return Ok(toml::from_str(&content)?);
            }
        }

        let conf = Self::default();
        conf.save(path)?;
        Ok(conf)
    }

    /// 将当前配置序列化为 TOML 并写入指定路径（自动创建父目录）。
    fn save<P>(&self, path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        // 纯文件名（如 "quic.conf.toml"）时 parent 为空串，跳过建目录
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        fs::write(path, content)?;
        Ok(())
    }
}
