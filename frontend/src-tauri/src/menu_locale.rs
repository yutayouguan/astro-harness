//! 原生菜单 / 托盘文案（zh / en），与前端 `astro-locale` 同步。

use serde::Deserialize;

/// 应用界面语言（原生菜单用）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AppLocale {
    #[default]
    Zh,
    En,
}

impl AppLocale {
    /// 解析 `"zh"` / `"en"`（其它值回落中文）。
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "en" => Self::En,
            _ => Self::Zh,
        }
    }

    /// 当前语言的菜单文案。
    pub fn strings(self) -> MenuStrings {
        match self {
            Self::Zh => MenuStrings::zh(),
            Self::En => MenuStrings::en(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_locale() {
        assert_eq!(AppLocale::parse("en"), AppLocale::En);
        assert_eq!(AppLocale::parse("EN"), AppLocale::En);
        assert_eq!(AppLocale::parse("zh"), AppLocale::Zh);
        assert_eq!(AppLocale::parse("zh-CN"), AppLocale::Zh);
        assert_eq!(AppLocale::parse(""), AppLocale::Zh);
    }
}

/// 菜单栏与托盘用到的本地化字符串（`'static`，便于预置项传 `Some`）。
#[derive(Debug, Clone, Copy)]
pub struct MenuStrings {
    pub preferences: &'static str,
    pub submenu_file: &'static str,
    pub submenu_edit: &'static str,
    pub submenu_view: &'static str,
    pub submenu_window: &'static str,
    pub submenu_help: &'static str,
    pub about: &'static str,
    pub services: &'static str,
    pub hide: &'static str,
    pub hide_others: &'static str,
    pub quit: &'static str,
    pub close_window: &'static str,
    pub minimize: &'static str,
    pub maximize: &'static str,
    pub fullscreen: &'static str,
    pub undo: &'static str,
    pub redo: &'static str,
    pub cut: &'static str,
    pub copy: &'static str,
    pub paste: &'static str,
    pub select_all: &'static str,
    pub tray_show: &'static str,
    pub tray_quit: &'static str,
    pub tray_tooltip: &'static str,
}

impl MenuStrings {
    const fn zh() -> Self {
        Self {
            about_title: "关于Astro",
            about_credits: "Astro（阿童木）是本地 AI 桌面工作站，名字取自经典动漫《铁臂阿童木》——希望它像阿童木一样，成为你身边可靠、聪明、敢闯敢干的助手。支持智能对话、记忆召回、工作区与文件空间，可接入多家模型，并调用工具与 Skills 完成复杂任务。偏好设置保存在本机。",
            preferences: "偏好设置...",
            submenu_file: "文件",
            submenu_edit: "编辑",
            submenu_view: "显示",
            submenu_window: "窗口",
            submenu_help: "帮助",
            about: "关于 Astro",
            services: "服务",
            hide: "隐藏 Astro",
            hide_others: "隐藏其他",
            quit: "退出 Astro",
            close_window: "关闭窗口",
            minimize: "最小化",
            maximize: "缩放",
            fullscreen: "进入全屏幕",
            undo: "撤销",
            redo: "重做",
            cut: "剪切",
            copy: "拷贝",
            paste: "粘贴",
            select_all: "全选",
            tray_show: "显示 Astro",
            tray_quit: "退出 Astro",
            tray_tooltip: "Astro Agent",
        }
    }

    const fn en() -> Self {
        Self {
            about_title: "About Astro",
            about_credits: "Astro (阿童木) is a local AI desktop workstation, named after the classic anime Astro Boy — meant to be a reliable, smart, and adventurous helper by your side. Chat, memory, workspace, and filespace in one; connect multiple model providers, and use tools & Skills for complex tasks. Preferences are saved on this device.",
            preferences: "Preferences...",
            submenu_file: "File",
            submenu_edit: "Edit",
            submenu_view: "View",
            submenu_window: "Window",
            submenu_help: "Help",
            about: "About Astro",
            services: "Services",
            hide: "Hide Astro",
            hide_others: "Hide Others",
            quit: "Quit Astro",
            close_window: "Close Window",
            minimize: "Minimize",
            maximize: "Zoom",
            fullscreen: "Enter Full Screen",
            undo: "Undo",
            redo: "Redo",
            cut: "Cut",
            copy: "Copy",
            paste: "Paste",
            select_all: "Select All",
            tray_show: "Show Astro",
            tray_quit: "Quit Astro",
            tray_tooltip: "Astro Agent",
        }
    }
}
