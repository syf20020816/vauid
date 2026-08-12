use serde::{Deserialize, Serialize};

/// 拥塞控制算法（常规化枚举，与具体 QUIC 实现解耦）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum CcAlgorithm {
    /// CUBIC
    Cubic,
    /// BBR（默认）
    #[default]
    Bbr,
    /// BBRv3（实验性）
    Bbr3,
    /// COPA（实验性）
    Copa,
    /// Dummy（测试用，静态拥塞窗口）
    Dummy,
}

impl CcAlgorithm {
    /// 算法名称字符串
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Cubic => "cubic",
            Self::Bbr => "bbr",
            Self::Bbr3 => "bbr3",
            Self::Copa => "copa",
            Self::Dummy => "dummy",
        }
    }
}
