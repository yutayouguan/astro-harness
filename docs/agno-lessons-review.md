# Agno 课时全循环 Review

验收日期：2026-07-17（第二轮复审）  
对照文档：[`docs/agno-lessons.md`](agno-lessons.md)  
范围：§一–§十一竖切（文档「不必 / P4」项除外）  
归档 tag：`v-agno-lessons-complete`（首轮）；本轮修复另开 commit

## 测试快照（第二轮）

| Crate / 过滤 | 结果 |
|--------------|------|
| agent --lib context_source / hitl | pass |
| agent --lib（全量） | 见下方复跑 |
| memory decision_log / protocol | pass |
| usage eval_export / sqlite_store | pass |
| artifacts content_db | pass |
| tools skill_override | pass |
| skills parse_astro_tools | pass |

## 问题清单

| ID | 严重度 | 问题 | 方案 | 状态 |
|----|--------|------|------|------|
| R1 | 中 | 并发工具失败路径未写 DecisionLog | `run_tool_on_snapshot` 写 DecisionLog | **已修**（首轮） |
| R9 | **高** | §四把 `pending_inject_context` 编进 `build_system_prompt`，与 `take_inject_context`→`[astro:hook-context]` user 消息**双重注入** | `build_system_prompt` 对 inject 传 `None`；协议层仍支持 inject 参数供测试 | **已修**（本轮） |
| R5 | 低 | Knowledge FTS `MATCH` 特殊字符易失败 | MATCH 失败回退 title/path `LIKE` | **已修**（本轮） |
| R2 | 中 | 用户附图未落 `messages.media_json`；hydrate 仅 tool sidecar | schema v15 + NewMessage/StoredMessage + hydrate | **已修**（本轮） |
| R3 | 低 | `ChatContentPart` 不支持 audio/video 入模 | Provider 原生多模态后再做 | **记债** |
| R4 | 低 | `SqliteStore` 仅 Example，未挂真实 UsageDb/KnowledgeDb | 各库缓存 path 后 `impl` | **记债** |
| R6 | 信息 | EntityMemory / Always / embedding / Postgres | 文档「不必 / P4」 | **不做** |
| R7 | 信息 | `search_context` / `pin_context` | §四未做项 | **记债** |
| R8 | 信息 | Team `tasks`、`is_exclusive_tool` 名称表 | 文档已声明 | **记债** |
| R10 | 低 | `assemble_from_sources` 分隔符在 contribute 之后扣预算，极限预算下分隔符与层切分略不精确 | 可先预留 sep 再 contribute | **记债** |
| R11 | 信息 | `skills` 加载后再次 `load_skill_by_name` 解析 `astro_tools`（双读磁盘） | 可让 dispatch 返回 toolsets 或缓存 | **记债** |

## 文档对齐

- §一–§九、§十一均有「已落地」；§十认定已完成。
- §四「已落地」已更正：inject **不**进 system。
- P0–P3 竖切齐全；P4 EntityMemory 未做。

## 修复迭代记录

1. R1：并发工具失败 → DecisionLog。
2. R9：去掉 system 侧 inject，恢复 hooks 单通道语义。
3. R5：Knowledge search FTS 失败回退 LIKE。
4. 其余记债不阻塞；建议下一刀优先 R2（用户附图 `media_json`）。
