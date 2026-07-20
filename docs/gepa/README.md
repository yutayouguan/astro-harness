# GEPA 完全指南

> **GEPA**（Genetic-Pareto）是基于 LLM 反思的文本进化引擎，可优化 prompt、代码、Agent 架构、配置等任何可文本化的制品。
>
> 论文：[GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning](https://arxiv.org/abs/2507.19457)（Agrawal et al., 2025）
>
> 本指南综合了 [dspy.ai](https://dspy.ai/api/optimizers/GEPA/overview/)、[gepa-ai.github.io](https://gepa-ai.github.io/gepa/)、[OpenAI Cookbook](https://developers.openai.com/cookbook/examples/partners/self_evolving_agents/autonomous_agent_retraining)、[HuggingFace Cookbook](https://huggingface.co/learn/cookbook/en/dspy_gepa) 的内容。

---

## 目录

1. [核心思想](#1-核心思想)
2. [安装与快速开始](#2-安装与快速开始)
3. [三阶段流水线](#3-三阶段流水线)
4. [三种使用方式](#4-三种使用方式)
5. [dspy.GEPA 详解](#5-dspygepa-详解)
6. [Adapter 自定义](#6-adapter-自定义)
7. [高级特性](#7-高级特性)
8. [gskill：学习仓库级编码技能](#8-gskill学习仓库级编码技能)
9. [自演化 Agent 模式](#9-自演化-agent-模式)
10. [与 Astro GEPA-lite 的对照](#10-与-astro-gepa-lite-的对照)

---

## 1. 核心思想

传统优化（RL、进化策略、贝叶斯优化）把丰富的执行轨迹折叠成单标量奖励——它们知道**候选失败了**，但不知道**为什么**。GEPA 用 LLM 对完整执行轨迹做**自然语言反思**，分析失败原因并提出**定向改进**，而非随机扰动。

关键机制是 **Actionable Side Information (ASI)**——领域特定的文本诊断反馈（错误信息、profiling 输出、Agent 推理链），让 LLM proposer 知道**为什么失败**和**怎么修**。

**性能对比**：在 HotPotQA 上比 GRPO 高 20%，rollout 少 35 倍（3 小时 vs 24 小时，$20 vs $300）。

### 适用场景

| 场景 | 原因 |
|------|------|
| Rollout 昂贵 | 科学模拟、慢编译、复杂 Agent 工具调用 — 100–500 evals vs RL 10,000+ |
| 数据稀少 | 新硬件零训练数据；GEPA 最少 3 个例子即可工作 |
| API-only 模型 | 无需模型权重，直接优化 GPT-5/Claude/Gemini |
| 可解释性 | 每步产出可读的反思与改进理由 |
| 与 RL/微调互补 | 先 GEPA 快速初始优化，再 RL/SFT 进一步收益 |

---

## 2. 安装与快速开始

```bash
pip install gepa          # 基础
pip install gepa[full]    # 含所有可选依赖
pip install gepa[gskill]  # 含 gskill（编码技能学习）
```

### 最简示例（standalone）

```python
import gepa

trainset = [
    {"input": "What is 2+2?", "additional_context": {}, "answer": "4"},
    {"input": "Capital of France?", "additional_context": {}, "answer": "Paris"},
]

result = gepa.optimize(
    seed_candidate={"system_prompt": "You are a helpful assistant."},
    trainset=trainset,
    task_lm="openai/gpt-4o-mini",
    reflection_lm="openai/gpt-4o",
    max_metric_calls=50,
)

print("Best prompt:", result.best_candidate["system_prompt"])
print("Best score:", result.val_aggregate_scores[result.best_idx])
```

### 最简示例（DSPy）

```python
import dspy

dspy.configure(lm=dspy.LM("openai/gpt-4o-mini"))

class QA(dspy.Module):
    def __init__(self):
        self.gen = dspy.ChainOfThought("question -> answer")
    def forward(self, question):
        return self.gen(question=question)

def metric(gold, pred, trace=None, pred_name=None, pred_trace=None):
    correct = gold.answer.lower() in pred.answer.lower()
    feedback = f"Correct!" if correct else f"Wrong. Expected '{gold.answer}', got '{pred.answer}'."
    return dspy.Prediction(score=1.0 if correct else 0.0, feedback=feedback)

optimizer = dspy.GEPA(
    metric=metric,
    reflection_lm=dspy.LM("openai/gpt-4o", temperature=1.0, max_tokens=32000),
    auto="light",
    num_threads=8,
)
optimized = optimizer.compile(QA(), trainset=trainset)
print(optimized.gen.signature.instructions)
```

### 最简示例（optimize_anything）

```python
import gepa.optimize_anything as oa

def evaluate(candidate, example):
    result = run_my_system(candidate, example)
    oa.log(f"Output: {result.output}")
    oa.log(f"Error: {result.error}")
    return result.score

result = oa.optimize_anything(
    seed_candidate="<your artifact>",
    evaluator=evaluate,
    dataset=my_data,
    objective="Optimize for accuracy.",
    config=oa.GEPAConfig(engine=oa.EngineConfig(max_metric_calls=200)),
)
```

---

## 3. 三阶段流水线

每轮迭代：

```
┌────────────┐      ┌────────────┐      ┌────────────┐
│  Executor  │ ───▶ │  Reflector │ ───▶ │   Curator  │
│            │      │            │      │            │
│ 跑候选      │      │ 分析轨迹    │      │ 生成改进    │
│ 捕获完整    │      │ 诊断失败    │      │ 候选       │
│ 执行轨迹    │      │ 模式与因果  │      │            │
└────────────┘      └────────────┘      └────────────┘
```

1. **Executor**：用 task model 在 minibatch 上执行候选，捕获推理链、中间输出、错误、ASI
2. **Reflector**：强 LLM（`reflection_lm`）分析轨迹，识别失败模式、逻辑缺陷、因果关系
3. **Curator**：基于反思生成改进候选，继承搜索树中所有祖先的经验

### 双策略候选生成

- **Reflective Mutation**：从 Pareto 前沿采样一个候选 → minibatch 执行 → 反思 → 提出改进版
- **System-Aware Merge**：从前沿采样两个候选 → 基于进化历史策略性融合模块（A 精炼过的模块取 A 的，B 精炼过的取 B 的）

---

## 4. 三种使用方式

### 4.1 DSPy 集成（推荐用于 prompt 优化）

```python
optimizer = dspy.GEPA(
    metric=metric_with_feedback,
    reflection_lm=dspy.LM("openai/gpt-5", temperature=1.0, max_tokens=32000),
    auto="medium",          # 预设预算：light / medium / heavy
    num_threads=16,
    track_stats=True,
)
optimized = optimizer.compile(student, trainset=trainset, valset=valset)
```

### 4.2 Standalone `gepa.optimize()`

```python
result = gepa.optimize(
    seed_candidate={"system_prompt": "..."},
    trainset=train_data,
    valset=val_data,
    adapter=MyAdapter(),
    reflection_lm="openai/gpt-4o",
    max_metric_calls=100,
)
```

### 4.3 `optimize_anything`（最灵活）

支持三种模式：
- **Single-Task Search**：解一个难问题（圆填充、黑盒优化）
- **Multi-Task Search**：批量相关问题 + 跨任务迁移（CUDA kernel）
- **Generalization**：构建可泛化技能（prompt 优化、Agent 架构发现）

```python
# 甚至可以无种子启动
result = oa.optimize_anything(
    evaluator=my_eval,
    objective="Generate a Python function that reverses a string.",
)
```

---

## 5. dspy.GEPA 详解

### 关键参数

| 参数 | 默认 | 说明 |
|------|------|------|
| `metric` | **必填** | 带反馈的评分函数（返回 `float` 或 `ScoreWithFeedback`） |
| `auto` | `None` | 预设预算 `"light"` / `"medium"` / `"heavy"` |
| `max_metric_calls` | `None` | LLM 调用预算硬顶（与 `auto` 二选一） |
| `reflection_lm` | **必填** | 反思用强模型（推荐 `gpt-5, temperature=1.0`） |
| `reflection_minibatch_size` | `3` | 每次反思分析多少错误例 |
| `candidate_selection_strategy` | `"pareto"` | `"pareto"` 从前沿随机采样 / `"current_best"` 总用最优 |
| `use_merge` | `True` | 启用 System-Aware Merge |
| `max_merge_invocations` | `5` | 最大 merge 尝试次数 |
| `component_selector` | `"round_robin"` | 选哪些组件优化：`"round_robin"` / `"all"` / 自定义 |
| `skip_perfect_score` | `True` | 跳过满分样本的反思 |
| `num_threads` | `None` | 评估并行数 |
| `log_dir` | `None` | 保存日志 + 断点续跑 |
| `seed` | `0` | 随机种子 |

### 带反馈的 Metric 函数

```python
def metric(gold, pred, trace=None, pred_name=None, pred_trace=None):
    """
    gold: 标注样本
    pred: 模型预测
    trace: 完整执行轨迹
    pred_name: 当前被优化的 predictor 名
    pred_trace: 该 predictor 的子轨迹

    返回 float 或 dspy.Prediction(score=float, feedback=str)
    """
    correct = gold.answer.lower() in pred.answer.lower()
    score = 1.0 if correct else 0.0

    if correct:
        feedback = f"Correct! Answer '{pred.answer}' matches '{gold.answer}'."
    else:
        feedback = (
            f"Incorrect. Expected '{gold.answer}' but got '{pred.answer}'. "
            f"The solution: {gold.get('solution', 'N/A')}"
        )
    return dspy.Prediction(score=score, feedback=feedback)
```

> **反馈是关键**：信息越丰富，GEPA 反思越精准。包含：出了什么错、预期是什么、参考答案、子分数分解。

### GEPAResult 输出

```python
result.best_candidate                          # 最优文本组件 dict
result.best_idx                                # 最优候选索引
result.val_aggregate_scores                    # 每候选的平均验证分
result.val_aggregate_scores[result.best_idx]   # 最优分
result.candidates                              # 所有探索过的候选
result.per_val_instance_best_candidates        # val_id → Pareto 前沿候选集
result.total_metric_calls                      # 总评估调用数
```

### 批量推理搜索

GEPA 也可作为 inference-time scaling 策略：

```python
gepa = dspy.GEPA(metric=metric, track_stats=True, track_best_outputs=True)
new_prog = gepa.compile(student, trainset=trainset, valset=batch_of_tasks)
# Pareto 前沿结果
pareto = new_prog.detailed_results.val_aggregate_scores
best_outputs = new_prog.detailed_results.best_outputs_valset
```

---

## 6. Adapter 自定义

> 大多数场景用 `optimize_anything` 就够了。Adapter 用于需要完全控制的高级场景。

### GEPAAdapter 协议

```python
from gepa.core.adapter import GEPAAdapter, EvaluationBatch

class MyAdapter(GEPAAdapter):
    def evaluate(self, batch, candidate, capture_traces=False):
        """执行系统，返回 EvaluationBatch(outputs, scores, trajectories)"""
        ...

    def make_reflective_dataset(self, candidate, eval_batch, components_to_update):
        """构建反思数据集：{component_name: [{"Inputs": ..., "Generated Outputs": ..., "Feedback": ...}]}"""
        ...
```

### 完整示例：QA Adapter

```python
from dataclasses import dataclass
import litellm
from gepa.core.adapter import GEPAAdapter, EvaluationBatch

@dataclass
class QAInput:
    question: str
    answer: str

@dataclass
class QATrace:
    prompt: str
    response: str

@dataclass
class QAOutput:
    answer: str

class SimpleQAAdapter(GEPAAdapter):
    def __init__(self, model="openai/gpt-4o-mini"):
        self.model = model

    def evaluate(self, batch, candidate, capture_traces=False):
        outputs, scores = [], []
        trajectories = [] if capture_traces else None

        for item in batch:
            prompt = f"{candidate['system_prompt']}\n\nQuestion: {item.question}"
            response = litellm.completion(
                model=self.model,
                messages=[{"role": "user", "content": prompt}],
            )
            answer = response.choices[0].message.content
            output = QAOutput(answer=answer)
            score = 1.0 if item.answer.lower() in answer.lower() else 0.0

            outputs.append(output)
            scores.append(score)
            if capture_traces:
                trajectories.append(QATrace(prompt=prompt, response=answer))

        return EvaluationBatch(outputs=outputs, scores=scores, trajectories=trajectories)

    def make_reflective_dataset(self, candidate, eval_batch, components_to_update):
        dataset = {"system_prompt": []}
        for i, trace in enumerate(eval_batch.trajectories or []):
            dataset["system_prompt"].append({
                "Inputs": {"question": trace.prompt.split("Question: ")[-1]},
                "Generated Outputs": {"answer": trace.response},
                "Feedback": f"Score: {eval_batch.scores[i]}",
            })
        return dataset

# 使用
result = gepa.optimize(
    seed_candidate={"system_prompt": "Answer questions accurately."},
    trainset=trainset,
    adapter=SimpleQAAdapter(),
    reflection_lm="openai/gpt-4o",
    max_metric_calls=50,
)
```

### 内置 Adapter 一览

| Adapter | 用途 |
|---------|------|
| `DefaultAdapter` | 通用 prompt 优化（LLM 单轮任务） |
| `ConfidenceAdapter` | 结构化分类 + logprob 置信度 |
| `DSPy Adapter` | DSPy 程序指令优化 |
| `DSPy Full Program Adapter` | 整个 DSPy 程序结构进化 |
| `RAG Adapter` | RAG 管道组件优化 |
| `MCP Adapter` | MCP 工具描述与 system prompt 优化 |
| `TerminalBench Adapter` | 终端/CLI Agent 优化 |

---

## 7. 高级特性

### 7.1 自定义 Instruction Proposer

默认 proposer 的模板：

```
I provided an assistant with the following instructions:
```<curr_param>```

The following are examples with the assistant's response and feedback:
```<side_info>```

Write a new instruction for the assistant.
Read the inputs carefully and identify the input format and infer detailed task description.
Read all responses and feedback. Identify niche and domain specific factual information.
Provide the new instructions within ``` blocks.
```

#### 自定义示例：带字数限制的 Proposer

```python
import dspy
from gepa.core.adapter import ProposalFn

class GenerateWordLimitedInstruction(dspy.Signature):
    """Generate improved instruction with word limit."""
    current_instruction = dspy.InputField()
    feedback_summary = dspy.InputField()
    max_words = dspy.InputField()
    improved_instruction = dspy.OutputField()

class WordLimitProposer(ProposalFn):
    def __init__(self, max_words=1000):
        self.max_words = max_words
        self.improver = dspy.ChainOfThought(GenerateWordLimitedInstruction)

    def __call__(self, candidate, reflective_dataset, components_to_update):
        updated = {}
        for name in components_to_update:
            if name not in candidate or name not in reflective_dataset:
                continue
            feedback = "\n".join(
                f"Example {i+1}: {ex.get('Feedback', '')}"
                for i, ex in enumerate(reflective_dataset[name])
            )
            result = self.improver(
                current_instruction=candidate[name],
                feedback_summary=feedback,
                max_words=self.max_words,
            )
            updated[name] = result.improved_instruction
        return updated

gepa = dspy.GEPA(
    metric=metric,
    reflection_lm=dspy.LM("openai/gpt-5", temperature=1.0, max_tokens=32000),
    instruction_proposer=WordLimitProposer(max_words=700),
    auto="medium",
)
```

#### 自定义示例：RAG 增强 Proposer

```python
class DocumentationEnhancedProposer(ProposalFn):
    """从专业文档库检索相关指南来增强指令。"""
    def __init__(self, retriever):
        self.improver = RAGInstructionImprover(retriever)

    def __call__(self, candidate, reflective_dataset, components_to_update):
        updated = {}
        for name in components_to_update:
            result = self.improver(
                current_instruction=candidate[name],
                component_examples=reflective_dataset[name],
            )
            updated[name] = result.improved_instruction
        return updated

gepa = dspy.GEPA(
    metric=metric,
    reflection_lm=dspy.LM("openai/gpt-5", temperature=1.0, max_tokens=32000),
    instruction_proposer=DocumentationEnhancedProposer(chroma_collection),
    auto="medium",
)
```

### 7.2 Component Selector

控制每轮优化哪些组件：

```python
# 内置策略
component_selector="round_robin"  # 默认：轮流单组件
component_selector="all"           # 同时优化所有组件

# 自定义
def alternating_half_selector(state, trajectories, subsample_scores, candidate_idx, candidate):
    """奇偶轮交替优化前/后半组件。"""
    components = list(candidate.keys())
    if len(components) <= 1:
        return components
    mid = len(components) // 2
    return components[:mid] if state.i % 2 == 0 else components[mid:]

gepa = dspy.GEPA(
    metric=metric,
    reflection_lm=reflection_lm,
    component_selector=alternating_half_selector,
    auto="medium",
)
```

### 7.3 Stop Conditions

```python
from gepa import TimeoutStopCondition, NoImprovementStopper

result = gepa.optimize(
    ...,
    max_metric_calls=100,
    stop_callbacks=[
        TimeoutStopCondition(timeout_seconds=3600),
        NoImprovementStopper(max_iterations_without_improvement=10),
    ],
)
```

### 7.4 日志与追踪

```python
result = gepa.optimize(
    ...,
    use_wandb=True,
    use_mlflow=True,
    run_dir="./gepa_runs/exp1",  # 断点续跑
    display_progress_bar=True,
)
```

---

## 8. gskill：学习仓库级编码技能

gskill 是 GEPA 在编码领域的应用——从 GitHub 仓库自动学习 bug-fixing 技能。

### 工作流

1. **SWE-smith 生成任务**：从目标仓库挖掘 commit → 引入 bug → 产出可验证任务（问题描述 + Docker 环境 + 测试）
2. **GEPA 优化技能**：空技能启动 → Agent 在 Docker 容器中批量执行 → pass/fail + Agent 轨迹 + 测试输出 → 反思模型提出更好技能 → 迭代
3. **部署**：输出 `best_skills.txt`，注入 Agent system prompt（支持跨模型迁移——便宜模型训练，贵模型使用）

### 快速开始

```bash
# 安装
pip install gepa[gskill]
pip install mini-swe-agent swebench
export OPENAI_API_KEY=<your-key>

# 冒烟测试
python -m gepa.gskill.train_optimize_anything \
  --smoke-test --model "gpt-5-mini"

# 完整训练
python -m gepa.gskill.train_optimize_anything \
  --repo pygments__pygments \
  --train-size 200 --val-size 50 --test-size 100 \
  --model gpt-5-mini --reflection-model gpt-5.2-pro \
  --workers 6 --max-metric-calls 600 \
  --proposer loop --wandb
```

### Fitness 函数签名

```python
def fitness_fn(candidate: dict[str, str], example: dict[str, Any]) -> tuple[float, dict]:
    """
    candidate: {"skills": "..."} — 可优化的文本参数
    example: 单个任务实例

    返回 (score, side_info)：
      score: 越高越好（1.0 通过，0.0 失败）
      side_info: 反思模型的诊断输入
    """
    skills = candidate["skills"]
    harness = get_harness()

    harness.setup_task(example)
    patch, trace, metrics = harness.run_agent(example["problem_statement"], skills)
    passed, test_output = harness.verify_with_patch(patch)

    return (
        1.0 if passed else 0.0,
        {
            "Input": {"Problem": example["problem_statement"][:200]},
            "Generated Outputs": {"Patch": patch[:500], "Agent Trace": trace},
            "Feedback": {"Status": "passed" if passed else "failed", "Test Output": test_output},
            "scores": {"correctness": 1.0 if passed else 0.0},
        },
    )
```

### 评估

```bash
# Mini-SWE-agent 评估
python -m gepa.gskill.gskill.evaluate.mini_swe_agent \
  --config gepa_results/logs/run_xxx/config.json --workers 16

# Claude Code 评估（带/不带技能）
python -m gepa.gskill.gskill.evaluate.claude_code \
  --config gepa_results/logs/run_xxx/config.json \
  --model haiku --workers 4 --use-skills
```

---

## 9. 自演化 Agent 模式

> 来源：[OpenAI Cookbook — Self-Evolving Agents](https://developers.openai.com/cookbook/examples/partners/self_evolving_agents/autonomous_agent_retraining)

### 自演化循环

```
基线 Agent → 评估（人工/LLM-as-judge）→ 反馈聚合 → Prompt 优化 → 更新 Agent → 重复
```

### 结合 OpenAI Evals + GEPA

```python
from openai import OpenAI

# 1. 创建 eval（4 个 grader：化学名、长度、余弦相似度、LLM judge）
eval = client.evals.create(name="self_evolving_eval", ...)

# 2. 运行 eval
eval_run = run_eval(eval_id, section=section, summary=summary)
grader_scores = parse_eval_run_output(poll_eval_run(eval_id, eval_run.id))

# 3. 反馈驱动的 prompt 优化循环
for section in dataset:
    for attempt in range(MAX_RETRIES):
        summary = await Runner.run(summarization_agent, section)
        scores = await get_eval_grader_score(eval_id, summary, section)
        avg_score = calculate_grader_score(scores)

        if is_lenient_pass(scores, avg_score):
            break

        # 未通过 → metaprompt 优化
        feedback = collect_grader_feedback(scores)
        improved = await Runner.run(metaprompt_agent, TEMPLATE.format(
            original_prompt=current_prompt,
            section=section,
            summary=summary,
            reasoning=feedback,
        ))
        prompt_versions.update(improved)
        summarization_agent = make_agent(prompt_versions.current())
```

### Versioned Prompt 管理

```python
class VersionedPrompt:
    """追踪 prompt 版本历史，支持回滚。"""
    def __init__(self, initial_prompt, model="gpt-5"):
        self._versions = [PromptVersionEntry(version=0, prompt=initial_prompt, model=model)]

    def update(self, new_prompt, model="gpt-5", metadata=None):
        version = self.current().version + 1
        self._versions.append(PromptVersionEntry(version=version, prompt=new_prompt, model=model))

    def current(self):
        return self._versions[-1]

    def revert_to_version(self, version):
        idx = next(i for i, e in enumerate(self._versions) if e.version == version)
        self._versions = self._versions[:idx + 1]
        return self._versions[-1]
```

---

## 10. 与 Astro GEPA-lite 的对照

Astro 的 GEPA-lite（见 [gepa-lite-design.md](../gepa-lite-design.md)）是面向技能进化的 Rust 原生实现。与完整 GEPA 库的对应关系：

| GEPA 概念 | Astro GEPA-lite 对应 |
|-----------|---------------------|
| `reflection_lm` | `evolution.reflection` 路由 |
| `task_lm` | 被优化技能运行时的主模型（不在进化内调用） |
| Pareto frontier | `pareto_front()` + `select_front_capped()` + `select_population()` |
| Reflective Mutation | `build_mutation_prompt()` + `MUTATION_SYSTEM_PROMPT` |
| System-Aware Merge | `build_crossover_prompt()` + `CROSSOVER_SYSTEM_PROMPT` |
| ASI / feedback | `EvalJudgement { satisfied, unmet }` + `aggregate_critiques()` |
| Evaluation | `fitness_score()` + `weighted_eval_score()`（Fail 双权重） |
| Holdout split | `split_eval_examples()` optimize/holdout 划分 |
| Sandbox execution | `sandbox_test_candidate()` → `TestOutcome` → 三维 Pareto |
| Budget / stop | `SearchBudget::try_reserve_one()` + `max_llm_calls` |
| Curator (Hermes-style) | `run_curator()` + `find_overlap_clusters()` + `CurateSuggestion::Merge` |
| `log_dir` / checkpoint | `history.jsonl` + `SearchRunMeta` |
| `optimize_anything` | 无直接对应（Astro 进化专精于 Skill SKILL.md） |

### 主要差异

- **GEPA 库**：Python、通用文本制品、DSPy 深度集成、`optimize_anything` 万能 API
- **Astro GEPA-lite**：Rust、专精技能 SKILL.md、LLM 调用外置（Tauri 注入）、人工审批不可绕过

### 可借鉴的方向

1. **`optimize_anything` evaluator 模式**：评估函数返回 `(score, side_info)` — Astro 的 `fitness_score` 已返回 `(score, reason, judgements)` 但可更结构化
2. **gskill 的 Docker 沙箱**：Astro 目前用 tempdir + shell 脚本，可扩展到容器级隔离
3. **Batch sampler / epoch shuffling**：Astro 搜索每代用全部 eval 例（受 `max_eval_examples` 限制），可引入 minibatch 随机采样减少过拟合
4. **自定义 ProposalFn**：Astro 的 `MUTATION_SYSTEM_PROMPT` 是固定模板，可开放为用户可注入的 Skill-level proposer

---

## 参考链接

- [GEPA 官网](https://gepa-ai.github.io/gepa/)
- [dspy.GEPA API](https://dspy.ai/api/optimizers/GEPA/overview/)
- [dspy.GEPA Advanced](https://dspy.ai/api/optimizers/GEPA/GEPA_Advanced/)
- [论文 arXiv:2507.19457](https://arxiv.org/abs/2507.19457)
- [GitHub: gepa-ai/gepa](https://github.com/gepa-ai/gepa)
- [OpenAI Cookbook: Self-Evolving Agents](https://developers.openai.com/cookbook/examples/partners/self_evolving_agents/autonomous_agent_retraining)
- [HuggingFace Cookbook: DSPy + GEPA](https://huggingface.co/learn/cookbook/en/dspy_gepa)
- [Astro GEPA-lite 设计文档](../gepa-lite-design.md)

```bibtex
@misc{agrawal2025gepareflectivepromptevolution,
    title={GEPA: Reflective Prompt Evolution Can Outperform Reinforcement Learning},
    author={Lakshya A Agrawal and Shangyin Tan and Dilara Soylu and Noah Ziems and
            Rishi Khare and Krista Opsahl-Ong and Arnav Singhvi and Herumb Shandilya and
            Michael J Ryan and Meng Jiang and Christopher Potts and Koushik Sen and
            Alexandros G. Dimakis and Ion Stoica and Dan Klein and Matei Zaharia and Omar Khattab},
    year={2025},
    eprint={2507.19457},
    archivePrefix={arXiv},
}
```
