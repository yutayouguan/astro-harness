# Permission System Improvements Plan

> **Status:** P0-P5 全部完成（2026-08-28）
> **需求分析**：`docs/01-需求分析阶段/06-权限系统改进需求分析.md`
> **详细设计**：`docs/04-详细设计阶段/06-安全与基础设施/10-权限系统改进详细设计.md`

**Goal:** 从 P0 到 P5 逐步提升 Astro 权限审批系统的用户体验、安全性和可扩展性。对标 Codex 的优势（会话缓存、上下文注入），同时发挥 Astro 的独特架构（HitlGate 并发模型、能力声明制、Interrupt 通用协议）。

**Architecture:** 改动集中在 `agent-core/src/control/`（新增 approval_cache、trust_model 模块）和 `agent-core/src/streaming/tools_exec.rs`（缓存查询插入点）。不改变 HitlGate/Interrupt 核心协议，仅在审批管线上叠加缓存层和上下文增强。

**Tech Stack:** Rust、Tokio、serde、YAML。

---

## P0: 会话级审批缓存 SessionApprovalCache

**目标：** 同一会话内，用户批准过的命令模式不再重复弹窗。

**影响范围：** 用户体验直接提升，改动面小。

### Task 1: 定义 ApprovalCache 数据结构

**Files:**
- Create: `crates/agent-core/src/control/approval_cache.rs`
- Modify: `crates/agent-core/src/control/mod.rs`

1. 定义 `ApprovalCacheKey`：按 `(tool_name, command_prefix)` 序列化。命令前缀取命令的前 2 个 token（如 `cargo test`），实现模式泛化。
2. 定义 `CachedApproval`：`{ decision: ApprovalAction, capabilities: Vec<PermissionCapability>, granted_at: Instant, grant_scope: GrantScope }`。
3. 定义 `SessionApprovalCache`：
   ```rust
   pub struct SessionApprovalCache {
       entries: Mutex<HashMap<ApprovalCacheKey, CachedApproval>>,
       session_id: String,
   }
   ```
4. 实现方法：`new(session_id)`、`lookup(key) -> Option<&CachedApproval>`、`insert(key, approval)`、`clear()`、`derive_child_view() -> ReadOnlyApprovalCache`（P3 使用）。
5. 在 `mod.rs` 中添加 `pub mod approval_cache;`。

### Task 2: 在审批管线中插入缓存查询

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`

1. 在 `approval_route()` 返回 `Manual` 或 `Smart` 之前，先查询 `SessionApprovalCache`。命中且 scope=Session 则直接返回 `Allowlist`（跳过审批）。
2. 在 `park_confirm()` 用户批准后，若用户选择"本次会话有效"（非 `allow_always`），将决定写入缓存，scope=`Session`。
3. 在 `park_confirm()` 的 `allow_always` 路径保持不变（写入持久白名单），同时也写入缓存（避免白名单生效前的窗口期重复弹窗）。

### Task 3: 缓存生命周期管理

**Files:**
- Modify: `crates/agent-core/src/control/hitl.rs`（HitlGate 清理时一并清缓存）
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`

1. `SessionApprovalCache` 跟随 HitlGate 生命周期。`HitlRegistry.cancel_and_remove_if` 清理 gate 时同步清理对应缓存。
2. 提供 `invalidate(key)` 方法——当命令执行失败（沙箱拒绝、进程异常退出）时主动驱逐缓存，防止坏命令被反复自动放行。
3. 缓存不持久化到磁盘，session 结束即清空。

### Task 4: 单元测试

**Files:**
- Modify: `crates/agent-core/src/control/approval_cache.rs`（内联 `#[cfg(test)]` 模块）

1. 测试 `lookup` 命中/未命中。
2. 测试命令前缀泛化：`cargo test -p agent` 和 `cargo test -p tools` 命中同一 key `cargo test`。
3. 测试 `invalidate` 驱逐后 `lookup` 返回 None。
4. 测试 `clear` 清空所有条目。
5. 运行 `cargo test -p agent approval_cache`。

### Task 5: 集成测试

**Files:**
- Create: `crates/agent-core/tests/approval_cache_integration_test.rs`

1. 模拟完整审批流：首次 `cargo test` → Manual → 用户批准(Session) → 缓存写入 → 二次 `cargo test` → 缓存命中跳过审批。
2. 模拟失败驱逐：缓存命中 → 执行失败 → 缓存驱逐 → 再次触发审批。
3. 运行 `cargo test -p agent approval_cache_integration`。

---

## P1: Smart Approval 注入对话上下文

**目标：** 让辅模型判断时看到当前对话语境，提升判断准确度。

**影响范围：** 改动集中在 smart_approval.rs，不影响审批管线主流程。

### Task 6: 定义 SmartApprovalContext

**Files:**
- Modify: `crates/agent-core/src/control/smart_approval.rs`

