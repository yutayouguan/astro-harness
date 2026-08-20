//! 危险命令检测与分级审批（Hermes 风格）。
//!
//! 危险命令分级与 regex 审批规则。

use regex::Regex;
use std::sync::OnceLock;

pub use types::approval::{ApprovalAction, ApprovalDecision, ApprovalMode};

/// 极危：直接 deny。
fn deny_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        compile(&[
            (r"(?i)\bmkfs\b", "filesystem format (mkfs)"),
            (r"(?i)\bmkfs\.|\bwipefs\b", "disk wipe/format"),
            (r"(?i)\bdd\s+.*\bof\s*=\s*/dev/", "dd write to block device"),
            (r"(?i)>\s*/etc/", "overwrite under /etc"),
            (r":\(\)\s*\{\s*:\|:&\s*\}\s*;?", "fork bomb"),
            (
                r"(?i)\bsystemctl\s+(stop|disable|mask)\s+(ssh|networking|firewalld|ufw)\b",
                "stop critical system service",
            ),
        ])
    })
}

/// 需人工确认。
fn ask_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        compile(&[
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)",
                "recursive delete (rm -rf)",
            ),
            (r"(?i)\bdd\s+.*\bof\s*=", "dd write"),
            (r"(?i)curl\s+[^|]*\|\s*(ba)?sh", "curl piped to shell"),
            (r"(?i)wget\s+[^|]*\|\s*(ba)?sh", "wget piped to shell"),
            (r"(?i)\bDROP\s+TABLE\b", "SQL DROP TABLE"),
            (
                r"(?i)\bDELETE\s+FROM\b(?![^\n]*\bWHERE\b)",
                "SQL DELETE without WHERE",
            ),
            (
                r"(?i)\bsystemctl\s+(stop|disable|mask)\b",
                "systemctl stop/disable",
            ),
        ])
    })
}

/// 低危自动批：常见构建缓存清理。
fn auto_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        compile(&[
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?node_modules\b",
                "rm -rf node_modules (auto)",
            ),
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?target\b",
                "rm -rf target (auto)",
            ),
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?dist\b",
                "rm -rf dist (auto)",
            ),
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?\.next\b",
                "rm -rf .next (auto)",
            ),
            (
                r"(?i)\brm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?__pycache__\b",
                "rm -rf __pycache__ (auto)",
            ),
        ])
    })
}

fn compile(raw: &[(&'static str, &'static str)]) -> Vec<(Regex, &'static str)> {
    raw.iter()
        .filter_map(|(pat, desc)| Regex::new(pat).ok().map(|re| (re, *desc)))
        .collect()
}

/// 对命令分级；安全命令返回 `None`。
pub fn classify_dangerous_command(command: &str) -> Option<ApprovalDecision> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return None;
    }
    // auto 优先于 ask（白名单覆盖 rm -rf node_modules）
    for (re, desc) in auto_patterns() {
        if re.is_match(cmd) {
            return Some(ApprovalDecision {
                action: ApprovalAction::Auto,
                description: desc,
            });
        }
    }
    for (re, desc) in deny_patterns() {
        if re.is_match(cmd) {
            return Some(ApprovalDecision {
                action: ApprovalAction::Deny,
                description: desc,
            });
        }
    }
    for (re, desc) in ask_patterns() {
        if re.is_match(cmd) {
            return Some(ApprovalDecision {
                action: ApprovalAction::Ask,
                description: desc,
            });
        }
    }
    None
}

/// 命令是否命中 **hardline blocklist**（`Deny` 级）——任何模式 / 白名单都不可越过。
///
/// 返回命中的描述；未命中为 `None`。
pub fn is_hardline_blocked(command: &str) -> Option<&'static str> {
    let cmd = command.trim();
    if cmd.is_empty() {
        return None;
    }
    for (re, desc) in deny_patterns() {
        if re.is_match(cmd) {
            return Some(desc);
        }
    }
    None
}

/// 命令是否命中用户白名单。
///
/// 每个条目：含 `* ? [` 时按大小写不敏感 glob 整串匹配；否则按整串精确匹配（忽略大小写）。
pub fn matches_allowlist(command: &str, allowlist: &[String]) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    allowlist
        .iter()
        .any(|pat| allowlist_pattern_matches(pat, cmd))
}

fn allowlist_pattern_matches(pattern: &str, cmd: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return false;
    }
    if p.contains('*') || p.contains('?') || p.contains('[') {
        match glob_to_regex(p) {
            Some(re) => re.is_match(cmd),
            None => false,
        }
    } else {
        cmd.eq_ignore_ascii_case(p)
    }
}

/// 把 fnmatch 风格 glob 编译为大小写不敏感、整串锚定的正则。
fn glob_to_regex(glob: &str) -> Option<Regex> {
    let mut re = String::with_capacity(glob.len() + 8);
    re.push_str("(?i)^");
    let mut chars = glob.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '*' => re.push_str(".*"),
            '?' => re.push('.'),
            '[' => {
                // 透传字符类到 ']'
                re.push('[');
                for cc in chars.by_ref() {
                    re.push(cc);
                    if cc == ']' {
                        break;
                    }
                }
            }
            other => re.push_str(&regex::escape(&other.to_string())),
        }
    }
    re.push('$');
    Regex::new(&re).ok()
}

