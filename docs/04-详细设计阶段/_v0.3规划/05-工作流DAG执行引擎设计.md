# 工作流执行引擎设计（SkillChain）

> v0.3 精简版 | 状态：规划中 | 原始设计已精简，保留 MVP 核心

---

## 1. 设计定位

v0.3 的工作流执行引擎简化为 **SkillChain** -- 一个顺序 Skill 链执行器，支持简单条件分支。不实现完整的 DAG 拓扑排序和并行执行。

- **顺序 Skill 链**：按顺序执行 N 个 Skill
- **简单条件分支**：if/else 基于上一步输出
- **错误处理**：失败时立即停止（FailFast 唯一策略）
- **编译为 SKILL.md**：将 SkillChain 导出为 SKILL.md

---

## 2. 数据结构

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillChain {
    pub id: String,
    pub name: String,
    pub version: String,
    pub steps: Vec<ChainStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChainStep {
    RunSkill {
        skill_name: String,
        input: Value,
    },
    Condition {
        field: String,
        op: ConditionOp,
        value: Value,
        then_steps: Vec<ChainStep>,
        else_steps: Vec<ChainStep>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionOp {
    Eq, Neq, Gt, Lt, Contains, IsEmpty,
}
```

### 存储格式（JSON 示例）

```json
{
  "id": "chain-abc123",
  "name": "代码审查链",
  "version": "1.0.0",
  "steps": [
    { "type": "run_skill", "skill_name": "read-file", "input": { "path": "{{input.file_path}}" } },
    { "type": "run_skill", "skill_name": "code-review", "input": { "content": "{{prev.output}}" } },
    {
      "type": "condition", "field": "issues_count", "op": "gt", "value": 0,
      "then_steps": [{ "type": "run_skill", "skill_name": "apply-fix", "input": {} }],
      "else_steps": []
    }
  ]
}
```

---

## 3. 执行引擎

```rust
pub struct ChainExecutor {
    skill_registry: Arc<RwLock<SkillRegistry>>,
    provider: Arc<dyn TextClient>,
}

impl ChainExecutor {
    pub async fn run(&self, chain: &SkillChain, input: Value) -> anyhow::Result<ChainResult> {
        let mut ctx = ChainContext::new(input);
        for (i, step) in chain.steps.iter().enumerate() {
            match self.execute_step(step, &ctx).await {
                Ok(output) => ctx.push_output(output),
                Err(e) => return Ok(ChainResult {
                    success: false, outputs: ctx.outputs,
                    error: Some(format!("步骤 {} 失败: {}", i, e)),
                }),
            }
        }
        Ok(ChainResult { success: true, outputs: ctx.outputs, error: None })
    }

    async fn execute_step(&self, step: &ChainStep, ctx: &ChainContext) -> anyhow::Result<Value> {
        match step {
            ChainStep::RunSkill { skill_name, input } => {
                let resolved = ctx.resolve_template(input)?;
                let registry = self.skill_registry.read().await;
                let skill = registry.get(skill_name)
                    .ok_or_else(|| anyhow::anyhow!("Skill 不存在: {}", skill_name))?;
                self.run_skill(skill, resolved).await
            }
            ChainStep::Condition { field, op, value, then_steps, else_steps } => {
                let branch = if evaluate_condition(ctx.last_output(), field, op, value) {
                    then_steps
                } else {
                    else_steps
                };
                let mut last = Value::Null;
                for sub in branch { last = self.execute_step(sub, ctx).await?; }
                Ok(last)
            }
        }
    }
}

struct ChainContext { input: Value, outputs: Vec<Value> }

impl ChainContext {
    fn new(input: Value) -> Self { Self { input, outputs: vec![] } }
    fn push_output(&mut self, v: Value) { self.outputs.push(v); }
    fn last_output(&self) -> &Value { self.outputs.last().unwrap_or(&Value::Null) }

    fn resolve_template(&self, value: &Value) -> anyhow::Result<Value> {
        // 替换 {{input.xxx}} 和 {{prev.output.xxx}} 模板变量
        match value {
            Value::String(s) => {
                let mut r = s.clone();
                if r.contains("{{input.") { r = replace_input_refs(&r, &self.input); }
                if r.contains("{{prev.output") { r = replace_prev_refs(&r, self.last_output()); }
                Ok(Value::String(r))
            }
            Value::Object(map) => Ok(Value::Object(map.iter()
                .map(|(k, v)| Ok((k.clone(), self.resolve_template(v)?)))
                .collect::<anyhow::Result<_>>()?)),
            other => Ok(other.clone()),
        }
    }
}

pub struct ChainResult { pub success: bool, pub outputs: Vec<Value>, pub error: Option<String> }
```

---

## 4. 条件求值

```rust
fn evaluate_condition(output: &Value, field: &str, op: &ConditionOp, value: &Value) -> bool {
    let actual = output.get(field).unwrap_or(&Value::Null);
    match op {
        ConditionOp::Eq => actual == value,
        ConditionOp::Neq => actual != value,
        ConditionOp::Gt => cmp_num(actual, value) == Some(std::cmp::Ordering::Greater),
        ConditionOp::Lt => cmp_num(actual, value) == Some(std::cmp::Ordering::Less),
        ConditionOp::Contains => actual.as_str().zip(value.as_str())
            .map(|(a, v)| a.contains(v)).unwrap_or(false),
        ConditionOp::IsEmpty => actual.is_null()
            || actual.as_str().map(|s| s.is_empty()).unwrap_or(false)
            || actual.as_array().map(|a| a.is_empty()).unwrap_or(false),
    }
}
```

---

## 5. 编译为 SKILL.md

```rust
pub fn compile_to_skill(chain: &SkillChain) -> String {
    let steps_desc = chain.steps.iter().enumerate().map(|(i, step)| match step {
        ChainStep::RunSkill { skill_name, .. } => format!("{}. 执行 Skill: {}", i+1, skill_name),
        ChainStep::Condition { field, op, .. } => format!("{}. 条件: {} {:?}", i+1, field, op),
    }).collect::<Vec<_>>().join("\n");

    format!("---\nname: {}\nversion: {}\ndescription: \"SkillChain 编译\"\n\
             origin: compiled\nstatus: ready\n---\n\n# {}\n\n## 步骤\n\n{}\n",
        chain.name, chain.version, chain.name, steps_desc)
}
```

---

## 6. v0.4+ 远期能力（本版本不实现）

Kahn 算法拓扑排序 + DAG 并行执行、FuturesUnordered 并发调度、子工作流嵌套、多种错误策略（SkipOnFailure / ContinueAll）、暂停/恢复与 checkpoint、模板变量解析引擎（完整表达式求值）、LoopNode、React Flow 实时进度。

---

## 相关文档

- [01-Skills系统详细设计.md](01-Skills系统详细设计.md) -- SkillRegistry、SKILL.md 格式
- [04-工作流编辑器.md](../../03-系统设计阶段/06-桌面端/04-工作流编辑器.md) -- React Flow 节点 UI（v0.4+）
