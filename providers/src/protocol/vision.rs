//! 视觉理解模式（Google Interactions 与 OpenAI 兼容视觉共用）。

use anyhow::Result;

/// 图片理解模式：描述 / 检测框 / 分割。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VisionMode {
    Describe,
    Detect,
    Segment,
}

impl VisionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Describe => "describe",
            Self::Detect => "detect",
            Self::Segment => "segment",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "describe" => Ok(Self::Describe),
            "detect" => Ok(Self::Detect),
            "segment" => Ok(Self::Segment),
            other => anyhow::bail!("无效 mode: {other}（期望 describe|detect|segment）"),
        }
    }
}

impl std::str::FromStr for VisionMode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_modes() {
        assert_eq!(VisionMode::parse("").unwrap(), VisionMode::Describe);
        assert_eq!(VisionMode::parse("detect").unwrap(), VisionMode::Detect);
        assert_eq!(VisionMode::parse("SEGMENT").unwrap(), VisionMode::Segment);
        assert!(VisionMode::parse("nope").is_err());
    }
}
