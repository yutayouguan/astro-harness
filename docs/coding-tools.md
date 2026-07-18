# Astro 编码工具链

面向「让 Agent 写代码」的一组内置系统工具：文件读写检索改（`file_ops`）、Shell 执行（`terminal`）、后台任务（`terminal_job`）、临时代码片段（`code_exec`）。本文档描述它们的能力边界、参数、上限与安全模型。

实现位于 `tools/src/builtins/system/`，参数 schema 由 `schemars` 自动生成后下发给模型。

---

## 沙箱与路径模型

所有文件类工具的**根目录**由 `ToolContext::project_or_workspace()` 决定：

- 有 `project_root`（委派 git worktree 或 `ASTRO_PROJECT_ROOT`）时以其为根；
- 否则回落到 Agent 记忆工作区 `workspace_dir`。

相对路径先经 `path_safe::resolve_safe` 校验：

- **始终拒绝** `..` 组件（含 `subdir/..`）、盘符、绝对根；
- 逐段跟随 symlink，目标必须仍在根内，越界报错；
- 写操作前 `reaffirm_within` 二次确认，防 TOCTOU。

> `terminal` / `code_exec` 的**默认工作目录**落在根内，但命令本身不是牢笼——`sh -c` 可访问整机路径。真正的隔离靠危险命令审批（见下）。

---

## `file_ops`

单工具多操作，参数结构见 `FileOpsArgs`。

### 操作一览

| operation | 作用 | 关键参数 |
|-----------|------|----------|
| `read` | 读取 UTF-8 文本 | `offset`/`limit`（字节）或 `start_line`/`end_line`（行） |
| `write` | 覆盖写入（自动建父目录） | `content` |
| `append` | 追加写入 | `content` |
| `patch` | 文本替换 | `old_string`、`new_string`、`replace_all` |
| `list` | 列目录 | `recursive`、`ext` |
| `search` | 搜文件名 / 内容 | `query`、`regex`、`ext`、`limit` |
| `move`/`rename` | 移动或重命名 | `dest` |
| `copy`/`cp` | 复制文件或目录树 | `dest` |
| `mkdir` | 递归建目录 | — |
| `delete` | 删除 | `recursive`（删目录树） |

### read：字节模式与行模式

- **字节模式**（默认）：从 `offset` 起最多返回 `limit.min(64KiB)` 字节。超上限时追加 `[truncated] ... offset=N` 提示，用返回的 `offset` 续读。末尾不完整多字节序列会被裁掉，下次补齐。
  - `offset` 落在多字节字符中间、或 `limit` 小到装不下单个字符时，报明确错误（**不会**误判为二进制）。
- **行模式**：传 `start_line`/`end_line`（1 起，含端点）。输出带 `[lines X..Y of N]` 头。
  - 整读文件，上限 **8 MiB**（更大请用字节模式）。
  - 单行超 64KiB 时返回该行的字符边界截断前缀并提示改用字节模式，**不静默丢弃**。
- 非 UTF-8（二进制）拒绝整段灌入上下文。

### patch：精确替换

- 默认要求 `old_string` 在文件中**唯一出现**：0 处或多处均报错（提示补充上下文或用 `replace_all`）。
- `replace_all: true` 替换**全部**匹配（至少 1 处），用于变量 / 字符串重命名。
- 仅支持 UTF-8 文本文件。

### search：文件名 + 内容

- `query` 必填（也可用 `content` 代替）。默认**大小写不敏感子串**；`regex: true` 按正则匹配。
- `ext` 按扩展名过滤，逗号分隔（如 `"rs,toml"`，不含点）。
- 每文件最多回 5 行命中，单行截断到 160 字符；总命中 ≤ `limit`（默认/上限 50）。
- 跳过 symlink 与噪音目录：`node_modules` `.git` `target` `.astro` `dist` `build` `.venv`。
- 单文件 >1 MiB 只匹配文件名；最多扫描 2000 个文件；输出软上限 64KiB。

### list / move / copy / delete

- `list`：默认列单层；`recursive: true` 递归成树（目录带尾 `/`），复用噪音目录忽略表；`ext` 过滤。上限 500 条 / 64KiB。
- `move` / `copy`：需 `dest`（走 `resolve_safe`）。目标**已存在则拒绝**。`move` 跨设备 rename 失败时回退「复制 + 删除」；`copy` 支持目录树递归。
- `delete`：拒绝删根目录；删目录树需 `recursive: true`；symlink 只删链接本身不跟随。

