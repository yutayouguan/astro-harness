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
            (
                r"(?i)\bgit\b[^\n;&|]*\breset\s+--hard\b",
                "git reset --hard discards local changes",
            ),
            (
                r"(?i)\bgit\b[^\n;&|]*\bclean\b[^\n;&|]*(?:\s-[a-z]*f[a-z]*\b|\s--force\b)",
                "git clean --force deletes untracked files",
            ),
            (
                r"(?i)\bgit\b[^\n;&|]*\bcheckout\s+--\s+\S+",
                "git checkout -- discards path changes",
            ),
            (
                r"(?i)\bgit\b[^\n;&|]*\brestore\b",
                "git restore changes the worktree or index",
            ),
            (
                r"(?i)\bgit\b[^\n;&|]*\bbranch\b[^\n;&|]*(?:\s-[a-z]*d[a-z]*\b|\s--delete\b)",
                "git branch delete",
            ),
            (
                r"(?i)\bgit\b[^\n;&|]*\bpush\b[^\n;&|]*(?:\s-[a-z]*f[a-z]*\b|\s--force(?:-with-lease|-if-includes)?\b)",
                "forced git push",
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
                r"(?i)^\s*rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?node_modules/?\s*$",
                "rm -rf node_modules (auto)",
            ),
            (
                r"(?i)^\s*rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?target/?\s*$",
                "rm -rf target (auto)",
            ),
            (
                r"(?i)^\s*rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?dist/?\s*$",
                "rm -rf dist (auto)",
            ),
            (
                r"(?i)^\s*rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?\.next/?\s*$",
                "rm -rf .next (auto)",
            ),
            (
                r"(?i)^\s*rm\s+(-[a-zA-Z]*r[a-zA-Z]*f|-[a-zA-Z]*f[a-zA-Z]*r)\s+(\./)?__pycache__/?\s*$",
                "rm -rf __pycache__ (auto)",
            ),
        ])
    })
}

/// 当命令包含 shell 动态语法时返回 true——此时运行时实际值
/// 不能视为审批规则所用的字面源文本。
fn contains_dynamic_shell_words(command: &str) -> bool {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        Unquoted,
        Single,
        Double,
    }

    let mut quote = Quote::Unquoted;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        match quote {
            Quote::Unquoted => match character {
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '$' | '`' | '{' | '}' | '*' | '?' | '[' | ']' | '\\' | '~' | '^' | '#' => {
                    return true;
                }
                _ => {}
            },
            Quote::Single => {
                if character == '\'' {
                    quote = Quote::Unquoted;
                }
            }
            Quote::Double => match character {
                '"' => quote = Quote::Unquoted,
                '$' | '`' => return true,
                '\\' if chars
                    .peek()
                    .is_some_and(|next| matches!(next, '$' | '`' | '"' | '\\' | '\n')) =>
                {
                    return true;
                }
                _ => {}
            },
        }
    }
    quote != Quote::Unquoted
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
    // hardline 必须最先判定，避免安全前缀掩盖复合命令中的拒绝规则。
    for (re, desc) in deny_patterns() {
        if re.is_match(cmd) {
            return Some(ApprovalDecision {
                action: ApprovalAction::Deny,
                description: desc,
            });
        }
    }

    // 未展开的 shell 词不能拿源码字面量证明运行时 argv 安全，也不能命中 glob 白名单。
    if contains_dynamic_shell_words(cmd) {
        // 明确的危险模式优先于笼统分类，避免 `curl $URL | sh` 获得低风险类型许可。
        for (re, desc) in ask_patterns() {
            if re.is_match(cmd) {
                return Some(ApprovalDecision {
                    action: ApprovalAction::Ask,
                    description: desc,
                });
            }
        }
        return Some(ApprovalDecision {
            action: ApprovalAction::Ask,
            description: "dynamic shell expansion",
        });
    }

    // auto 规则必须整串匹配，且只能在排除动态 shell 词后生效。
    for (re, desc) in auto_patterns() {
        if re.is_match(cmd) {
            return Some(ApprovalDecision {
                action: ApprovalAction::Auto,
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
    let dynamic = contains_dynamic_shell_words(cmd);
    allowlist.iter().any(|pattern| {
        (!dynamic || !is_glob_pattern(pattern)) && allowlist_pattern_matches(pattern, cmd)
    })
}

fn is_glob_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[')
}

fn allowlist_pattern_matches(pattern: &str, cmd: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return false;
    }
    if is_glob_pattern(p) {
        match glob_to_regex(p) {
            Some(re) => re.is_match(cmd),
            None => false,
        }
    } else {
        cmd.eq_ignore_ascii_case(p)
    }
}

/// 为低风险动态命令生成可持久化的“同类命令”规则。
///
/// 规则同时绑定程序族和风险分类；复合 shell、解释器、删除/系统管理命令不会产生候选项。
pub fn command_type_rule_candidate(command: &str, risk: &str) -> Option<memory::CommandTypeRule> {
    if risk != "dynamic shell expansion" {
        return None;
    }
    let words = simple_command_words(command)?;
    let executable = words
        .iter()
        .find(|word| !is_environment_assignment(word) && word.as_str() != "env")?;
    let family = executable
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(executable)
        .to_ascii_lowercase();
    const ELIGIBLE_FAMILIES: &[&str] = &[
        "cat", "curl", "grep", "head", "jq", "ls", "rg", "sed", "tail", "wc",
    ];
    if !ELIGIBLE_FAMILIES.contains(&family.as_str()) {
        return None;
    }
    Some(memory::CommandTypeRule {
        command_family: family,
        risk: risk.to_string(),
    })
}

/// 命令是否命中一条已保存的低风险命令类型规则。
pub fn matches_command_type_allowlist(
    command: &str,
    risk: &str,
    rules: &[memory::CommandTypeRule],
) -> bool {
    let Some(candidate) = command_type_rule_candidate(command, risk) else {
        return false;
    };
    rules.iter().any(|rule| {
        rule.command_family
            .eq_ignore_ascii_case(&candidate.command_family)
            && rule.risk == candidate.risk
    })
}

fn is_environment_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|character| character == '_' || character.is_ascii_alphabetic())
        && chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
}

