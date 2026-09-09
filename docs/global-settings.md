# 全局可编辑设置：TOML 与运行状态的边界

本次收口范围是此前仍散落在 JSON 中的四类可编辑设置。全局入口为 `~/.astro/config.toml`，尊重 `ASTRO_MEMORY_DIR`；桌面、Cron 与 Workflow 从同一个配置段读取 Provider 注册信息。

## 配置归属

| 数据 | 新的唯一读写位置 | 说明 |
|---|---|---|
| UI Provider 列表、端点、模型与活动选择 | `desktop.providers` | 维持原 Provider ID、fallback 与媒体模型；API key 仍由现有环境变量/安全存储解析 |
| 全局工具开关 | `desktop.tools` | 缺失项运行时默认启用；读取默认值不写盘 |
| 全局工具组加载策略 | `desktop.tool_loading` | `auto` / `always` / `on_demand`；缺失或 Auto 使用内置默认，不改变启用开关 |
| 全局 Skill 开关 | `desktop.skills` | 只持久化明确设置，不把扫描结果当用户配置写回 |
| Agent 默认设置 | `desktop.agents.<id>` | 包含名称、默认模型、温度、轮次上限与工具覆盖 |
| Agent Skill 覆盖 | `desktop.agent_skills.<id>` | 缺失时回退全局；默认 Agent 与全局开关在一次事务中同步 |
| 任务系统通知开关 | `desktop.notifications.enabled` | 缺失默认关闭；只改该字段，不在启动时申请系统权限；通知内容固定脱敏 |

磁盘中默认 Agent 的键固定为 `default`，不随运行时旧 ID `workspace` 的更名发生二次迁移。其他 Agent 名称经现有 ID 规范化函数处理。

登录启动由操作系统登录项（Tauri autostart）管理，不在 TOML 保存易失同步的副本。
桌宠继续使用 `ui/desktop-pet/state.json` 的共享事务与 revision；引导里的待应用开关只是
`ui/onboarding.json` 草稿，完成后才应用，未操作的 null 不覆盖既有偏好。

这些桌面设置属于机器本地配置，项目 `config.toml` 不能通过 `[desktop]` 替换 Provider 端点或开关。原有项目级模型/权限等受支持字段仍走既有分层契约；本次没有扩大项目权限。

示例（Provider 行中的 id/类型只是示例，实际写入保留已配置的 ID）：

```toml
[desktop]
settings_version = 1

[desktop.providers]
active_provider_id = "local"

[[desktop.providers.providers]]
id = "local"
kind = "ollama"
display_name = "Local"
endpoint = "http://127.0.0.1:11434/v1"
model = "example"
enabled = true
added = true

[desktop.tools]
exec_command = false

[desktop.skills]
"/absolute/path/to/skill" = false

[desktop.agents.default]
id = "default"
name = "Astro"
temperature = 0.7
max_turns = 90
additional_params_json = '{"vendor_option":null}'
```

高级 Provider 参数可能含 JSON null/数组，Agent 的 `additional_params_json` 用 JSON 字符串无损保存；不允许与原生 `additional_params` 表同时设置。未设置的 optional 字段省略，不用 TOML 不支持的 null。

## 工具组加载策略

桌面「工具 → 详情 → 工具组加载策略」可调整普通工具组的加载方式；这是全局设置，
适用于所有 Agent，与按 Agent 的启用开关分开。每次只保存一个工具组，失败时界面保留
最后确认的值并显示错误，不在首次读取时自动写盘。

```toml
[desktop.tool_loading]
browser = "on_demand"
memory = "always"
```

- `auto`：使用内置默认。Browser 的 16 个工具默认 Deferred；选择 Auto 时移除该组覆盖。
- `always`：启用且环境可用时，直接提供完整工具定义。
- `on_demand`：不进入首次普通工具请求，经 `tool_search` 发现后从下一 Step 起可调用。
  已发现工具保留在当前线程的发现历史中，不是每次调用后卸载。
- 所选模型不支持 `tool_search` 时，允许降级的工具会提前加载，且移除请求中的
  `defer_loading` 标记。界面显示该降级，能力未知时明确提示未知；模型目录
  CodeModeOnly 则显示 `exec` 间接调用。显示的是所选模型预期行为，会话模型与权限仍优先。
- 加载策略在下一次采样捕获 Step 时重新读取，不能修改已发请求的路由快照，也不绕过
  Plan、审批、沙箱、工具开关或 Skill 原有授权规则。
- 系统控制、隐藏、模型专用、freeform 及禁止 eager fallback 的工具不允许该设置改写；
  含此类工具的组不提供选择器。MCP/Workflow 仍使用各自的既有策略。

## 不迁入 config.toml 的内容

- `AGENTS.md`、`SOUL.md`、`IDENTITY.md`、`USER.md`、`MEMORY.md`：工作规则、人设及记忆，不改为配置参数。
- rollout、Session/usage 数据库、notes、任务执行记录：运行事实与工作状态。
- 模型列表缓存、工具/技能使用统计、下载缓存、界面引导完成状态：派生或运行数据。
- 已安装 Skill 文件、工作流定义、自定义 Agent 定义：独立资源，保留各自格式。
- 凭证：不复制到新配置段；现有安全存储/环境变量机制不变。

现有 `[mcp_servers]`、`[custom_providers]` 和其他 TOML 段保留原契约。没有特有配置的项目不创建 `.astro`；空 MCP 项目配置保存也是无操作。

