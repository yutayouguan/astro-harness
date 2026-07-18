# evolution-dspy

Astro 离线进化（Phase 3）的外部 DSPy + GEPA 优化器。**独立 Python 包**，不属于 cargo workspace；由 Astro 通过「临时目录 + JSON 文件」契约以子进程方式调用，产物回流到 Astro 的待审提案队列。

默认关闭（`config.yaml` 的 `evolution.dspy.enabled = false`）。需用户自备 Python 与依赖。

## 安装（推荐 venv 在用户数据目录）

```bash
python3 -m venv ~/.astro/evolution-dspy/.venv
~/.astro/evolution-dspy/.venv/bin/pip install -e <path-to>/evolution-dspy
# 若 DSPy 版本未内置 GEPA：
~/.astro/evolution-dspy/.venv/bin/pip install -e '<path-to>/evolution-dspy[gepa]'
```

`config.yaml`：
```yaml
evolution:
  dspy:
    enabled: true
    python_bin: "~/.astro/evolution-dspy/.venv/bin/python"
    project_path: "<path-to>/evolution-dspy"
    timeout_secs: 600
```

## 数据契约

Astro 在临时目录写入，然后运行：

```bash
python -m evolution_dspy optimize --input <dir> --output <dir>/result.json
```

输入（`<dir>/`）：
- `skill.md` — 目标技能当前 SKILL.md
- `evalset.jsonl` — 每行一个 `{skill_id?, task, expectations[], verdict}`（作为 trainset / 评分依据）
- `config.json` — `{skill_id, model, base_url, provider_backend}`

模型凭据经环境变量：`ASTRO_DSPY_API_KEY`。

输出（`result.json`）：
- `{skill_id, kind:"edit", content, score, rationale, log}`；失败时 `{error}` 且退出码非零。

## 版本说明

DSPy / GEPA API 随版本演进。本包对 `dspy.LM` / `dspy.GEPA` 做了适配并带回退（GEPA 不可用时退化为单轮反思式改写）。若你的 DSPy 版本 API 不同，调整 `program.py` / `__main__.py` 中标注 `# ADAPT:` 的位置。
