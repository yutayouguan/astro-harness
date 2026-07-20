//! 候选门禁：体积、patch 结构完整性、可选沙箱测试。
//!
//! 沙箱测试：将候选 apply 到 tempdir，运行 `scripts/test.{sh,py}`（若存在），
//! 结果作为适应度信号（`TestOutcome`），不作硬性门禁（技能无脚本时为 `NotApplicable`）。

use std::path::Path;
use std::time::Duration;

use memory::EvolutionGates;

use crate::candidate::{CandidateKind, SkillCandidate};
use crate::search::effective_candidate_size;

/// 门禁结果。
#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub passed: bool,
    pub reasons: Vec<String>,
}

impl GateOutcome {
    fn pass() -> Self {
        Self {
            passed: true,
            reasons: Vec::new(),
        }
    }
    fn fail(reason: impl Into<String>) -> Self {
        Self {
            passed: false,
            reasons: vec![reason.into()],
        }
    }
}

/// 对单个候选做静态门禁（不含测试运行）。
///
/// `current_skill`：patch 时传入目标技能当前 SKILL.md，用 post-image 体积；
/// 缺省则 patch 在 dry-run 失败时直接拦截。
pub fn check_candidate(
    c: &SkillCandidate,
    gates: &EvolutionGates,
    current_skill: Option<&str>,
) -> GateOutcome {
    // 策展动作：不走体积/patch 校验
    if matches!(c.kind, CandidateKind::Disable | CandidateKind::Merge) {
        if c.kind == CandidateKind::Merge {
            let has_absorb = c.sources.iter().any(|s| s.starts_with("absorb:"));
            if !has_absorb {
                return GateOutcome::fail("merge 缺少 absorb 清单");
            }
        }
        return GateOutcome::pass();
    }
    // 体积门禁（patch 用 effective_candidate_size，与搜索 Pareto 一致）
    let len = match effective_candidate_size(c, current_skill) {
        Ok(n) => n,
        Err(e) => return GateOutcome::fail(format!("patch dry-run 失败: {e}")),
    };
    if len == 0 {
        return GateOutcome::fail("候选内容为空");
    }
    if gates.max_skill_bytes > 0 && len > gates.max_skill_bytes {
        return GateOutcome::fail(format!(
            "超出体积上限：{len} > {} 字节",
            gates.max_skill_bytes
        ));
    }
    // patch 结构完整性
    if c.kind == CandidateKind::Patch {
        let old_ok = c.old_string.as_deref().map(str::is_empty) == Some(false);
        if !old_ok {
            return GateOutcome::fail("patch 缺少 old_string");
        }
        if c.new_string.is_none() {
            return GateOutcome::fail("patch 缺少 new_string");
        }
        if c.old_string == c.new_string {
            return GateOutcome::fail("patch old_string 与 new_string 相同");
        }
    }
    GateOutcome::pass()
}

// ---------------------------------------------------------------------------
// 沙箱测试
// ---------------------------------------------------------------------------

/// 技能测试脚本执行结果。
#[derive(Debug, Clone, PartialEq)]
pub enum TestOutcome {
    /// 测试通过（退出码 0）。
    Passed,
    /// 测试失败（退出码非零或超时）。摘要截断至 500 字符。
    Failed(String),
    /// 该技能无 `scripts/test.{sh,py}`，不参与适应度评分。
    NotApplicable,
}

impl TestOutcome {
    /// 适应度分：Passed → 1.0，Failed → 0.0，NotApplicable → None。
    pub fn fitness(&self) -> Option<f32> {
        match self {
            TestOutcome::Passed => Some(1.0),
            TestOutcome::Failed(_) => Some(0.0),
            TestOutcome::NotApplicable => None,
        }
    }
}

/// 在给定目录中查找并运行 `scripts/test.{sh,py}`。
///
/// - 无脚本 → `NotApplicable`
/// - 超时 → `Failed`
/// - 不继承调用方环境变量中的 API Key（显式清除 `ASTRO_DSPY_API_KEY` 等）
pub fn run_skill_tests_in_dir(skill_dir: &Path, timeout: Duration) -> TestOutcome {
    let scripts = skill_dir.join("scripts");
    let (program, script) = if scripts.join("test.sh").is_file() {
        ("sh", scripts.join("test.sh"))
    } else if scripts.join("test.py").is_file() {
        ("python3", scripts.join("test.py"))
    } else {
        return TestOutcome::NotApplicable;
    };

    let mut cmd = std::process::Command::new(program);
    cmd.arg(&script)
        .current_dir(skill_dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .env_remove("ASTRO_DSPY_API_KEY")
        .env_remove("OPENAI_API_KEY")
        .env_remove("ANTHROPIC_API_KEY");

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return TestOutcome::Failed(format!("启动测试失败: {e}")),
    };

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return TestOutcome::Passed;
                }
                let tail = child
                    .wait_with_output()
                    .ok()
                    .map(|o| {
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(500)
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                return TestOutcome::Failed(format!("退出码非零: {tail}"));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return TestOutcome::Failed("测试超时".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => return TestOutcome::Failed(format!("执行出错: {e}")),
        }
    }
}

