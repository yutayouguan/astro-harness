"""评测集 grounded 打分：与 Astro Rust 侧同口径（0~1）。"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, field
from typing import List


@dataclass
class EvalExample:
    task: str
    expectations: List[str] = field(default_factory=list)
    verdict: str = "fail"
    skill_id: str | None = None


def load_evalset(path: str) -> List[EvalExample]:
    """读取 evalset.jsonl；跳过坏行。"""
    out: List[EvalExample] = []
    try:
        with open(path, "r", encoding="utf-8") as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    d = json.loads(line)
                except json.JSONDecodeError:
                    continue
                out.append(
                    EvalExample(
                        task=d.get("task", ""),
                        expectations=list(d.get("expectations", []) or []),
                        verdict=d.get("verdict", "fail"),
                        skill_id=d.get("skill_id"),
                    )
                )
    except FileNotFoundError:
        pass
    return out


EVAL_JUDGE_SYSTEM = (
    "你是技能评测器。给定技能内容、任务与期望要点，判断若用该技能执行此任务能在多大程度上满足期望。"
    "只输出 JSON：{\"score\":0.0}。score 为 0~1。"
)


def _parse_score(raw: str) -> float:
    m = re.search(r"\{.*\}", raw, re.S)
    if not m:
        return 0.0
    try:
        d = json.loads(m.group(0))
        return max(0.0, min(1.0, float(d.get("score", 0.0))))
    except (json.JSONDecodeError, TypeError, ValueError):
        return 0.0


def score_skill(lm, skill_content: str, examples: List[EvalExample]) -> float:
    """对每个例子让 LM 打分取均值；无例子返回 0.5（中性）。

    `lm` 为可调用对象：lm(prompt) -> str（见 __main__ 的适配）。
    """
    if not examples:
        return 0.5
    total = 0.0
    n = 0
    for ex in examples:
        exp = "\n".join(f"- {e}" for e in ex.expectations) or "（按任务合理判断）"
        prompt = (
            f"{EVAL_JUDGE_SYSTEM}\n\n## 技能内容\n{skill_content}\n\n"
            f"## 任务\n{ex.task}\n\n## 期望要点\n{exp}\n\n只输出 JSON。"
        )
        try:
            raw = lm(prompt)
        except Exception:  # noqa: BLE001 - 单例失败不应中断整体评分
            continue
        total += _parse_score(raw)
        n += 1
    return (total / n) if n else 0.5
