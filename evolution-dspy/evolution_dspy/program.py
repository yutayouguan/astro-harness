"""DSPy 程序：给定当前技能与任务/期望，产出改进后的 SKILL.md。

DSPy API 随版本变化，这里对 dspy.Signature / dspy.Predict 做基本适配；
若不可用，调用方会走 __main__ 的纯 LM 回退路径。标注 `# ADAPT:` 处按你的版本调整。
"""

from __future__ import annotations

try:
    import dspy  # type: ignore

    class ImproveSkill(dspy.Signature):  # type: ignore[misc]
        """基于任务与期望要点，改进一个技能的 SKILL.md，使其更可复用、正确、简洁。"""

        current_skill = dspy.InputField(desc="当前 SKILL.md 全文")
        tasks = dspy.InputField(desc="该技能需覆盖的任务与期望要点")
        improved_skill = dspy.OutputField(desc="改进后的完整 SKILL.md")

    class SkillImprover(dspy.Module):  # type: ignore[misc]
        def __init__(self):
            super().__init__()
            self.step = dspy.Predict(ImproveSkill)  # ADAPT: 可换 ChainOfThought

        def forward(self, current_skill: str, tasks: str):
            return self.step(current_skill=current_skill, tasks=tasks)

    DSPY_AVAILABLE = True
except Exception:  # noqa: BLE001 - dspy 未安装/不兼容时优雅降级
    DSPY_AVAILABLE = False
    SkillImprover = None  # type: ignore


def tasks_blob(examples) -> str:
    """把评测例子拼成一段「任务 + 期望」文本，作为改进输入。"""
    if not examples:
        return "（无标注例子：请基于技能自身合理性做改进）"
    parts = []
    for ex in examples:
        exp = "; ".join(ex.expectations) if ex.expectations else "（无具体要点）"
        parts.append(f"- 任务：{ex.task}\n  期望：{exp}\n  历史结果：{ex.verdict}")
    return "\n".join(parts)
