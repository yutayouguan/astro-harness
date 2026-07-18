//! 上下文卫生配置：读写 `config.yaml` 的 `compression:` 段。

use serde::{Deserialize, Serialize};

/// 前端 Preferences 用的上下文压缩设置 DTO。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionSettingsDto {
    pub enabled: bool,
    pub soft_ratio: f32,
    pub medium_ratio: f32,
    pub hard_ratio: f32,
    pub soft_max_chars: usize,
    pub soft_head_chars: usize,
    pub soft_tail_chars: usize,
    pub medium_max_chars: usize,
    pub medium_head_chars: usize,
    pub medium_tail_chars: usize,
    pub hard_max_chars: usize,
    pub hard_head_chars: usize,
    pub hard_tail_chars: usize,
    pub tool_results_limit: usize,
    pub mid_run_summary_ratio: f32,
    pub recommend_compact_ratio: f32,
    pub protect_last_n: usize,
    pub protect_first_messages: usize,
    pub thrashing_min_gain_ratio: f32,
    pub thrashing_max_consecutive: u32,
    pub keep_tail_bubbles: usize,
}

impl From<memory::CompressionConfig> for CompressionSettingsDto {
    fn from(c: memory::CompressionConfig) -> Self {
        Self {
            enabled: c.enabled,
            soft_ratio: c.soft_ratio,
            medium_ratio: c.medium_ratio,
            hard_ratio: c.hard_ratio,
            soft_max_chars: c.soft_max_chars,
            soft_head_chars: c.soft_head_chars,
            soft_tail_chars: c.soft_tail_chars,
            medium_max_chars: c.medium_max_chars,
            medium_head_chars: c.medium_head_chars,
            medium_tail_chars: c.medium_tail_chars,
            hard_max_chars: c.hard_max_chars,
            hard_head_chars: c.hard_head_chars,
            hard_tail_chars: c.hard_tail_chars,
            tool_results_limit: c.tool_results_limit,
            mid_run_summary_ratio: c.mid_run_summary_ratio,
            recommend_compact_ratio: c.recommend_compact_ratio,
            protect_last_n: c.protect_last_n,
            protect_first_messages: c.protect_first_messages,
            thrashing_min_gain_ratio: c.thrashing_min_gain_ratio,
            thrashing_max_consecutive: c.thrashing_max_consecutive,
            keep_tail_bubbles: c.keep_tail_bubbles,
        }
    }
}

impl From<&CompressionSettingsDto> for memory::CompressionConfig {
    fn from(d: &CompressionSettingsDto) -> Self {
        Self {
            enabled: d.enabled,
            soft_ratio: d.soft_ratio,
            medium_ratio: d.medium_ratio,
            hard_ratio: d.hard_ratio,
            soft_max_chars: d.soft_max_chars,
            soft_head_chars: d.soft_head_chars,
            soft_tail_chars: d.soft_tail_chars,
            medium_max_chars: d.medium_max_chars,
            medium_head_chars: d.medium_head_chars,
            medium_tail_chars: d.medium_tail_chars,
            hard_max_chars: d.hard_max_chars,
            hard_head_chars: d.hard_head_chars,
            hard_tail_chars: d.hard_tail_chars,
            tool_results_limit: d.tool_results_limit,
            mid_run_summary_ratio: d.mid_run_summary_ratio,
            recommend_compact_ratio: d.recommend_compact_ratio,
            protect_last_n: d.protect_last_n,
            protect_first_messages: d.protect_first_messages,
            thrashing_min_gain_ratio: d.thrashing_min_gain_ratio,
            thrashing_max_consecutive: d.thrashing_max_consecutive,
            keep_tail_bubbles: d.keep_tail_bubbles,
        }
    }
}

fn clamp_ratio(v: f32, lo: f32, hi: f32) -> f32 {
    v.clamp(lo, hi)
}