/// Docker 容器内运行测试（对标 GEPA gskill 的 Docker harness）。
///
/// `--network=none` 阻止网络访问；`--rm` 自动清理容器。
/// Docker 不可用时返回 `Failed`（调用方负责 fallback）。
pub fn docker_run_skill_tests(skill_dir: &Path, image: &str, timeout: Duration) -> TestOutcome {
    let scripts = skill_dir.join("scripts");
    let (_, script_name) = if scripts.join("test.sh").is_file() {
        ("sh", "test.sh")
    } else if scripts.join("test.py").is_file() {
        ("python3", "test.py")
    } else {
        return TestOutcome::NotApplicable;
    };

    if std::process::Command::new("docker")
        .args(["info"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| !s.success())
        .unwrap_or(true)
    {
        return TestOutcome::Failed("Docker 不可用".into());
    }

    let dir_str = skill_dir.to_string_lossy();
    let mut child = match std::process::Command::new("docker")
        .args([
            "run",
            "--rm",
            "--network=none",
            "-v",
            &format!("{dir_str}:/skill:ro"),
            "-w",
            "/skill",
            image,
            "sh",
            &format!("scripts/{script_name}"),
        ])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return TestOutcome::Failed(format!("启动 Docker 失败: {e}")),
    };

    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if status.success() {
                    return TestOutcome::Passed;
                }
                let tail = child
                    .wait_with_output()
                    .ok()
                    .map(|o| {
                        String::from_utf8_lossy(&o.stderr)
                            .chars()
                            .take(500)
                            .collect::<String>()
                    })
                    .unwrap_or_default();
                return TestOutcome::Failed(format!("Docker 测试退出码非零: {tail}"));
            }
            Ok(None) => {
                if start.elapsed() > timeout {
                    let _ = child.kill();
                    return TestOutcome::Failed("Docker 测试超时".into());
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => return TestOutcome::Failed(format!("Docker 执行出错: {e}")),
        }
    }
}

/// 将候选 apply 到 tempdir 并跑测试，返回测试结果。
///
/// `sandbox_mode`：`"tempdir"`（默认）或 `"docker"`（容器隔离）。
/// Docker 不可用时自动 fallback 到 tempdir。
pub fn sandbox_test_candidate(
    cand: &SkillCandidate,
    current_skill_content: Option<&str>,
    test_scripts_dir: Option<&Path>,
    timeout: Duration,
    sandbox_mode: &str,
    docker_image: &str,
) -> TestOutcome {
    let test_dir =
        test_scripts_dir.filter(|p| p.join("test.sh").is_file() || p.join("test.py").is_file());
    if test_dir.is_none() {
        return TestOutcome::NotApplicable;
    }

    let tmpdir = match tempfile::tempdir() {
        Ok(d) => d,
        Err(e) => return TestOutcome::Failed(format!("创建临时目录失败: {e}")),
    };
    let skill_dir = tmpdir.path();

    // 写 SKILL.md
    let md_content = match cand.kind {
        CandidateKind::NewSkill => match crate::proposal::candidate_new_markdown(cand) {
            Ok(md) => md,
            Err(e) => return TestOutcome::Failed(format!("生成 SKILL.md 失败: {e}")),
        },
        CandidateKind::Patch => {
            let base_text = match current_skill_content {
                Some(t) => t,
                None => return TestOutcome::Failed("patch 缺少 current_skill_content".into()),
            };
            let old = cand.old_string.as_deref().unwrap_or("");
            let new = cand.new_string.as_deref().unwrap_or("");
            match crate::proposal::apply_patch_unique(base_text, old, new) {
                Ok(patched) => patched,
                Err(e) => return TestOutcome::Failed(format!("patch 失败: {e}")),
            }
        }
        CandidateKind::Disable | CandidateKind::Merge => return TestOutcome::NotApplicable,
    };
    if std::fs::write(skill_dir.join("SKILL.md"), md_content.as_bytes()).is_err() {
        return TestOutcome::Failed("写入 SKILL.md 失败".into());
    }

    // 复制测试脚本目录
    if let Some(src) = test_dir {
        let dest = skill_dir.join("scripts");
        if let Err(e) = copy_dir_recursive(src, &dest) {
            return TestOutcome::Failed(format!("复制测试脚本失败: {e}"));
        }
    }

    if sandbox_mode == "docker" {
        let result = docker_run_skill_tests(skill_dir, docker_image, timeout);
        match &result {
            TestOutcome::Failed(msg) if msg.contains("Docker 不可用") => {
                tracing::warn!("Docker 不可用，回退到 tempdir 模式");
                run_skill_tests_in_dir(skill_dir, timeout)
            }
            _ => result,
        }
    } else {
        run_skill_tests_in_dir(skill_dir, timeout)
    }
}

