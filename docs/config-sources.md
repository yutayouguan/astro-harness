# 配置来源、缓存与 Hook 状态

`~/.astro/config.toml` 是唯一全局配置入口，不必把所有定义都放在同一个文件。
少量定义可以内联；数量增加时显式引用领域文件。未引用的文件不会自动加载。
项目规则仍在项目根 `AGENTS.md`，项目 `.astro/` 只在需要项目配置时创建。

## 全局入口示例

以下字段均由运行时支持；引用文件必须先存在。不需要拆分时省略 `config_sources`。

```toml
[config_sources]
mcp = ["mcp/servers.toml"]
hooks = ["hooks/rules.toml"]

[cache.models]
enabled = true
directory = "models/cache"
ttl_seconds = 600
max_size_mb = 256

[cache.mcp]
enabled = true
directory = "mcp/cache"
ttl_seconds = 600
max_size_mb = 256
```

| 内容 | 位置 / 约束 |
| --- | --- |
| MCP 定义与显式工具开关 | 入口的 `mcp_servers` 或显式引用的 TOML |
| Command/MCP Hook 定义 | 入口的 `hooks` 或显式引用的 TOML / JSON |
| 旧 Shell 观察总线命令 | 全局入口的 `shell_hooks`，与结构化 Hook 分开 |
| 模型目录、OpenRouter 元数据与定价 | `cache.models` 控制的可丢弃缓存 |
| MCP 发现元数据 | `cache.mcp` 控制的可丢弃缓存，不产生权限开关 |
| Hook 内容审批哈希 | 固定 `security/hooks/trust.json`，不是配置或缓存 |
| Hook 执行元数据 | 固定 `logs/hooks/runs-*.jsonl`，不保存命令、输入或输出正文 |

会话数据库、rollout、凭证、浏览器登录资料、审批记录不通过这些缓存字段重定向，
也不随缓存清理。Hook 日志当前没有自动保留期/轮转策略，不把它伪装成缓存 TTL。
全局 `SOUL.md`、`IDENTITY.md`、`USER.md`、`TOOLS.md` 等工作区文件不受项目配置覆盖。

## 来源与覆盖

- 引用路径相对于**声明它的配置文件**解析，不相对于进程工作目录；支持本地绝对路径，
  不支持 URL、环境变量或 `~` 展开。
- 全局引用不得逃出 Astro home；项目引用不得逃出项目根。软链接按真实路径检查。
  项目引用不得与已加载的全局配置来源重合。引用文件只允许本领域内容和同领域引用。
- 单文件最多 1 MiB、每领域最多 64 个来源、递归深度最多 8；缺失文件、循环、重复引用、
  同一配置层重复 ID 或跨领域内容均报错。不可信项目先被禁用，不解析其引用文件。
- 分层仍是系统 → 用户 → profile → 可信项目（根到当前目录）；引用继承声明层的优先级。
  MCP 按规范化 server ID **整体替换**，不把低层凭证/命令拼到高层 server 中。
- Hook 推荐显式 `id`，同层不能重复；高层同 ID 替换整条规则，其他规则继续叠加。
  没有 ID 的旧规则继续按位置标识；迁移不会擅自加 ID，以免改变已审批内容的哈希。
- 有 ID 时，规则数组重排不会改变 handler 标识；同一规则内部多个 handler 仍按索引区分，
  需要独立稳定身份时拆成各自有 ID 的规则。内容修改后已有审批哈希不匹配，该 handler 禁用。

`mcp/servers.toml` 示例：

```toml
[mcp_servers.docs]
command = "my-docs-mcp"
enabled = true
[mcp_servers.docs.tools]
write = false
```

`hooks/rules.toml` 示例：

```toml
[[hooks.PreToolUse]]
id = "terminal-check"
matcher = "terminal"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "./scripts/check-tool.sh"

[hooks.state.terminal-check]
enabled = true
```

只为引用文件路径改变解析基准；Hook 命令与 MCP 的工作目录仍遵循各自执行上下文。
`enabled` 是配置；`trusted_hash` 不再允许写入定义文件。全局定义与可信项目沿用既有信任机制，
拆分文件不会新增一次审批，也不会自行授予项目信任。

## 写回与缓存安全

MCP 设置返回每个 server 的 `sourcePath`，列表名称悬停显示来源。修改既有 server 写回原文件；
新增 server 写入所选 scope 的入口。后端重新解析来源，不接受客户端指定任意写入路径。
跨多个来源的同一次变更明确拒绝且不写任何文件，请按来源分次保存；未实现跨文件事务。
写回有跨进程锁，保留未改字段与注释，不把引用内容展开覆盖到入口。

缓存只在写入时创建目录；读取缺失缓存不会创建目录。TTL 为 1–2592000 秒，容量为
1–10240 MiB，默认 600 秒 / 256 MiB。`cache` 仅允许机器全局设置，项目层不能覆盖。
相对缓存路径限制在 Astro home 内；绝对路径可用于独立缓存盘，但不能指向用户/数据根或
受保护持久数据目录。容量清理只处理本领域、可验证封装的 `astro-cache-v1-*` 文件，
不递归清目录、不删除未知文件。

MCP 缓存身份包含定义、声明的凭证环境和项目根的摘要；磁盘只保存摘要和发现元数据。
所有 HTTP MCP 暂不持久缓存：自动 OAuth 尚缺稳定账号标识，继续实时发现。
模型缓存按 Provider 定义与当前凭证摘要校验；变更配置/凭证后旧结果不再匹配。
OpenRouter 磁盘缓存使用同一策略；请求失败时可能继续使用当前进程已加载的模型元数据，
这不是磁盘 TTL 的延长。缓存失败不应把成功的 MCP 实时发现判为连接失败。