/// 校验并规范化压缩配置；非法顺序返回错误。
pub fn normalize_compression_settings(
    mut d: CompressionSettingsDto,
) -> Result<CompressionSettingsDto, String> {
    d.soft_ratio = clamp_ratio(d.soft_ratio, 0.05, 0.95);
    d.medium_ratio = clamp_ratio(d.medium_ratio, 0.05, 0.95);
    d.hard_ratio = clamp_ratio(d.hard_ratio, 0.05, 0.95);
    if !(d.soft_ratio < d.medium_ratio && d.medium_ratio < d.hard_ratio) {
        return Err(format!(
            "阶段比例须满足 Soft < Medium < Hard（当前 {:.0}% / {:.0}% / {:.0}%）",
            d.soft_ratio * 100.0,
            d.medium_ratio * 100.0,
            d.hard_ratio * 100.0
        ));
    }

    d.mid_run_summary_ratio = clamp_ratio(d.mid_run_summary_ratio, 0.05, 0.99);
    d.recommend_compact_ratio = clamp_ratio(d.recommend_compact_ratio, d.hard_ratio, 0.99);
    d.thrashing_min_gain_ratio = clamp_ratio(d.thrashing_min_gain_ratio, 0.01, 0.5);

    fn check_stage(name: &str, max: usize, head: usize, tail: usize) -> Result<(), String> {
        if max < 100 {
            return Err(format!("{name} max_chars 须 ≥ 100"));
        }
        if head + tail > max {
            return Err(format!("{name} 须满足 head + tail ≤ max_chars"));
        }
        Ok(())
    }
    check_stage("Soft", d.soft_max_chars, d.soft_head_chars, d.soft_tail_chars)?;
    check_stage(
        "Medium",
        d.medium_max_chars,
        d.medium_head_chars,
        d.medium_tail_chars,
    )?;
    check_stage("Hard", d.hard_max_chars, d.hard_head_chars, d.hard_tail_chars)?;

    d.tool_results_limit = d.tool_results_limit.min(200);
    d.protect_last_n = d.protect_last_n.clamp(1, 200);
    d.protect_first_messages = d.protect_first_messages.clamp(1, 50);
    d.thrashing_max_consecutive = d.thrashing_max_consecutive.clamp(1, 20);
    d.keep_tail_bubbles = d.keep_tail_bubbles.clamp(1, 50);

    Ok(d)
}

/// 读取上下文卫生设置。
#[tauri::command]
pub async fn get_compression_settings() -> Result<CompressionSettingsDto, String> {
    let root = home::default_memory_dir();
    Ok(memory::load_compression_config(&root).into())
}

/// 整包写入上下文卫生设置。
#[tauri::command]
pub async fn set_compression_settings(
    settings: CompressionSettingsDto,
) -> Result<CompressionSettingsDto, String> {
    let normalized = normalize_compression_settings(settings)?;
    let root = home::default_memory_dir();
    let cfg = memory::CompressionConfig::from(&normalized);
    Ok(memory::set_compression_config(&root, &cfg)
        .map_err(|e| e.to_string())?
        .into())
}

/// 重置为默认上下文卫生设置。
#[tauri::command]
pub async fn reset_compression_settings() -> Result<CompressionSettingsDto, String> {
    let root = home::default_memory_dir();
    Ok(memory::reset_compression_config(&root)
        .map_err(|e| e.to_string())?
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_rejects_unordered_ratios() {
        let mut d = CompressionSettingsDto::from(memory::CompressionConfig::default());
        d.soft_ratio = 0.70;
        d.medium_ratio = 0.50;
        d.hard_ratio = 0.80;
        assert!(normalize_compression_settings(d).is_err());
    }

    #[test]
    fn normalize_clamps_recommend_to_hard() {
        let mut d = CompressionSettingsDto::from(memory::CompressionConfig::default());
        d.hard_ratio = 0.80;
        d.recommend_compact_ratio = 0.50;
        let ok = normalize_compression_settings(d).unwrap();
        assert!((ok.recommend_compact_ratio - 0.80).abs() < 1e-6);
    }

    #[test]
    fn normalize_rejects_head_tail_overflow() {
        let mut d = CompressionSettingsDto::from(memory::CompressionConfig::default());
        d.soft_max_chars = 500;
        d.soft_head_chars = 400;
        d.soft_tail_chars = 200;
        assert!(normalize_compression_settings(d).is_err());
    }
}
