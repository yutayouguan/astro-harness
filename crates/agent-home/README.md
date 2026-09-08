# home

Astro 本机根（默认 `~/.astro` / `ASTRO_MEMORY_DIR`）：路径解析、日志、Agent 配置层（图标 / 工具开关 / 内容扫描）。刻意不引入 SQLite 依赖，供轻量 crate 直接使用。

## 核心职责

- **路径约定** -- 统一解析 `~/.astro/` 下的 agents / sessions / cron / workflows / teams 等目录结构，跨平台（macOS / Windows / Linux）
- **Agent 生命周期** -- 管理身份文件与 `config.toml [desktop.agents.<id>]` 默认设置
- **工具开关** -- `config.toml [desktop.tools]` 热加载，支持 per-agent 覆盖、toolset 粒度启停、`is_tool_call_allowed()` 实时校验
- **统一设置** -- `settings` 提供分段读写、跨进程锁、原子替换与显式 JSON 迁移；说明见 [全局设置](../../docs/global-settings.md)
- **Agent 图标** -- 多种图标来源（Lucide / 自定义 SVG / 待确认队列），自动 Lucide 图标推荐
- **日志基础设施** -- `init_logging()` 初始化 tracing-subscriber + 文件 appender，`query_agent_logs()` 结构化日志查询
- **内容扫描** -- `scan_memory_content()` 扫描 MEMORY.md 等文件内容
- **模板脚手架** -- 工作区 Markdown 与全局 TOML Agent 默认段初始化，不再生成可编辑 JSON 副本
- **测试工具** -- `AstroMemoryDirGuard` 安全地串行化环境变量覆盖

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | 顶层 re-exports |
| `workspace/paths.rs` | 路径解析核心：`astro_data_root()` / `agent_dir()` / `sessions_dir()` / `user_home_dir()` / `user_downloads_dir()`，Agent ID 生成（slug + hex） |
| `workspace/lifecycle.rs` | Agent 生命周期（26KB）：创建/删除/列表/切换/重命名，`active_agent_id()` / `set_active_agent()` |
| `workspace/agent_config.rs` | `AgentRuntimeConfig` -- Agent 运行时配置 JSON 的序列化结构 |
| `workspace/templates.rs` | 模板脚手架：SOUL.md / MEMORY.md / USER.md / config.json 默认内容生成 |
| `workspace/generated.rs` | 生成式配置 -- Agent 自动生成的元数据管理 |
| `config/tools_enabled.rs` | 工具开关：加载/保存 `tools_enabled.json`、toolset 默认同步、`is_tool_call_allowed()` / `is_toolset_enabled()` |
| `config/agent_icons.rs` | Agent 图标管理：写入/读取/待确认队列、`resolve_icon_field()` |
| `config/auto_icon.rs` | Lucide 自动图标：`suggest_lucide_icon_id()` / `apply_auto_lucide_icon()` / SVG 字节查找 |
| `config/scan.rs` | 内容安全扫描：`scan_memory_content()` |
| `infra/logging.rs` | `init_logging()` / `logs_dir()` -- tracing 初始化与日志目录 |
| `infra/log_query.rs` | `query_agent_logs()` / `AgentLogQuery` / `AgentLogLine` -- 结构化日志查询 |
| `infra/tool_calls.rs` | `record_tool_call()` -- 工具调用记录 |
| `test_env.rs` | `AstroMemoryDirGuard` -- 测试环境安全串行化 `ASTRO_MEMORY_DIR` |

## 核心类型与 API

- `astro_data_root()` -- 解析数据根目录（`ASTRO_MEMORY_DIR` > `~/.astro`）
- `agent_dir(agent_id)` -- 单个 Agent 的工作区目录
- `DEFAULT_AGENT_ID` -- 默认 Agent ID（`"default"`）
- `AgentRuntimeConfig` -- Agent 运行时配置结构体
- `load_tools_enabled()` / `save_tools_enabled()` -- 工具开关 JSON 读写
- `is_tool_call_allowed(name)` -- 判断工具调用是否被允许
- `KNOWN_TOOLSET_IDS` -- 已知 toolset ID 列表
- `AstroMemoryDirGuard` -- 测试辅助：RAII 式环境变量覆盖与恢复

## Crate 关系

| 方向 | crate |
|------|-------|
| 无内部依赖 | 本 crate 不依赖 workspace 内其他 crate（刻意避免 rusqlite） |
| 被依赖 | `mcp`、`skills`、`agent`、`tools`、`memory`、`server` 等几乎所有 crate 均依赖本 crate 获取路径和配置 |

## 测试

```bash
# 全部测试（单元测试分布在各源文件的 #[cfg(test)] 中）
cargo test -p home

# 单个测试
cargo test -p home test_slug_generation -- --nocapture
```