1. 新增结构体 `SmartApprovalContext`：
   ```rust
   pub struct SmartApprovalContext {
       pub recent_turns: Vec<TurnSummary>,          // 最近 3 轮摘要
       pub current_task_description: Option<String>, // 当前任务描述
       pub tool_call_chain: Vec<String>,             // 本轮已执行工具名列表
   }
   ```
2. `TurnSummary`：`{ role: Role, content_preview: String(最长 200 字符) }`。
3. 在 `build_prompt()` 中除 `PermissionRequest` JSON 外，附加 `SmartApprovalContext` 序列化后的上下文段。

### Task 7: 上下文采集与脱敏

**Files:**
- Modify: `crates/agent-core/src/control/smart_approval.rs`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`（传递上下文到 smart_approval 调用点）

1. 从 `TurnContext` 或 `StepContext` 采集最近消息。截断每条消息到 200 字符，总上下文不超过 1500 token。
2. 脱敏规则：去除 `content` 中匹配 `KEY|TOKEN|SECRET|PASSWORD|BEARER` 模式的值（替换为 `[REDACTED]`）。
3. 修改 `maybe_smart_downgrade_ask()` 签名，增加 `context: Option<SmartApprovalContext>` 参数。
4. 在 `tools_exec.rs` 中 Smart 路径的调用点构造并传入上下文。

### Task 8: 测试

**Files:**
- Modify: `crates/agent-core/src/control/smart_approval.rs`（内联测试）

1. 测试 `build_prompt` 输出包含上下文段。
2. 测试脱敏：输入含 `API_KEY=sk-xxx` 的消息，输出中替换为 `[REDACTED]`。
3. 测试截断：超长消息被截断到 200 字符。
4. 运行 `cargo test -p agent smart_approval`。

---

## P2: 统一审批粒度配置面

**目标：** 在 `config.toml` 提供统一的 `[approval]` section，简化用户配置体验。

**影响范围：** 配置加载层映射，不改变底层审批逻辑。

### Task 9: 定义统一配置结构

**Files:**
- Modify: `crates/agent-home/src/config.rs`（或 agent config 加载入口）

1. 在 `AgentConfig` 中新增 `ApprovalConfig` section：
   ```rust
   pub struct ApprovalConfig {
       pub terminal: Option<TerminalApprovalMode>,  // smart | manual | off
       pub mcp: Option<McpToolApprovalMode>,         // auto | prompt | writes | approve
       pub network: Option<NetworkApprovalMode>,     // allow | ask | deny
       pub file_write: Option<FileWriteApprovalMode>, // workspace | full | ask
   }
   ```
2. TOML 映射：
   ```toml
   [approval]
   terminal = "smart"
   mcp = "writes"
   network = "ask"
   file_write = "workspace"
   ```

### Task 10: 配置加载与合并

**Files:**
- Modify: `crates/agent-home/src/config.rs`
- Modify: 各域的审批策略读取点

1. `ApprovalConfig` 解析后，分发到各域已有的审批模式字段。未设置的字段使用各域默认值。
2. 优先级：`[approval]` section < 各域已有的独立配置字段（向后兼容）。即 `[approval].terminal = "off"` 可被 `[terminal].approval_mode = "manual"` 覆盖。
3. 启动日志中打印生效的审批配置摘要。

### Task 11: 测试

**Files:**
- 修改相关配置测试文件

1. 测试 `[approval]` section 解析与默认值。
2. 测试优先级覆盖：域级配置 > 统一配置。
3. 运行 `cargo test -p agent-home config`。

---

## P3: 子 Agent 审批缓存继承（依赖 P0）

**目标：** 子 agent 继承父级会话审批缓存的只读视图，减少 subagent 场景重复审批。

**影响范围：** agent-subagents + agent-core 交互。

### Task 12: ReadOnlyApprovalCache

**Files:**
- Modify: `crates/agent-core/src/control/approval_cache.rs`

1. 实现 `ReadOnlyApprovalCache`：持有父级 `Arc<SessionApprovalCache>` 的只读引用 + 子级自己的可写缓存。
2. `lookup` 先查子级缓存，未命中再查父级只读视图。
3. `insert` 只写入子级缓存，不影响父级。
4. 安全约束：子 agent 沙箱模式比父级窄时，父级 `FileWrite` 类批准不继承（通过 capability 交叉检查过滤）。

### Task 13: 子 Agent 创建时注入缓存

**Files:**
- Modify: `crates/agent-subagents/src/lifecycle.rs`（或 agent spawn 路径）
- Modify: `crates/agent-core/src/exec/subagents.rs`

1. `spawn_agent` 时，从父级 session 获取 `SessionApprovalCache`，调用 `derive_child_view()` 生成子级缓存。
2. 将子级缓存注入子 agent 的 `SessionServices` 或等效上下文。
3. 子 agent 结束时，其缓存自然丢弃（不回写父级）。

### Task 14: 测试

**Files:**
- Create: `crates/agent-subagents/tests/approval_inheritance_test.rs`

1. 测试子 agent 从父级缓存读取命中。
2. 测试子 agent 写入不影响父级。
3. 测试沙箱收窄时 capability 过滤。
4. 运行 `cargo test -p subagents approval_inheritance`。

---

## P4: 审批决策回溯与可解释性

**目标：** 在审计记录中保存 smart approval 的判断理由。

**影响范围：** 审计数据模型扩展，不影响运行时行为。

### Task 15: 扩展审计记录

**Files:**
- Modify: `crates/agent-memory/src/permission_audit.rs`
- Modify: `crates/agent-types/src/permissions.rs`（如需扩展 PermissionAuditEvent）

1. 在 `PermissionAuditEvent` 中新增 `reasoning: Option<String>` 字段（最长 500 字符）。
2. smart approval 路径中，将辅模型返回的原始文本（截断后）作为 reasoning 传入。
3. 用户手动审批路径中，reasoning 为 `Some("user_manual_approval")`。
4. 缓存命中路径中，reasoning 为 `Some("session_cache_hit: {original_key}")`。

### Task 16: 审计查询增强

**Files:**
- Modify: `crates/agent-memory/src/permission_audit.rs`

1. `list_recent_permission_audits` 返回结果中包含 reasoning 字段。
2. 新增 `list_permission_audits_by_tool(tool_name, limit)` 便捷方法，支持按工具名过滤审计记录。
3. 保持 JSONL 向后兼容：旧记录 `reasoning` 反序列化为 `None`。

### Task 17: 测试

**Files:**
- Modify: `crates/agent-memory/src/permission_audit.rs`（内联测试）

1. 测试 reasoning 字段写入/读取。
2. 测试旧格式 JSONL 向后兼容。
3. 运行 `cargo test -p memory permission_audit`。

---

## P5: 渐进式信任模型

**目标：** 同一 agent 连续被批准同类能力后，自动提升信任等级，减少审批频率。

**影响范围：** 较大，需要新增信任评估逻辑，但安全性要求最高。

### Task 18: 定义信任模型

**Files:**
- Create: `crates/agent-core/src/control/trust_model.rs`
- Modify: `crates/agent-core/src/control/mod.rs`

1. 定义 `TrustLevel` 枚举：`AlwaysAsk`（默认）→ `SmartReview` → `AutoApprove`。
2. 定义 `TrustScore`：
   ```rust
   pub struct TrustScore {
       pub capability: PermissionCapability,
       pub consecutive_approvals: u32,
       pub consecutive_denials: u32,
       pub level: TrustLevel,
   }
   ```
3. 升级阈值：连续 5 次批准 `AlwaysAsk → SmartReview`，连续 10 次批准 `SmartReview → AutoApprove`。
4. 降级规则：任何 1 次拒绝立即回退到 `AlwaysAsk`（fail-closed）。

### Task 19: 信任评估集成

**Files:**
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-core/src/control/approval_cache.rs`

