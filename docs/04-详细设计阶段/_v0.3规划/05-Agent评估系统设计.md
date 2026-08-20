# Agent 评估系统

> v0.3 精简版 | 状态：规划中 | 聚焦确定性断言测试，LLM Judge 推迟至 v0.4
> 原文档：561 行（LLM-as-Judge、多投票反幻觉、并行执行引擎、回归检测、前端 Dashboard），已归档精简

---

## 1. 数据集格式

```yaml
# evals/translation.yaml
id: translation-basic
version: "1.0.0"
description: 翻译任务基础测试
judge: contains          # exact | contains | regex

cases:
  - id: hello-world
    input: "翻译：Hello World"
    expected:
      contains: ["你好", "世界"]
    model: "claude-sonnet-4"

  - id: greeting
    input: "翻译：Good morning"
    expected:
      contains: ["早上好"]

  - id: code-term
    input: "翻译：variable declaration"
    expected:
      regex: "变量.{0,2}声明"
```

数据集存放在 `evals/` 目录，按功能分组：

```
evals/
├── translation.yaml
├── tool-file-read.yaml
├── tool-shell-exec.yaml
└── golden-set.yaml        # CI 门控用例
```

---

## 2. Judge 类型

| 类型 | 判定规则 | 适用场景 |
|------|---------|---------|
| **Exact** | `output == expected` 完全匹配 | 格式化输出、固定模板 |
| **Contains** | 输出包含 expected 中的所有关键词 | 翻译、摘要、知识问答 |
| **Regex** | 输出匹配正则表达式 | 结构化输出、数值范围 |

```rust
// crates/agent-runtime/src/eval/judge.rs

pub enum JudgeType { Exact, Contains, Regex }

pub fn judge(judge_type: &JudgeType, output: &str, expected: &EvalExpected) -> bool {
    match judge_type {
        JudgeType::Exact => {
            expected.exact.as_ref().map_or(false, |e| output.trim() == e.trim())
        }
        JudgeType::Contains => {
            expected.contains.iter().all(|kw| output.contains(kw))
        }
        JudgeType::Regex => {
            expected.regex.as_ref().map_or(false, |pattern| {
                regex::Regex::new(pattern).map_or(false, |re| re.is_match(output))
            })
        }
    }
}
```

---

## 3. Eval Runner

```rust
// crates/agent-runtime/src/eval/runner.rs

pub struct EvalRunner {
    db_pool: SqlitePool,
    agent_factory: Arc<dyn AgentFactory>,
}

pub struct EvalRunResult {
    pub dataset_id: String,
    pub total: usize,
    pub passed: usize,
    pub failed: Vec<FailedCase>,
    pub pass_rate: f32,
    pub duration_ms: u64,
}

impl EvalRunner {
    pub async fn run_dataset(&self, dataset: &EvalDataset) -> EvalRunResult {
        let mut passed = 0;
        let mut failed = Vec::new();

        for case in &dataset.cases {
            let agent = self.agent_factory.create_eval_agent().await;
            let output = agent.run(&case.input).await;
            let ok = judge(&dataset.judge, &output.text, &case.expected);
            if ok {
                passed += 1;
            } else {
                failed.push(FailedCase {
                    case_id: case.id.clone(),
                    output: output.text.clone(),
                    expected: case.expected.clone(),
                });
            }
        }

        EvalRunResult {
            dataset_id: dataset.id.clone(),
            total: dataset.cases.len(),
            passed,
            failed,
            pass_rate: passed as f32 / dataset.cases.len() as f32,
            ..
        }
    }
}
```

---

## 4. CLI

```bash
# 运行指定数据集，通过率 >= 90% 则成功，否则退出码 1
astro eval run --dataset evals/translation.yaml --threshold 0.9

# 运行 golden set（CI 门控）
astro eval run --dataset evals/golden-set.yaml --threshold 1.0

# 列出所有数据集
astro eval list

# 查看上次运行结果
astro eval results --last
```

**输出示例**：

```
Dataset: translation-basic (3 cases)
  [PASS] hello-world
  [PASS] greeting
  [FAIL] code-term
    Output:   "变量定义"
    Expected: regex "变量.{0,2}声明"

Result: 2/3 passed (66.7%) — BELOW threshold 90%
```

---

## 5. CI 门控

```yaml
# .github/workflows/eval-gate.yml
- name: Run Golden Set
  run: astro eval run --dataset evals/golden-set.yaml --threshold 0.9
  env:
    ANTHROPIC_API_KEY: ${{ secrets.ANTHROPIC_API_KEY }}
```

Golden Set 维护原则：
- 规模控制在 20-30 个用例
- 每个关键功能至少 1 个用例
- 只使用确定性 Judge（Exact/Contains/Regex），不用 LLM Judge
- 失败 → 阻断合并

---

## 已删除的过度设计

以下内容从原文档中移除，v0.3 不实现：

- ~~LLM-as-Judge（复杂、成本高、结果不稳定）~~ → 推迟至 v0.4
- ~~多投票反幻觉（同一用例跑 3 次 Judge 取多数票）~~ → 不需要，无 LLM Judge
- ~~并行执行引擎 + Semaphore~~ → 串行执行足够，数据集规模小
- ~~回归检测 + pass_rate delta~~ → 简化为 threshold 门控
- ~~前端 Eval Dashboard~~ → CLI 输出即可
- ~~与自我进化引擎的集成~~ → 自我进化引擎本身也推迟