/// 结合审批模式与白名单，得出终审动作。
///
/// 优先级：**hardline（Deny）** > 白名单（Auto）> 规则分级；`Off` 把 `Ask` 降级为 `Auto`，
/// hardline 永不降级。`None`（安全命令）视为 `Auto`。
pub fn resolve_command_action(
    command: &str,
    mode: ApprovalMode,
    allowlist: &[String],
) -> ApprovalAction {
    if is_hardline_blocked(command).is_some() {
        return ApprovalAction::Deny;
    }
    if matches_allowlist(command, allowlist) {
        return ApprovalAction::Auto;
    }
    match classify_dangerous_command(command).map(|d| d.action) {
        Some(ApprovalAction::Deny) => ApprovalAction::Deny, // 防御性：理论上已被 hardline 捕获
        Some(ApprovalAction::Ask) => {
            if mode == ApprovalMode::Off {
                ApprovalAction::Auto
            } else {
                ApprovalAction::Ask
            }
        }
        Some(ApprovalAction::Auto) | None => ApprovalAction::Auto,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_node_modules() {
        let d = classify_dangerous_command("rm -rf node_modules").unwrap();
        assert_eq!(d.action, ApprovalAction::Auto);
    }

    #[test]
    fn ask_rm_rf_tmp() {
        let d = classify_dangerous_command("rm -rf /tmp/x").unwrap();
        assert_eq!(d.action, ApprovalAction::Ask);
    }

    #[test]
    fn deny_mkfs() {
        let d = classify_dangerous_command("mkfs.ext4 /dev/sdb1").unwrap();
        assert_eq!(d.action, ApprovalAction::Deny);
    }

    #[test]
    fn safe_commands_pass() {
        assert!(classify_dangerous_command("ls -la").is_none());
        assert!(classify_dangerous_command("cargo test").is_none());
        assert!(classify_dangerous_command("rm file.txt").is_none());
    }

    #[test]
    fn detects_curl_pipe_sh() {
        let d = classify_dangerous_command("curl https://x.sh | bash").unwrap();
        assert_eq!(d.action, ApprovalAction::Ask);
    }

    #[test]
    fn mode_parse_lenient() {
        assert_eq!(ApprovalMode::parse_lenient("manual"), ApprovalMode::Manual);
        assert_eq!(ApprovalMode::parse_lenient("OFF"), ApprovalMode::Off);
        assert_eq!(ApprovalMode::parse_lenient("yolo"), ApprovalMode::Off);
        assert_eq!(ApprovalMode::parse_lenient("wat"), ApprovalMode::Smart);
        assert_eq!(ApprovalMode::default(), ApprovalMode::Smart);
    }

    #[test]
    fn hardline_never_overridden() {
        assert!(is_hardline_blocked("mkfs.ext4 /dev/sdb1").is_some());
        // off 模式 + 命中白名单都不能放行 hardline
        let allow = vec!["mkfs*".to_string()];
        assert_eq!(
            resolve_command_action("mkfs.ext4 /dev/sdb1", ApprovalMode::Off, &allow),
            ApprovalAction::Deny
        );
    }

    #[test]
    fn allowlist_exact_and_glob() {
        let allow = vec![
            "rm -rf /tmp/build".to_string(),
            "rm -rf *node_modules".to_string(),
        ];
        // 精确
        assert!(matches_allowlist("rm -rf /tmp/build", &allow));
        // glob
        assert!(matches_allowlist("rm -rf ./frontend/node_modules", &allow));
        // 不匹配
        assert!(!matches_allowlist("rm -rf /etc", &allow));
    }

    #[test]
    fn resolve_respects_mode_and_allowlist() {
        let empty: Vec<String> = vec![];
        // Ask 命令：smart/manual 仍需 Ask
        assert_eq!(
            resolve_command_action("rm -rf /tmp/x", ApprovalMode::Smart, &empty),
            ApprovalAction::Ask
        );
        assert_eq!(
            resolve_command_action("rm -rf /tmp/x", ApprovalMode::Manual, &empty),
            ApprovalAction::Ask
        );
        // off 把 Ask 降级为 Auto
        assert_eq!(
            resolve_command_action("rm -rf /tmp/x", ApprovalMode::Off, &empty),
            ApprovalAction::Auto
        );
        // 命中白名单 → Auto（即便 smart）
        let allow = vec!["rm -rf /tmp/x".to_string()];
        assert_eq!(
            resolve_command_action("rm -rf /tmp/x", ApprovalMode::Smart, &allow),
            ApprovalAction::Auto
        );
        // 安全命令 → Auto
        assert_eq!(
            resolve_command_action("ls -la", ApprovalMode::Smart, &empty),
            ApprovalAction::Auto
        );
    }
}