1. 在 `approval_route()` 中，查询当前 agent 对应能力的 `TrustLevel`。
2. `TrustLevel::SmartReview` → 强制走 Smart 路径（即使 policy=Manual）。
3. `TrustLevel::AutoApprove` → 跳过审批（相当于 Allowlist）。
4. 每次审批决定后更新 `TrustScore`（批准 +1，拒绝归零+降级）。
5. 信任分数存储在 `SessionApprovalCache` 中，不跨会话。

### Task 20: 安全护栏

**Files:**
- Modify: `crates/agent-core/src/control/trust_model.rs`

1. `hardline_blocked` 命令永远不受信任模型影响（始终 Deny）。
2. `PermissionCapability::ExternalSideEffect` 不参与信任升级（始终至少 SmartReview）。
3. 提供 `reset_trust(capability)` 和 `reset_all_trust()` 方法。
4. 信任升级时在审计日志中记录 reasoning = `"trust_upgrade: {capability} {old_level} -> {new_level}"`。

### Task 21: 测试

**Files:**
- Modify: `crates/agent-core/src/control/trust_model.rs`（内联测试）

1. 测试升级阈值：5 次连续批准 → SmartReview。
2. 测试降级：1 次拒绝 → AlwaysAsk。
3. 测试 hardline 和 ExternalSideEffect 不受信任升级影响。
4. 测试 reset 方法。
5. 运行 `cargo test -p agent trust_model`。

---

## 全量验证

### Task 22: 全量编译与测试

1. `cargo check` 全量编译检查。
2. `cargo test` 全量测试。
3. `cargo clippy --all-targets` 静态分析。
4. `cargo fmt --all --check` 格式检查。

### Task 23: 文档更新

**Files:**
- Modify: `CLAUDE.md`（更新权限系统相关描述）

1. 在 Key Invariants 中添加审批缓存不变量。
2. 在 Core Architecture 中补充 SessionApprovalCache 说明。
3. 更新配置约定中的 `[approval]` section 文档。
