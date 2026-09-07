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
| usage eval_export / types::sqlite | pass |
| artifacts content_db | pass |
| tools skill_override | pass |
| skills parse_astro_tools | pass |

## 问题清单

| ID | 严重度 | 问题 | 方案 | 状态 |
|----|--------|------|------|------|
| R1 | 中 | 并发工具失败路径未写 DecisionLog | `run_tool_on_snapshot` 写 DecisionLog | **已修**（首轮） |
| R9 | **高** | §四把 `pending_inject_context` 编进 `build_system_prompt`，与 `take_inject_context`→`[astro:hook-context]` user 消息**双重注入** | `build_system_prompt` 对 inject 传 `None`；协议层仍支持 inject 参数供测试 | **已修**（本轮） |
| R5 | 低 | Knowledge FTS `MATCH` 特殊字符易失败 | MATCH 失败回退 title/path `LIKE` | **已修**（本轮） |
| R2 | 中 | 用户附图需参与持久化和 hydrate | schema v22 直接保存 `ResponseItem::Message` 的 `InputImage` | **已修**（已由 Responses 原生链路取代） |
| R3 | 低 | `ChatContentPart` 不支持 audio/video 入模 | `AudioUrl`/`VideoUrl` + Gemini inlineData；其它厂商文本回落 | **已修**（本轮） |
| R4 | 低 | `SqliteStore` 仅 Example，未挂真实库 | 协议下沉 `types::sqlite`；Usage/Session/Knowledge/Artifact/Cron/Orchestration 均已 `impl` | **已修** |
| R6 | 信息 | EntityMemory / Always / embedding / Postgres | 文档「不必 / P4」 | **不做** |
| R7 | 信息 | `search_context` / `pin_context` | `context_tools` + pinned 注入 Dynamic | **已修**（本轮） |
| R8 | 信息 | Team `tasks`、`is_exclusive_tool` 名称表 | `tasks` 串行共享任务板；`exclusive_access` 进 ToolEntry | **已修**（本轮） |
| R10 | 低 | `assemble_from_sources` 分隔符在 contribute 之后扣预算，极限预算下分隔符与层切分略不精确 | 先预留 sep（+1）再 contribute，空层 refund | **已修**（本轮） |
| R11 | 信息 | `skills` 加载后再次 `load_skill_by_name` 解析 `astro_tools`（双读磁盘） | `recent_astro_tools` 5s 窗口复用；激活路径优先命中 | **已修**（本轮） |

## 文档对齐

- §一–§九、§十一均有「已落地」；§十认定已完成。
- §四「已落地」已更正：inject **不**进 system。
- P0–P3 竖切齐全；P4 EntityMemory 未做。

## 修复迭代记录

1. R1：并发工具失败 → DecisionLog。
2. R9：去掉 system 侧 inject，恢复 hooks 单通道语义。
3. R5：Knowledge search FTS 失败回退 LIKE。
4. 其余记债不阻塞；建议下一刀优先 R2（用户附图 `media_json`）。
5. R2：session schema v15 `media_json`；`run_turn_with_images` / tool 结果落盘；hydrate 优先读列。
6. R4：`UsageDb` 增加 `path` 字段与 `SqliteStore` 实现；`db_path()` 公开路径。
7. R11：`skills::recent_astro_tools` + `activate_skill_toolsets_from_args` 复用刚加载结果，避免双读磁盘。
8. R10：`assemble_from_sources` 先预留分隔符再 contribute，空层 refund。
9. R7：`search_context` / `pin_context` 工具；pinned 写入 workspace 并注入 Dynamic。
10. R8：`team_run` tasks 串行共享任务板；`ToolEntry.exclusive_access` 替代硬编码表。
11. R3：`ChatContentPart` Audio/Video；Gemini 入模，OpenAI/Anthropic 文本回落；`Message.media` 并入 parts。
12. R4 收尾：`SqliteStore` 下沉 `types::sqlite`；Usage/Session/Knowledge/Artifact/Cron/Orchestration 全量 `impl`；删除 `usage::sqlite_store`。
13. Model 一等公民：`ModelSpec` / `ModelRole` + `AgentLoop::set_model`；Team/delegate 可选 `model`。

## 归档

- 记债已清（R6 与文档声明的「不必 / 刻意保留」项除外）。
- Tags：
  - `v-agno-lessons-complete`（首轮课时）
  - `v-agno-lessons-complete-r3`（含 R3 多模态入模）
  - `v-agno-lessons-final`（含 types::sqlite 统一与全量 Review 收尾）
- 关键测试（本机）：common 14 / usage 13 / artifacts 6 / agent 74 / tools 120 passed。
- **流程结束**：Agno 课时全循环 + Review 修复已锁定；后续 `ModelSpec` 为增量增强。