## 显式离线迁移

先完成目录布局和全局可编辑设置迁移，再关闭所有使用同一数据根的 Astro 进程。
本工具不负责停止进程，也不在启动时自动运行。

```bash
# 默认只读预览
cargo run -p hooks --bin astro-migrate-extensions -- --root /absolute/path/to/astro-home
# 审核预览后才应用
cargo run -p hooks --bin astro-migrate-extensions -- --root /absolute/path/to/astro-home --apply
```

工具仅迁移选定全局根及其显式引用：为旧 `hooks.json` 增加引用、把字符串 Hook 移到
`shell_hooks`、把已有审批哈希原值移到安全状态文件、移除 MCP 定义中的 `discovered`。
显式工具开关、Hook enabled、规则 ID 和命令不变；不会自动把内联定义拆到新文件。
旧模型缓存不导入，运行时重建；旧缓存文件也不会在迁移中删除。

项目 `.astro/hooks.json` 需要项目所有者在 `.astro/config.toml` 显式添加
`[config_sources] hooks = ["hooks.json"]`。项目自带的 `trusted_hash` 不会由本工具导入
全局信任库；需人工核对来源与内容后处理，不能由仓库自行批准自己的代码。

应用前创建私有 `backups/extensions-*`，`manifest.json` 记录原路径及原始副本。
途中失败保留 `backups/extensions-in-progress.json`，运行时拒绝初始化/执行 Hook。
停机后按清单恢复每个原文件；`original: null` 表示原本不存在，应仅移除对应新文件。
核对恢复完成后才移除进行中标记。保留备份以便回滚，不直接重跑覆盖半迁移现场。

验证：`cargo test -p agent-config -p home -p mcp -p hooks -p usage --lib -- --test-threads=1`。
所有测试使用隔离临时目录，不对真实 `~/.astro` 执行迁移。

跨进程持久化验收：

```bash
cargo test -p server --test config_sources_restart_test -- --test-threads=1
```

父测试创建私有临时数据根，并启动独立测试进程完成写入和重新读取；验证原来源写回、
项目 server 整体覆盖、缓存删除后定义与审批状态保留、配置/凭证变更使缓存失效、Hook
内容变更不沿用旧审批，以及引用缺失时明确报错。每个子进程有超时和执行回执校验。
此测试不启动真实 App、不执行 Hook 命令、不连接 MCP，也不等同于原生界面启动验收。

## 原生 macOS 验收

沿用仓库现有 Tauri 构建流程，使用专用验收入口，不改变普通开发启动命令：

```bash
node tools/verify-config-native.mjs
# 恢复脚本上次输出的私有清单，不覆盖已有验收配置
node tools/verify-config-native.mjs --resume /absolute/path/to/astro-config-native-XXXXXX/manifest.json
```

脚本创建独立应用标识、回环前端端口和临时数据根，内置一个禁用的 MCP server 和一条禁用
Hook。专用 debug 二进制通过编译期 `ASTRO_NATIVE_ACCEPTANCE_ROOT` 绑定数据根，在日志与
凭证加载前校验目录和标记；缺失、错误或软链接标记都会拒绝启动，不回落真实数据根。
普通构建不设置该变量，release 构建不包含此分支。复制 QA 包后重新构建普通 dev 工件，
避免共享 `target/debug/astro-agent` 留下验收专用身份。

`ASTRO_ENV_HYDRATE=disabled` 显式跳过 Astro 的 `.env` 加载、登录 shell 密钥发现和自动
持久化，默认行为不变。此开关不清空进程原有环境变量，也不禁用其他代码的 Keychain 访问；
验收中不要添加真实账号或调用模型。必须使用自己的私有清单，恢复校验不是不可信二进制的
安全沙箱。脚本会在执行前核对二进制中包含预期数据根，防止误拷贝普通构建产物。

恢复入口校验应用标识、数据与日志路径、回环 URL、标记和二进制绑定；只跟踪精确 QA
可执行路径，不按 `astro-agent` 名称结束其他实例。退出 QA 或中断脚本会停止其私有前端，
保留验收文件。稍后重新打开 QA 包时仍绑定相同数据根，但必须恢复其私有前端；可以刷新
窗口重新加载页面。屏幕锁定时停止 GUI 操作，解锁后再继续，不以后台进程存活代替 UI 验收。

验证命令：

```bash
node --test tools/verify-config-native.test.mjs
cargo test -p astro-agent --bin astro-agent native_acceptance -- --test-threads=1
cargo test -p astro-agent --lib hydration_is_enabled -- --test-threads=1
```

2026-09-09 已观察到原生 MCP 页面显示独立 `mcp/servers.toml` 来源；将禁用 server 的默认
审批改为“每次询问”后，原来源保存 `default_tools_approval_mode = "prompt"`，入口 TOML
字节未变，server 仍禁用。没有执行 MCP/Hook 或模型调用；真实网络连接不在此验收结论内。

验收故障记录：最初仅通过 `open --env` 传入隔离变量，重新唤起时未继承这些变量，曾打开
真实数据根的日志和数据库。该实例已停止、失败 QA 包已禁用，未回滚或清理真实数据；当时
真实 `config.toml` 和 `.env` 校验值未变。此后改为 debug 二进制绑定数据根，并核对实际打开
的数据库路径与 `.env` 未生成；不能把早期失败尝试描述为“真实目录完全未被触及”。
