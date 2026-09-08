# home

Astro 本机数据根（默认 `~/.astro` / `ASTRO_MEMORY_DIR`）的路径、初始化、配置写锁、
日志与工作区模板。不依赖 SQLite，也不在初始化时搬移旧数据。

## 路径入口

领域路径集中在 `layout.rs`，数据库路径在 `workspace/paths.rs`，统一从 `home` 导出。
业务模块使用 `providers_path`、`sessions_dir`、`session_db_path`、`cron_dir`、
`workflow_db_path`、`uploads_dir`、`security_audit_dir` 等函数，不重拼领域目录。
完整目录、迁移与历史保护契约见 [本机领域布局](../../docs/home-layout.md)。

- `default_memory_dir()`：解析本机数据根。
- `ensure_workspace_dirs(base)`：拒绝旧布局/未完成迁移，仅确保领域目录。
- `ensure_workspace(base)`：补齐缺失的默认状态和提示词，不覆盖已有个性化内容。
- `agent_config_dir(base, id)`：当前单专家模式的 `agents/default/` 运行时 JSON 配置。
- `agent_workspace_dir(base, id)`：工作区内容目录，包含人格、用户记忆、日记及生成物。
- `config_file::lock_config_file(path)`：跨进程配置写锁，必须持有到读改写完成。
- `config_file::write_config_file(path, text)`：在锁内执行私有临时文件写入、同步与替换。

## 其他职责

| 模块 | 职责 |
| --- | --- |
| `workspace/agent_config.rs` | `AgentRuntimeConfig` 序列化及持久化 |
| `workspace/lifecycle.rs` | 工作区模板和 Agent 生命周期 |
| `workspace/templates.rs` | SOUL / USER / AGENTS / TOOLS 等模板 |
| `config/tools_enabled.rs` | `tools/enabled.json` 全局默认值与 Agent 覆写 |
| `config/agent_icons.rs` | Agent 图标与 `agents/pending-icons/` 暂存 |
| `infra/logging.rs`、`infra/log_query.rs` | 按日运行日志与结构化查询 |
| `infra/tool_calls.rs` | `security/audit/agents/{id}/tool-calls.jsonl` |
| `test_env.rs` | `AstroMemoryDirGuard` 串行覆盖并恢复测试数据根 |

项目规则读取只使用项目根 `AGENTS.md`，不得为了读取规则创建项目 `.astro/`。
项目显式配置与全局数据目录是不同作用域。

## 验证

```bash
cargo test -p home
```