### HTML 预览联动

`write` / `append` / `patch` 命中 `.html`/`.htm` 时追加 media sidecar，前端活动卡渲染可预览的 HTML 卡片。

### 上限常量

| 常量 | 值 | 含义 |
|------|----|------|
| `MAX_READ_BYTES` | 64 KiB | 单次 read 上限 |
| `MAX_LIST_ENTRIES` / `MAX_LIST_BYTES` | 500 / 64 KiB | list 上限 |
| `MAX_SEARCH_HITS` / `MAX_SEARCH_BYTES` | 50 / 64 KiB | search 命中 / 字节上限 |
| `MAX_SEARCH_FILE_BYTES` | 1 MiB | 超此只匹配文件名 |
| `MAX_SEARCH_FILES_SCANNED` | 2000 | 最多扫描文件数 |
| 行模式整读上限 | 8 MiB | 更大改用字节模式 |

---

## `terminal`

通过 `sh -c` 执行命令，参数见 `TerminalArgs`。

| 参数 | 默认 | 说明 |
|------|------|------|
| `command` | — | 必填，Shell 命令字符串 |
| `cwd` | 根目录 | 相对根的工作子目录，经 `resolve_safe` 校验 |
| `timeout_secs` | 60 | 前台超时，钳制 `1..=900`；构建 / 测试 / 装依赖用 |
| `background` | false | 见下「后台任务」 |

- stdout/stderr 合并返回，超 `MAX_TOOL_RESULT_BYTES` 截断。
- **危险命令审批**：命令经 `classify_dangerous_command` 分类为 `Deny`/`Ask`/`Auto`，对 `background=true` 同样生效。详见下节。
- 支持 `TRANSFORM_TERMINAL_OUTPUT` 插件钩子在截断前改写输出（如脱敏）。

## 危险命令审批（smart / manual / off + hardline + 白名单）

配置在 `~/.astro/config.yaml` 的 `approvals:` 段（`memory::config::ApprovalsConfig`）：

```yaml
approvals:
  mode: smart              # smart(默认) | manual | off
  command_allowlist:       # 用户永久放行（精确或 glob，含 * ? [ ]）
    - "rm -rf /tmp/build"
    - "git push --force*"
```

判定优先级（`tools::resolve_command_action`）：

1. **hardline blocklist**（`Deny` 级：`mkfs`、fork 炸弹、`dd of=/dev/*`、`> /etc/`、停关键服务…）——**任何模式 / 白名单都不可越过**，直接拒。
2. **命中 `command_allowlist`** → 自动放行。
3. 其余按 `Ask` 级命令处理：
   - `off`：非 hardline 一律放行（等价 yolo，hardline 仍拦）。
   - `manual`：一律弹 HITL 卡人工确认（不走辅模型）。
   - `smart`（默认）：先用辅模型（`auxiliary.smart_approval`）评估，低危自动放行、拿不准才弹卡。
4. 安全命令（未分级）直接放行。

HITL 确认卡提供三个按钮：**Approve**（本次）/ **Approve & always allow**（本次 + 写入 `command_allowlist`，后续同命令自动放行）/ **Deny**。「always」经 `approve_always` 事件回传 `{approved:true, always:true}`，由 `memory::config::add_command_to_allowlist` 去重持久化。

> 与 Hermes 一致：这些是「防诚实但犯错的 agent」的护栏，不是对抗蓄意进程的沙箱；真隔离需容器/云后端。

---

## `terminal_job`（后台任务）

`terminal(background=true)` 会立即返回一个 `job id`、命令脱离 60s 超时在后台跑；再用 `terminal_job` 轮询 / 等待 / 终止。

### 为什么需要进程级注册表

工具在**每次调用时都跑在临时 tokio 运行时**上（`agent` 层从 `AgentLoop` 快照重建 `ToolContext`，调用结束即销毁）。后台进程若挂在 `ToolContext` 上会被一起回收，因此用**进程级全局注册表**（`OnceLock<Mutex<..>>`）持有 OS 子进程，stdout/stderr 由独立 OS 线程抽取写入共享缓冲，生命周期与临时运行时解耦。

### 动作

