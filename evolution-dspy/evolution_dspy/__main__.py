"""CLI: python -m evolution_dspy optimize --input <dir> --output <file>

读取 <dir>/{skill.md, evalset.jsonl, config.json}，用 DSPy(+GEPA) 优化技能，
写回 result.json。凭据经环境变量 ASTRO_DSPY_API_KEY。
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import traceback

from .metric import load_evalset, score_skill
from . import program as prog


def _configure_lm(model: str, base_url: str):
    """配置并返回一个 `lm(prompt)->str` 可调用；同时尽力 dspy.configure。

    ADAPT: 依 DSPy 版本调整 dspy.LM 的参数名（api_base/base_url/api_key）。
    """
    api_key = os.environ.get("ASTRO_DSPY_API_KEY", "")
    import dspy  # type: ignore

    # OpenAI 兼容端点：model 形如 "openai/<model>"
    lm = dspy.LM(  # ADAPT
        model=f"openai/{model}",
        api_base=base_url or None,
        api_key=api_key or "sk-none",
        temperature=0.4,
        max_tokens=4096,
    )
    dspy.configure(lm=lm)

    def call(prompt: str) -> str:
        out = lm(prompt)  # dspy.LM 可直接调用，返回 list[str] 或 str
        if isinstance(out, list):
            return out[0] if out else ""
        return str(out)

    return lm, call


def _read_config(input_dir: str) -> dict:
    with open(os.path.join(input_dir, "config.json"), "r", encoding="utf-8") as f:
        return json.load(f)


def _read_skill(input_dir: str) -> str:
    try:
        with open(os.path.join(input_dir, "skill.md"), "r", encoding="utf-8") as f:
            return f.read()
    except FileNotFoundError:
        return ""


def _mock_optimize(input_dir: str) -> dict:
    """不依赖 dspy 的确定性产物：验证 Rust↔Python↔提案 的 JSON 契约。"""
    cfg = _read_config(input_dir)
    skill_id = cfg.get("skill_id", "")
    current = _read_skill(input_dir)
    examples = load_evalset(os.path.join(input_dir, "evalset.jsonl"))
    marker = "\n\n<!-- evolved: mock -->\n"
    content = current if current.endswith(marker) else (current + marker)
    return {
        "skill_id": skill_id,
        "kind": "edit",
        "content": content,
        "score": 0.5,
        "rationale": "mock 产物（未调用真实 dspy）",
        "log": f"mock skill_id={skill_id} examples={len(examples)}",
    }


def _optimize(input_dir: str) -> dict:
    cfg = _read_config(input_dir)
    skill_id = cfg.get("skill_id", "")
    model = cfg.get("model", "")
    base_url = cfg.get("base_url", "")
    current = _read_skill(input_dir)
    examples = load_evalset(os.path.join(input_dir, "evalset.jsonl"))
    log_lines = [f"skill_id={skill_id} model={model} examples={len(examples)}"]

    if not prog.DSPY_AVAILABLE:
        raise RuntimeError("dspy 未安装或不兼容；请 pip install 'dspy-ai>=2.5'")

    lm, call = _configure_lm(model, base_url)
    tasks = prog.tasks_blob(examples)

    candidates: list[str] = []

    # 1) 首选 GEPA 优化（不同版本入口不同，做多路尝试；失败回退单轮）
    improver = prog.SkillImprover()
    try:
        import dspy  # type: ignore

        gepa = None
        if hasattr(dspy, "GEPA"):
            # ADAPT: 版本差异，metric 签名可能不同
            def _metric(example, pred, trace=None):  # noqa: ANN001
                content = getattr(pred, "improved_skill", "") or ""
                return score_skill(call, content, examples)

            gepa = dspy.GEPA(metric=_metric, auto="light")  # ADAPT
        if gepa is not None:
            trainset = [
                dspy.Example(current_skill=current, tasks=tasks).with_inputs(
                    "current_skill", "tasks"
                )
            ]
            compiled = gepa.compile(improver, trainset=trainset)
            pred = compiled(current_skill=current, tasks=tasks)
            candidates.append(getattr(pred, "improved_skill", "") or "")
            log_lines.append("GEPA 优化完成")
        else:
            log_lines.append("dspy.GEPA 不可用，走单轮回退")
    except Exception as e:  # noqa: BLE001
        log_lines.append(f"GEPA 失败，回退单轮：{e}")

    # 2) 单轮反思式改写（回退 / 兜底一个候选）
    try:
        pred = improver(current_skill=current, tasks=tasks)
        candidates.append(getattr(pred, "improved_skill", "") or "")
    except Exception as e:  # noqa: BLE001
        log_lines.append(f"单轮改写失败：{e}")

    candidates = [c.strip() for c in candidates if c and c.strip()]
    if not candidates:
        raise RuntimeError("未产出任何候选")

    # 选评测集得分最高的候选
    best, best_score = current, -1.0
    for c in candidates:
        s = score_skill(call, c, examples)
        if s > best_score:
            best, best_score = c, s
    log_lines.append(f"best_score={best_score:.3f}")

    return {
        "skill_id": skill_id,
        "kind": "edit",
        "content": best,
        "score": round(max(0.0, best_score), 4),
        "rationale": "DSPy+GEPA 优化产物" if best_score >= 0 else "",
        "log": "\n".join(log_lines),
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="evolution_dspy")
    sub = parser.add_subparsers(dest="cmd", required=True)
    opt = sub.add_parser("optimize")
    opt.add_argument("--input", required=True)
    opt.add_argument("--output", required=True)
    opt.add_argument(
        "--mock",
        action="store_true",
        help="不调用 dspy，产确定性候选（用于契约自测）",
    )
    args = parser.parse_args(argv)

    if args.cmd == "optimize":
        try:
            result = _mock_optimize(args.input) if args.mock else _optimize(args.input)
        except Exception as e:  # noqa: BLE001
            result = {"error": str(e), "trace": traceback.format_exc()}
            with open(args.output, "w", encoding="utf-8") as f:
                json.dump(result, f, ensure_ascii=False, indent=2)
            print(result["error"], file=sys.stderr)
            return 1
        with open(args.output, "w", encoding="utf-8") as f:
            json.dump(result, f, ensure_ascii=False, indent=2)
        return 0
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