fn simple_command_words(command: &str) -> Option<Vec<String>> {
    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Quote {
        Unquoted,
        Single,
        Double,
    }
    let mut quote = Quote::Unquoted;
    let mut escaped = false;
    let mut current = String::new();
    let mut words = Vec::new();
    for character in command.trim().chars() {
        if escaped {
            current.push(character);
            escaped = false;
            continue;
        }
        match quote {
            Quote::Unquoted => match character {
                '\\' => escaped = true,
                '\'' => quote = Quote::Single,
                '"' => quote = Quote::Double,
                '|' | '&' | ';' | '\n' | '\r' => return None,
                value if value.is_whitespace() => {
                    if !current.is_empty() {
                        words.push(std::mem::take(&mut current));
                    }
                }
                value => current.push(value),
            },
            Quote::Single => match character {
                '\'' => quote = Quote::Unquoted,
                value => current.push(value),
            },
            Quote::Double => match character {
                '"' => quote = Quote::Unquoted,
                '\\' => escaped = true,
                value => current.push(value),
            },
        }
    }
    if escaped || quote != Quote::Unquoted {
        return None;
    }
    if !current.is_empty() {
        words.push(current);
    }
    (!words.is_empty()).then_some(words)
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
    fn auto_cleanup_must_match_the_entire_command() {
        let d = classify_dangerous_command("rm -rf node_modules; rm -rf /tmp/x").unwrap();
        assert_eq!(d.action, ApprovalAction::Ask);

        let d = classify_dangerous_command("rm -rf node_modules; mkfs.ext4 /dev/sdb1").unwrap();
        assert_eq!(d.action, ApprovalAction::Deny);
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
    fn destructive_git_commands_require_approval() {
        for command in [
            "git reset --hard HEAD~1",
            "git -C repo reset --hard",
            "git clean -fdx",
            "git -c core.excludesFile=/dev/null clean -d -f",
            "git checkout -- src/main.rs",
            "git restore --staged Cargo.lock",
            "git branch -D old-feature",
            "git push origin main --force-with-lease",
        ] {
            let decision = classify_dangerous_command(command).unwrap();
            assert_eq!(decision.action, ApprovalAction::Ask, "{command}");
        }
    }

    #[test]
    fn read_only_git_commands_remain_automatic() {
        for command in ["git status --short", "git log -1", "git diff --stat"] {
            assert_eq!(classify_dangerous_command(command), None, "{command}");
        }
    }

    #[test]
    fn dynamic_shell_words_require_approval() {
        for command in [
            "echo $HOME",
            "echo `whoami`",
            "find . -{delete,print}",
            "find . -del*",
            r"find . -de\lete",
            "echo HEAD~1",
        ] {
            let decision = classify_dangerous_command(command).unwrap();
            assert_eq!(decision.action, ApprovalAction::Ask, "{command}");
        }
    }

    #[test]
    fn quoted_shell_metacharacters_remain_literal() {
        for command in ["echo '$HOME'", r#"echo "*.rs""#, r#"echo "~HOME" 'HEAD~1'"#] {
            assert_eq!(classify_dangerous_command(command), None, "{command}");
        }
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
    fn dynamic_shell_words_only_match_exact_allowlist_entries() {
        let allow = vec!["echo $HOME".to_string(), "find . -del*".to_string()];
        assert!(matches_allowlist("echo $HOME", &allow));
        assert!(!matches_allowlist("find . -del*", &allow));
    }

    #[test]
    fn command_type_rules_are_limited_to_simple_low_risk_families() {
        let command = r#"UA="astro/1" curl -H "User-Agent: $UA" https://example.com"#;
        let rule = command_type_rule_candidate(command, "dynamic shell expansion").unwrap();
        assert_eq!(rule.command_family, "curl");
        assert!(matches_command_type_allowlist(
            command,
            "dynamic shell expansion",
            std::slice::from_ref(&rule)
        ));
        assert!(command_type_rule_candidate("rm -rf $TARGET", "dynamic shell expansion").is_none());
        assert!(
            command_type_rule_candidate("curl $URL | bash", "dynamic shell expansion").is_none()
        );
        assert!(
            command_type_rule_candidate("python -c $CODE", "dynamic shell expansion").is_none()
        );
    }

    #[test]
    fn specific_danger_is_not_masked_by_dynamic_expansion() {
        let decision = classify_dangerous_command("curl $URL | bash").unwrap();
        assert_eq!(decision.description, "curl piped to shell");
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