| action | 作用 | 参数 |
|--------|------|------|
| `list` | 列当前会话的后台任务 | — |
| `status` / `poll` | 从 `offset` 增量拉取新输出 | `id`、`offset` |
| `wait` | 阻塞至任务结束或超时 | `id`、`timeout_secs`（默认 30，上限 600） |
| `kill` | 终止任务 | `id` |

### 语义与上限

- 输出合并 stdout/stderr，单任务缓冲上限 **1 MiB**，单次 `status`/`wait` 返回上限 **60 KiB**（按 UTF-8 字符边界对齐，接缝无替换字符）。
- 注册表最多 **50** 个任务，超出优先淘汰已结束的。
- **会话隔离**：`list` 只显示本会话任务；`status`/`wait`/`kill` 按 id 操作但校验会话归属，跨会话视为「未找到」。
- **进程组 kill**（Unix）：spawn 时 `process_group(0)` 让子进程自成进程组，`kill` 先 `SIGTERM` 后 `SIGKILL` 整组带走，避免 `npm run dev` 等 fork 出的孙进程变孤儿。
- **退出清理**：应用退出（Tauri `RunEvent::Exit`）调用 `shutdown_all_jobs()` 终止全部在运行任务——后台任务处于独立进程组，本进程退出不会自动带走它们。

---

## `code_exec`

在工作区临时目录（`.code_exec/`）运行短代码片段。

| 参数 | 默认 | 说明 |
|------|------|------|
| `code` | — | 必填，源代码 |
| `language` | `python` | `python` \| `javascript`\|`js` \| `shell`\|`bash` |

- 每次调用写入**唯一临时文件**（`snippet_{uuid8}.{ext}`），并发同语言调用互不覆盖。
- 超时 30s（超时也会删临时文件并 `kill_on_drop` 子进程），输出超 `MAX_TOOL_RESULT_BYTES` 截断。
- **资源护栏，不是硬沙箱**（仍跑在宿主、cwd 在工作区根内）：
  - **环境剥离**：子进程 `env_clear` 后仅注入 `PATH`/`HOME`/`LANG`/`TMPDIR` 等白名单变量；名称含 `KEY`/`TOKEN`/`SECRET`/`PASSWORD`/`CREDENTIAL`/`AUTH` 等一律不传。
  - **Unix rlimit**（`pre_exec` + `setrlimit`）：CPU 30s、地址空间 512MiB、单文件 32MiB、打开 fd 64（不设 `NPROC`：该限制按用户计数，桌面环境易误杀）。
  - 更深隔离（Landlock / `sandbox-exec` / `bwrap` / 容器）仍属后续优化。

---

## 工具集开关

`file_ops`、`terminal`（含 `terminal_job`）、`code_exec` 各为独立 toolset，受 `tools-enabled.json` 控制；`terminal_job` 归入 `terminal` toolset，与终端开关一起启停。映射见 `home::tool_name_to_toolset`。

---

## 已知限制与后续优化

按性价比排序（截至本文撰写未实现）：

1. **`code_exec` 更深隔离**：已具备 env 剥离 + Unix rlimits；下一层可为 Landlock/`sandbox-exec`/`bwrap` → 容器/WASM。~~并发临时文件名争用~~ / ~~敏感环境泄露~~ / ~~基础 rlimit~~ 已落地。
2. **后台任务输出「丢新留旧」**：缓冲满 1 MiB 后丢弃后续输出、保留最早 1 MiB，与「盯 dev server 最新日志」诉求相反。可考虑有界尾部窗口（需重设计 offset 分页语义）。
3. **`write` / `patch` 非原子写**：`std::fs::write` 直接截断重写，中途崩溃留半截文件。宜临时文件 + rename 原子落盘。
4. **`search` / `list` 不读 `.gitignore`**：仅硬编码噪音目录表，真实仓库会污染结果 / 拖慢扫描。可引入 `ignore` crate。
5. **`patch` 不支持一次多处编辑**：多点编辑需多次调用、逐次重读重写；可加 `patches: [{old,new}]` 批量原子应用。
6. **无 `stat` 操作**：读大文件前探大小 / 判断存在只能靠 `list` 或试错。
7. 锦上添花：`search` 上下文行、`read` 行号前缀（opt-in）、`move`/`copy` 的 `overwrite`、`terminal_job` 的 `remove`/`clear`。
