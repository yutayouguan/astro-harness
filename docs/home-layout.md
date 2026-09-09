# Astro 本机数据的领域布局

运行时路径入口为 `crates/agent-home/src/layout.rs`，数据库路径由
`crates/agent-home/src/workspace/paths.rs` 提供，并一起从 `home` 顶层导出。
所有路径以显式传入的数据根解析；默认根由 `ASTRO_MEMORY_DIR` 或 `~/.astro` 决定。
业务模块不能拼接旧路径，不能加入旧路径读取回退。完整目录树见根 `AGENTS.md`。

## 所有权与初始化

| 领域 | 文件和路径 API |
| --- | --- |
| 全局配置 | `config.toml`；`config_path`、`config_file::lock_config_file` |
| Agent | `agents/active.json`、`agents/*.toml`；默认设置位于全局 TOML `desktop.agents` |
| 模型 | `models/cache/`；`models_cache_dir`；Provider 注册信息位于全局 TOML `desktop.providers` |
| MCP / Hook | 显式领域定义引用；`mcp/cache/`、`security/hooks/trust.json`、`logs/hooks/` 分离 |
| 工具与技能 | 全局 TOML `desktop.tools` / `desktop.skills`；`skills/` 内的包、来源、锁与备份 |
| 会话 | `sessions/state.db`、`rollouts/`、`tool_spills/`、`subagents/subagents-v2.db` |
| 文件与知识 | `artifacts/artifacts.db`、`knowledge.db`、`uploads/` |
| 用量 | `usage/usage.db`、`usage/agents/{id}/stats.json` |
| 自动化 | `automation/cron/` 和 `automation/workflows/`，定义、执行记录和输出放在一起 |
| 记忆 | `memory/dreaming.json`、待审批记忆；用户记忆正文仍在工作区 |
| 学习与进化 | `evolution/learning/`、进化评估与提议、`evolution/dspy/.venv` |
| 安全 | `security/audit/`（权限、沙箱、Agent 工具调用）与配置锁 |
| Desktop | `ui/`、`browser/`；浏览器登录数据不视为普通缓存 |

`home::ensure_workspace_dirs` 只确保领域目录，不打开 SQLite；数据库由所属领域按需创建。
`home::ensure_workspace` 补齐缺失的默认状态和工作区模板，但不覆盖已有个性化文件。
MCP 与记忆/权限设置写同一 TOML 时，锁必须覆盖完整的读取—修改—原子写入过程。
全局技能/工具默认值与 Agent 级覆写仍是不同语义，但统一写入同一 TOML 的不同段，不再初始化可编辑 JSON 副本。

MCP/Hook 可通过入口的 `config_sources` 显式引用领域文件；模型与 MCP 缓存策略、来源写回、
审批状态和离线迁移详见 [配置来源与缓存](config-sources.md)。缓存路径使用 `home::cache` 解析，
不能绕过策略直接拼接 `models_cache_dir` 写缓存。

项目工作规则只从项目根 `AGENTS.md` 加载；项目 `.astro/` 只用于显式项目配置。
读取规则、打开项目不得创建项目 `.astro/`，现有项目配置不能因本机目录调整被删除。

## 离线迁移

迁移是显式管理操作，不是启动回退。脚本适用于具备 `lsof` 的 macOS/Linux，
要求 Python 3.11+；旧根存在 `config.yaml` 时还需要 PyYAML。

```bash
# 只生成清单，不改文件
python3 tools/migrate_home_layout.py --root /absolute/path/to/astro-home

# 关闭所有使用该数据根的 Astro/浏览器/开发进程，审阅清单后显式执行
python3 tools/migrate_home_layout.py --root /absolute/path/to/astro-home --apply
```

脚本不会扫描或恢复废纸篓。请不要把废纸篓或整个用户目录传给 `--root`。
新安装不需要迁移。应用启动不会自动执行该脚本。

若存在 Provider、工具/Skill 开关或 Agent 默认设置的旧 JSON，先完成上述目录迁移，再执行 [全局设置迁移](global-settings.md)。目录迁移负责历史与资源位置，第二步负责配置格式；在第二步完成前，新设置读取会明确报错，不能启动后用默认值覆盖用户配置。

迁移步骤：预检目标及冲突 → 确认没有文件持有进程 → 私有完整 tar 备份 →
读取验证备份 → 写入进行中标记 → SQLite checkpoint → 写新位置 →
验证数据库完整性与行数 → 删除精确的旧文件与空目录 → 写完成记录。
备份位于 `backups/layout-v2-<timestamp>/`，含 `before.tar.gz`、`plan.json` 和完成记录。
`plan.json` 保存旧、新文件的映射。浏览器、工作区和凭证保留，未知文件与未解决的配置
冲突必须人工处理；不同的 dreaming 开关不会凭目录名或修改时间自动选择。

发生中断时保留 `backups/layout-in-progress.json`，运行时和脚本都会拒绝继续初始化。
检查其中的备份位置；恢复应在停机状态逐项对照清单完成，不能直接解压覆盖正在使用的
新数据，更不能先递归清空数据根。备份保留期限由用户决定，不自动过期删除。

## 历史保护

- canonical rollout 原始字节、空行、物理行号均不变。
- `response_items`、mailbox、消息正文、工具参数/原始输出、用户提示词不改写。
- 不扫描所有 SQL TEXT 列，不对 JSON 或正文做路径字符串替换。
- 唯一可改写的数据库资源字段是 `artifacts.path` 和 `contents.path`；且必须精确匹配
  本次实际搬迁的文件。知识正文、标题、未知表和未移动路径保持原样。
- 历史文本提到旧位置属于当时事实；需要定位已搬迁文件时查迁移清单，不能为了让旧
  命令看起来仍可执行而篡改历史。新回合只生成新布局的路径。
- 已有技能软链接指向旧 `.agents/skills` 时，先实化到 `skills/`，再移除旧入口。

## 验证

```bash
python3 -m unittest discover -s tools -p 'test_migrate_home_layout.py' -v
cargo test -p home -p memory -p skills -p cron -p usage -p artifacts --lib
cargo check --workspace --all-targets
```

测试必须使用独立临时数据根，禁止用真实 `~/.astro` 做清空、迁移或写入夹具。
验证覆盖新安装、再次启动、旧目录拒绝、TOML 跨领域保存、历史字节保持、资源字段迁移、
技能软链接、配置冲突、活动进程、备份与迁移中断标记。
