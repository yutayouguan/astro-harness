//! 用户生成界面样式的跨层清单与持久化约定。

use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const UI_STYLE_SCHEMA_VERSION: u32 = 1;
pub const UI_STYLE_MAX_MANIFEST_BYTES: u64 = 64 * 1024;

type UiStyleChangeHandler = Arc<dyn Fn() + Send + Sync + 'static>;
static UI_STYLE_CHANGE_HANDLER: OnceLock<RwLock<Option<UiStyleChangeHandler>>> = OnceLock::new();

/// 注册界面样式变更回调；Desktop 用它将 Agent 工具写盘立即转成前端事件。
pub fn set_ui_style_change_handler(handler: UiStyleChangeHandler) {
    if let Ok(mut slot) = UI_STYLE_CHANGE_HANDLER
        .get_or_init(|| RwLock::new(None))
        .write()
    {
        *slot = Some(handler);
    }
}

/// 通知当前进程中的界面样式消费者重新读取 `active.json`。
pub fn notify_ui_style_changed() {
    let handler = UI_STYLE_CHANGE_HANDLER
        .get()
        .and_then(|slot| slot.read().ok())
        .and_then(|slot| slot.clone());
    if let Some(handler) = handler {
        handler();
    }
}

const ALLOWED_COLOR_TOKENS: &[&str] = &[
    "--accent",
    "--accent-2",
    "--assist-fill",
    "--bg",
    "--bg-base",
    "--border",
    "--color-accent",
    "--color-accent-secondary",
    "--color-bg-base",
    "--color-bg-raised",
    "--color-bg-subtle",
    "--color-border",
    "--color-text",
    "--color-text-muted",
    "--color-text-secondary",
    "--composer-bg",
    "--glass-border",
    "--glass-fill",
    "--glass-fill-soft",
    "--glass-hover",
    "--ink",
    "--ink-secondary",
    "--ink-soft",
    "--panel",
    "--shell-bg",
    "--sidebar-bg",
    "--surface",
    "--text",
    "--text-muted",
    "--user-fill",
];

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UiStyleWallpaperFit {
    #[default]
    Cover,
    Contain,
    Stretch,
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum UiStyleIconMotion {
    Smooth,
    Snappy,
    Bouncy,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStyleIcons {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub motion: Option<UiStyleIconMotion>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stroke_width: Option<f32>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct UiStyleTokens {
    #[serde(default)]
    pub light: BTreeMap<String, String>,
    #[serde(default)]
    pub dark: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStyleWallpaper {
    /// 相对于 `~/.astro/ui/style/` 的路径。
    pub path: String,
    #[serde(default)]
    pub fit: UiStyleWallpaperFit,
    #[serde(default = "default_shade")]
    pub shade: u8,
    #[serde(default)]
    pub blur: u8,
    #[serde(default = "default_true")]
    pub adaptive_color: bool,
    /// 以下分析字段由 Desktop 读取时补齐，不写入主题包也可正常工作。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_theme: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accent_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secondary_color: Option<String>,
}

fn default_shade() -> u8 {
    18
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UiStyleManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub revision: String,
    pub updated_at: String,
    #[serde(default)]
    pub tokens: UiStyleTokens,
    #[serde(default)]
    pub icons: UiStyleIcons,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wallpaper: Option<UiStyleWallpaper>,
}

pub fn ui_style_root(base: &Path) -> PathBuf {
    base.join("ui").join("style")
}

pub fn active_ui_style_path(base: &Path) -> PathBuf {
    ui_style_root(base).join("active.json")
}

fn safe_relative_path(path: &str) -> bool {
    let path = Path::new(path);
    !path.as_os_str().is_empty()
        && !path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

fn safe_css_color(value: &str) -> bool {
    let value = value.trim();
    if value.is_empty() || value.len() > 192 || value.chars().any(char::is_control) {
        return false;
    }
    let lower = value.to_ascii_lowercase();
    if [
        ";",
        "{",
        "}",
        "@import",
        "url(",
        "javascript:",
        "expression(",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
    {
        return false;
    }
    value.starts_with('#')
        || [
            "rgb(",
            "rgba(",
            "hsl(",
            "hsla(",
            "oklch(",
            "color-mix(",
            "linear-gradient(",
            "radial-gradient(",
            "var(",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix))
}

fn validate_tokens(tokens: &BTreeMap<String, String>) -> Result<(), String> {
    for (name, value) in tokens {
        if !ALLOWED_COLOR_TOKENS.contains(&name.as_str()) {
            return Err(format!("不允许覆盖界面变量 {name}"));
        }
        if !safe_css_color(value) {
            return Err(format!("界面变量 {name} 不是安全的颜色值"));
        }
    }
    Ok(())
}

impl UiStyleManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != UI_STYLE_SCHEMA_VERSION {
            return Err(format!("不支持的界面样式版本 {}", self.schema_version));
        }
        if self.id.trim().is_empty() || self.id.len() > 80 {
            return Err("界面样式 id 无效".to_string());
        }
        if !self
            .id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err("界面样式 id 只能包含小写字母、数字和连字符".to_string());
        }
        if self.name.trim().is_empty() || self.name.chars().count() > 80 {
            return Err("界面样式名称无效".to_string());
        }
        if self.revision.trim().is_empty() || self.updated_at.trim().is_empty() {
            return Err("界面样式缺少版本信息".to_string());
        }
        validate_tokens(&self.tokens.light)?;
        validate_tokens(&self.tokens.dark)?;
        if let Some(stroke) = self.icons.stroke_width {
            if ![1.0_f32, 2.0, 2.5].contains(&stroke) {
                return Err("图标线宽只能是 1、2 或 2.5".to_string());
            }
        }
        if let Some(wallpaper) = &self.wallpaper {
            if !safe_relative_path(&wallpaper.path) {
                return Err("壁纸必须使用界面样式目录内的相对路径".to_string());
            }
            if wallpaper.shade > 80 || wallpaper.blur > 40 {
                return Err("壁纸遮罩或模糊参数超出安全范围".to_string());
            }
            if wallpaper
                .recommended_theme
                .as_deref()
                .is_some_and(|theme| theme != "light" && theme != "dark")
            {
                return Err("壁纸推荐主题只能是 light 或 dark".to_string());
            }
            for color in [
                wallpaper.accent_color.as_deref(),
                wallpaper.secondary_color.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                if !safe_css_color(color) {
                    return Err("壁纸分析颜色不是安全颜色值".to_string());
                }
            }
        }
        if self.wallpaper.is_none()
            && self.tokens.light.is_empty()
            && self.tokens.dark.is_empty()
            && self.icons == UiStyleIcons::default()
        {
            return Err("界面样式没有任何可应用内容".to_string());
        }
        Ok(())
    }
}

pub fn read_active_ui_style(base: &Path) -> Result<Option<UiStyleManifest>, String> {
    let path = active_ui_style_path(base);
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("无法读取界面样式：{error}")),
    };
    if !metadata.is_file() || metadata.len() > UI_STYLE_MAX_MANIFEST_BYTES {
        return Err("界面样式清单无效或过大".to_string());
    }
    let bytes = fs::read(&path).map_err(|error| format!("无法读取界面样式：{error}"))?;
    let manifest: UiStyleManifest =
        serde_json::from_slice(&bytes).map_err(|error| format!("界面样式格式无效：{error}"))?;
    manifest.validate()?;
    Ok(Some(manifest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> UiStyleManifest {
        UiStyleManifest {
            schema_version: UI_STYLE_SCHEMA_VERSION,
            id: "aurora-night".into(),
            name: "极光夜色".into(),
            revision: "r1".into(),
            updated_at: "2026-09-06T12:00:00Z".into(),
            tokens: UiStyleTokens {
                light: BTreeMap::from([("--color-accent".into(), "#2563eb".into())]),
                dark: BTreeMap::from([("--color-accent".into(), "oklch(72% 0.15 230)".into())]),
            },
            icons: UiStyleIcons::default(),
            wallpaper: Some(UiStyleWallpaper {
                path: "themes/aurora-night/wallpaper.webp".into(),
                fit: UiStyleWallpaperFit::Cover,
                shade: 18,
                blur: 0,
                adaptive_color: true,
                recommended_theme: None,
                accent_color: None,
                secondary_color: None,
            }),
        }
    }

    #[test]
    fn accepts_versioned_safe_manifest() {
        assert!(manifest().validate().is_ok());
    }

    #[test]
    fn rejects_css_injection_and_path_escape() {
        let mut value = manifest();
        value
            .tokens
            .dark
            .insert("--color-bg-base".into(), "url(https://bad)".into());
        assert!(value.validate().is_err());
        let mut value = manifest();
        value.wallpaper.as_mut().unwrap().path = "../secret.png".into();
        assert!(value.validate().is_err());
    }

    #[test]
    fn invalid_active_manifest_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let root = ui_style_root(temp.path());
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("active.json"), br#"{"schemaVersion":99}"#).unwrap();
        assert!(read_active_ui_style(temp.path()).is_err());
    }
}
