# Agno 课时全循环 Review

验收日期：2026-07-17  
对照文档：[`docs/agno-lessons.md`](agno-lessons.md)  
范围：§一–§十一竖切（文档「不必 / P4」项除外）

## 测试快照

| Crate | 结果 |
|-------|------|
| agent --lib | 71 passed |
| memory --lib | 45 passed |
| usage --lib | 13 passed |
| tools registry | 4 passed |
| artifacts --lib | 3 passed |
| skills parse_astro_tools | 1 passed |
| cargo check (agent/tools/memory/usage/artifacts/skills/session) | ok |

## 问题清单

| ID | 严重度 | 问题 | 方案 | 状态 |
|----|--------|------|------|------|
| R1 | 中 | 并发工具失败路径未写 DecisionLog（仅串行路径写） | `run_tool_on_snapshot` 失败分支同样 `try_append_decision` | **已修** |
| R2 | 中 | 用户附图未落 `messages.media_json`；hydrate 仅 tool sidecar | schema v15 + NewMessage/StoredMessage + hydrate | **记债**（文档 §一未做） |
| R3 | 低 | `ChatContentPart` 不支持 audio/video 入模 | Provider 原生多模态后再做 | **记债**（文档明确不做本轮） |
| R4 | 低 | `SqliteStore` 未挂到真实 `UsageDb`/`KnowledgeDb`（仅 Example） | 后续给各库 `impl SqliteStore` 并缓存 path | **记债** |
| R5 | 低 | Knowledge FTS `MATCH` 对特殊字符查询可能失败 | 查询前 sanitize / 回退 LIKE | **记债** |
| R6 | 信息 | EntityMemory / Always / embedding / Postgres | 文档「不必 / P4」 | **不做** |
| R7 | 信息 | `search_context` / `pin_context` | §四未做项 | **记债** |
| R8 | 信息 | Team `tasks` 模式、`is_exclusive_tool` 仍名称表 | 文档已声明 | **记债** |

## 文档对齐

- §一–§九、§十一均有「已落地」；§十文档认定已完成。
- 建议落地顺序 P0–P3 均已有对应竖切；P4 EntityMemory 未做。

## 修复迭代记录

1. R1：并发工具失败写入 DecisionLog（本轮已合入）。
2. 其余记债项不阻塞 tag；后续按严重度排期。