fn copy_dir_recursive(src: &Path, dest: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let dst = dest.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_recursive(&entry.path(), &dst)?;
        } else {
            std::fs::copy(entry.path(), &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::candidate::CandidateKind;

    fn cand(kind: CandidateKind) -> SkillCandidate {
        SkillCandidate {
            id: "1".into(),
            kind,
            skill_id: "demo".into(),
            description: None,
            content: Some("# demo".into()),
            old_string: Some("a".into()),
            new_string: Some("b".into()),
            rationale: String::new(),
            sources: vec![],
            judge_score: None,
            judge_reason: None,
            created_at: "now".into(),
        }
    }

    #[test]
    fn size_gate_blocks_oversize() {
        let mut c = cand(CandidateKind::NewSkill);
        c.content = Some("x".repeat(100));
        let gates = EvolutionGates {
            run_tests: false,
            max_skill_bytes: 10,
            require_pr: true,
            min_judge_score: 0.6,
            ..EvolutionGates::default()
        };
        let out = check_candidate(&c, &gates, None);
        assert!(!out.passed);
        assert!(out.reasons[0].contains("体积"));
    }

    #[test]
    fn patch_needs_distinct_old_new() {
        let mut c = cand(CandidateKind::Patch);
        c.new_string = c.old_string.clone();
        let gates = EvolutionGates::default();
        assert!(!check_candidate(&c, &gates, Some("# demo\na")).passed);
    }

    #[test]
    fn valid_new_skill_passes() {
        let c = cand(CandidateKind::NewSkill);
        assert!(check_candidate(&c, &EvolutionGates::default(), None).passed);
    }

    #[test]
    fn patch_size_uses_post_image() {
        let mut c = cand(CandidateKind::Patch);
        c.old_string = Some("short".into());
        c.new_string = Some("x".repeat(50));
        let gates = EvolutionGates {
            run_tests: false,
            max_skill_bytes: 20,
            require_pr: true,
            min_judge_score: 0.6,
            ..EvolutionGates::default()
        };
        // post-image 全文超 20 字节，应被体积门禁拦截
        assert!(!check_candidate(&c, &gates, Some("# demo\nshort tail")).passed);
    }

    #[test]
    fn test_outcome_fitness() {
        assert_eq!(TestOutcome::Passed.fitness(), Some(1.0));
        assert_eq!(TestOutcome::Failed("x".into()).fitness(), Some(0.0));
        assert_eq!(TestOutcome::NotApplicable.fitness(), None);
    }

    #[test]
    fn run_tests_no_scripts_returns_not_applicable() {
        let dir = tempfile::tempdir().unwrap();
        let r = run_skill_tests_in_dir(dir.path(), Duration::from_secs(5));
        assert_eq!(r, TestOutcome::NotApplicable);
    }

    #[test]
    fn run_tests_passing_script() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("test.sh"), "#!/bin/sh\nexit 0\n").unwrap();
        let r = run_skill_tests_in_dir(dir.path(), Duration::from_secs(5));
        assert_eq!(r, TestOutcome::Passed);
    }

    #[test]
    fn run_tests_failing_script() {
        let dir = tempfile::tempdir().unwrap();
        let scripts = dir.path().join("scripts");
        std::fs::create_dir_all(&scripts).unwrap();
        std::fs::write(scripts.join("test.sh"), "#!/bin/sh\nexit 1\n").unwrap();
        let r = run_skill_tests_in_dir(dir.path(), Duration::from_secs(5));
        assert!(matches!(r, TestOutcome::Failed(_)));
    }

    #[test]
    fn sandbox_no_test_dir_returns_not_applicable() {
        let c = cand(CandidateKind::NewSkill);
        let r = sandbox_test_candidate(&c, None, None, Duration::from_secs(5), "tempdir", "");
        assert_eq!(r, TestOutcome::NotApplicable);
    }

    #[test]
    fn sandbox_passing_new_skill() {
        let scripts_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            scripts_dir.path().join("test.sh"),
            "#!/bin/sh\n[ -f SKILL.md ]\n",
        )
        .unwrap();
        let c = cand(CandidateKind::NewSkill);
        let r = sandbox_test_candidate(
            &c,
            None,
            Some(scripts_dir.path()),
            Duration::from_secs(5),
            "tempdir",
            "",
        );
        assert_eq!(r, TestOutcome::Passed);
    }

    #[test]
    fn sandbox_docker_fallback_when_unavailable() {
        let scripts_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            scripts_dir.path().join("test.sh"),
            "#!/bin/sh\n[ -f SKILL.md ]\n",
        )
        .unwrap();
        let c = cand(CandidateKind::NewSkill);
        let r = sandbox_test_candidate(
            &c,
            None,
            Some(scripts_dir.path()),
            Duration::from_secs(5),
            "docker",
            "nonexistent-image:v999",
        );
        // Docker may or may not be available; either way should not panic
        assert!(matches!(r, TestOutcome::Passed | TestOutcome::Failed(_)));
    }
}
