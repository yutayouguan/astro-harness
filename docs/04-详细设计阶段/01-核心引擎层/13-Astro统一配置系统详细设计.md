# Astro 统一配置系统详细设计

> **Harness 定位（2026-09-04）**：配置是 Harness 的输入，不是可在 step 中任意变化的全局变量。Thread 设置、model context、interaction mode、tool gates、MCP、Extension 和 project trust 需在 turn/step 边界捕获快照。Astro 配置仅从 `~/.astro` 和可信项目 `.astro` 加载，不读取 `.codex` 作为运行配置。

> 状态：已实现。Astro 对齐 Codex 的分层、项目发现、信任门控、来源追踪和请求快照语义，
> 但使用 Astro 自有路径 `.astro/config.toml`，不会读取 `.codex`。

## 1. 配置入口

```text
/etc/astro/config.toml                 # 系统层（Unix）
~/.astro/config.toml                   # 用户层
~/.astro/<profile>.config.toml         # 显式选择的 profile 层
<project>/.astro/config.toml           # 可信项目层
<project>/<nested>/.astro/config.toml  # 可信项目内更近目录的覆盖层
~/.astro/agents/*.toml                 # 用户自定义角色
<project>/.astro/agents/*.toml         # 可信项目自定义角色
```

旧入口 `mcp.json`、`~/.astro/agents/<agent_id>/config.toml`、
`~/.codex/config.toml` 和 `.codex/agents` 均被忽略，也不会自动迁移。
`agents/<agent_id>/config.json` 只保存 persona 的运行时模型/工具状态，不承载 MCP。

## 2. 层级与覆盖

通用层级从低到高为：

```text
packaged defaults
  → managed preferences
  → system
  → enterprise managed
  → user
  → selected profile
  → trusted project（仓库根到 cwd，越近越高）
  → declared agent role
  → session overrides
  → request overrides
```

`agent-config` 保存每层原始 TOML、来源和稳定指纹。解析完成后生成不可变
`EffectiveConfig`：包含合并值、逐 key 来源、有效层列表及整体版本。一次请求或 turn
持有同一快照，禁止中途重读配置导致行为漂移。

普通表递归合并；具体业务域可以定义更严格的覆盖单位。MCP 以 server 为单位整体覆盖，
不会把不同安全来源的 command、URL、header 或环境变量字段拼成一个 server。

## 3. 项目发现与信任

项目根由用户/系统层的 `project_root_markers` 决定。项目 TOML 只有在用户层显式声明
可信后才会读取：

```toml
[projects."/absolute/path/to/project"]
trust_level = "trusted"
```

未知或不可信项目的 `.astro/config.toml` 不解析、不执行，只产生 disabled layer 与诊断。
即使项目已可信，provider 路由、base URL、profile、通知、遥测、realtime endpoint 和
Responses metadata 等机器级 key 仍不能由项目覆盖；被拦截的 key保留诊断及来源。

## 4. Agent 设置与角色

```toml
[agents]
enabled = true
max_concurrent_threads_per_session = 4
default_subagent_model = "openai:gpt-5.6"
default_subagent_reasoning_effort = "high"
interrupt_message = true

[agents.reviewer]
description = "Review changes"
config_file = "agents/reviewer.toml"
nickname_candidates = ["reviewer", "auditor"]
```

角色文件可声明 `model`、`model_reasoning_effort`、`sandbox_mode`、`mcp_servers`、
`skills.config` 和 developer instructions。相对 `config_file` 按声明它的配置文件目录解析；
声明式角色在同层覆盖独立角色文件，可信项目层覆盖用户层。角色权限只能收窄父任务。

## 5. MCP

```toml
[mcp_servers.docs]
url = "https://developers.openai.com/mcp"
enabled = true
required = false
startup_timeout_sec = 10
tool_timeout_sec = 60

[mcp_servers.local]
command = "npx"
args = ["-y", "@example/mcp-server"]
env_vars = ["EXAMPLE_TOKEN"]
```

Desktop 配置编辑器只读写 `~/.astro/config.toml`。运行时再叠加可信项目层；Agent id
只标识连接 Hub 和运行状态，不选择另一份配置文件。自定义角色的 MCP 覆盖来自角色 TOML。
发现缓存只回写全局层已显式定义的 server，不会把系统或项目 server 摊平到用户层。

## 6. Runtime 权限与受管网络

Agent runtime 的权限选择保存在对应 Agent 目录的 `config.yaml`，使用三个正交维度：

```yaml
approval_policy: on-request       # on-request | never
approvals_reviewer: auto_review   # user | auto_review
permissions:
  default_profile: project-net
  profiles:
    project-net:
      extends: ":workspace"
      network:
        enabled: true
        domains:
          api.example.com: allow
        header_injections:
          - host: api.example.com
            methods: [POST]
            path_prefixes: [/console/v1]
            headers:
              x-managed-source: managed-value
network_proxy:
  enabled: true
```

- `approvals_reviewer = user` 不调用 SmartApproval；`auto_review` 只允许明确
  `approve_once` 自动放行。
- Full Access 跳过普通审查，但不能绕过 hardline deny、结构化用户输入或取消状态。
- profile/reviewer 在 active turn 切换后，后续 attempt 立即读取新值。
- `header_injections` 是 managed requirement；Debug 只显示 header 名，不显示值。当前
  HTTP CONNECT 代理看不到 TLS method/path，因此只携带规则，不执行 HTTPS 注入。

## 7. Profile 与临时覆盖

profile 必须由调用方显式选择，只允许字母、数字、`-`、`_`，对应
`~/.astro/<profile>.config.toml`。缺失 profile 直接失败。旧 `[profiles.*]` 与新独立
profile 文件同名时直接报冲突，不进行双轨合并。

session/request overrides 是内存层，不落盘；request 高于 session。它们同样进入来源追踪
和有效版本计算。

## 8. 与 Codex 的关系

Codex 官方路径是 `~/.codex/config.toml` 与可信项目 `.codex/config.toml`。Astro 采用相同
的关键语义，但选择 `.astro` 命名空间，避免两个产品互相执行对方的脚本、MCP 或项目配置。
因此这是“语义对标、路径隔离”，不是路径兼容层。