## 读写与并发

实现位于 `agent-home/src/settings.rs`，路径委托 `home::layout`，锁和原子写入委托共享的 `home::config_file`：

1. 读取只解析 TOML，不创建目录，不回退旧 JSON。
2. 写入使用跨进程文件锁，锁覆盖读—修改—写全过程。
3. 每个调用方只更新所属配置段；MCP 写回也使用相同锁，保留其他段和注释。
4. 临时文件同步后原子替换，错误不覆盖原文件；不替换配置符号链接。
5. UI 工具开关和 Skill 切换在事务中合并，避免覆盖其他键。Agent 重命名只改名称字段。
6. 配置损坏或迁移未完成时显式报错；工具注册表拒绝调用，Skill 不能借 additive override 绕过此状态。Cron 在认领任务前验证配置，Workflow 不回退到无配置的默认 Provider。

## 显式迁移

先停止 Astro。若存在旧目录或 `config.yaml`，先按 [本机布局迁移](home-layout.md) 完成目录迁移。配置迁移的 `--apply` 会拒绝旧布局和未完成迁移标记；预览可列出旧输入，但不会写入任何文件：

```sh
cargo run -p home --bin astro-migrate-config -- --root /absolute/path/to/.astro
```

确认输出中的源文件范围后再执行：

```sh
cargo run -p home --bin astro-migrate-config -- --root /absolute/path/to/.astro --apply
```

支持两套已有布局中的实际配置输入：旧根目录 JSON 和按职责拆分后的 `models/providers.json`、`tools/enabled.json`、`skills/enabled.json`，以及 `agents/*/config.json` 和 Agent Skill 开关。

- 同类旧文件内容不同，或 TOML 已有段与 JSON 不同：停止，不猜测优先级。
- JSON/TOML 无法解析、发现已知明文凭证字段或源文件符号链接：停止。
- 应用时备份现有 TOML 和全部输入到 `backups/global-config-<uuid>/`，随后再次检查 JSON 源文件是否变化。
- 原 JSON 不删除；成功后以 `desktop.settings_version = 1` 标记完成，再运行是无操作。运行时不重新导入之后修改的旧 JSON。
- 恢复必须在停止应用的情况下使用备份，并与对应应用版本配套；不要在新旧版本之间同时运行两个配置写入者。

## 与目录布局迁移的集成

已与领域布局和基础参数的 YAML→TOML 改造整合：所有配置路径共用 `home::layout`，所有 TOML 写入共用 `config_file` 文件锁与原子替换。内存/权限等设置编辑只写回实际变化的键，保留未修改的 Desktop/MCP 内容、TOML 日期值及其注释。

运行时初始化不再生成 Provider、工具/Skill 开关或 Agent 默认设置的 JSON 副本。新安装直接使用 TOML；旧安装须先迁移目录，再迁移可编辑设置。两步都不会在启动时隐式执行，本次代码合并也没有对真实用户配置执行迁移。

## 验证范围

配置段独立更新、注释保留、并发写入、损坏配置保护、迁移预览/备份/冲突/幂等、新旧布局输入、JSON null 无损、工具失败关闭、项目 protected keys、Skill 开关、桌面 Provider、Cron/Workflow 同源读取均有自动化覆盖。测试使用临时目录与本地示例值，不执行真实用户配置迁移或 Provider 网络请求。

### 独立分支阶段验收记录（2026-09-09，合并前）

- Agent 核心单元测试：428 通过。
- home / agent-config / MCP / Skills：分别 60 / 23 / 69 / 82 通过；Skills 原有 1 项 ignored。
- 工具注册表：12 通过；桌面 Provider：28 通过；Cron：3 通过；Workflow 配置读取：1 通过。
- 共 706 项相关测试，不重复计算定向重跑。
- Agent、Server、Skills、Desktop 编译检查及迁移 CLI 编译检查通过。Desktop 存在未使用的 `ProviderConfig::new_disabled` 存量警告。
- home、Skills、agent-config、MCP、tools、Workflow、Agent 的严格定向 Clippy 通过。Server 严格检查仍有两处未改动 gRPC 代码的 `result_large_err` 存量告警；本次 Cron 调用的 needless-borrow 已修复。
- 格式与差异检查通过。未合并到共享工作区，未启动应用或运行真实用户配置迁移。

### 合并验收记录（2026-09-09）

- 已与领域布局改造及浏览器工具命名修复整合；配置路径走 `home::layout`，所有写入共用 `config_file`，不再维护第二套锁和原子写入实现。
- 新安装不生成工具/Skill 开关 JSON；配置迁移应用前检查目录迁移已完成。迁移后的配置可连续初始化和重启，不覆盖已有偏好或人格文件。
- 内存/权限写入按前后值的实际差异更新键，保留原生 TOML 日期、未修改字段和注释。相关测试检查了跨线程写入及同一段中的未修改日期字段。
- Agent 429、home 64、memory 单元测试 76、目录/配置集成测试 4、agent-config 23、MCP 69、Skills 82、Provider 28、工具注册表 12、目录迁移脚本 13 项通过，共 800 项；Skills 另有 1 项原有 ignored。
- Agent/Server/Desktop 与迁移 CLI 编译检查通过；home、memory、Skills、agent-config、MCP、tools、Workflow、Agent 的严格定向 Clippy 及格式/差异检查通过。
- 未执行真实用户配置迁移、未重启应用、未推送；本次只合并代码与文档。worktree 保留，清理需另行确认。
