# agent-config

通用分层配置原语：按优先级合并多层 TOML 配置，提供精确到键的来源追溯，不含产品特定字段，不执行文件系统发现。

## 核心职责

- **分层合并** -- 将来自打包默认值、系统、用户、Profile、项目、Agent、Session、Request 等多层 TOML 配置，按优先级从低到高递归合并。Table 递归合并，标量和数组整体替换
- **键级来源追溯** -- 每个生效键可精确追溯到来自哪个配置层（文件路径 + SHA-256 内容指纹），支持调试和 UI 展示
- **信任与安全** -- 项目层配置在未获信任前不解析不应用；已信任项目的受保护键（凭证、Provider、遥测等）被自动剥离并产生诊断
- **文件系统发现** -- `loader` 模块实现 Codex 兼容的本地配置发现：从 cwd 到项目根逐层扫描 `.astro/config.toml`，支持 Profile、系统配置、项目根标记自定义
- **指纹稳定性** -- `parse()` 使用原始文本 SHA-256；`from_value()` 使用规范化二进制（键排序）；Table 键顺序不影响指纹

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | 核心原语：`ConfigLayerSource`（10 种来源 + 优先级）、`ConfigKeyPath`（无损键路径）、`ConfigLayerEntry`（单层解析 + 指纹）、`ConfigLayerStack`（有序层栈 + 递归合并 + 来源追溯）、`merge_toml_values()` |
| `loader.rs` | 文件系统发现：`load_local_config()` 加载全部本地配置层、项目根发现（`project_root_markers`）、信任评估（`ProjectTrust`）、受保护键剥离、Profile 校验与冲突检测、诊断收集 |

## 核心类型与 API

- `ConfigLayerSource` -- 10 种配置来源枚举，`precedence()` 返回 i16 优先级（-10 到 35）
- `ConfigKeyPath` -- 无损键路径（`Vec<String>` 段），含 `.` 的字面量键不与嵌套路径碰撞
- `ConfigLayerEntry` -- 单层配置：source + TOML Value + SHA-256 版本指纹 + 可选 disabled_reason
- `ConfigLayerStack` -- 有序层栈：`new()` 按优先级稳定排序、`effective_config()` 合并、`origins()` 来源追溯、`origin_at(segments)` 单键查询
- `ConfigOrigin` -- 单键来源：source + version 指纹
- `merge_toml_values(base, overlay)` -- 递归合并两个 TOML Value
- `load_local_config(options)` -- 加载全部本地配置层，返回 `LocalConfigLoad`
- `LocalConfigOptions` -- 加载选项：codex_home / cwd / system_config / profile / session_overrides / request_overrides
- `LocalConfigLoad` -- 加载结果：layers + diagnostics + project_root + project_trust
- `ProjectTrust` -- 信任枚举：Trusted / Untrusted / Unknown
- `ConfigDiagnostic` -- 诊断消息：code（ProjectLayerDisabled / ProjectKeyIgnored）+ source + key + message
- `PROJECT_PROTECTED_KEYS` -- 项目层不可覆盖的受保护键列表（openai_base_url / model_provider / notify 等）

## 设计要点

- **禁用层可见** -- 被禁用的层仍出现在层栈中（可供 UI 展示），但不参与合并计算
- **等优先级稳定** -- 同优先级层保持插入顺序（项目层从根到 cwd，最近目录优先）
- **表覆盖清理** -- 高优先级层用标量替换低优先级 Table 时，子键来源记录自动清除
- **Profile 隔离** -- Profile 不能授予项目信任或改变项目根发现逻辑

## Crate 关系

| 方向 | crate |
|------|-------|
| 无内部依赖 | 仅使用 serde / toml / sha2 / thiserror，不依赖 workspace 内其他 crate |
| 被依赖 | `agent`（运行时加载生效配置）、`home`（配置路径解析）、`apps/desktop`（Tauri 配置 UI） |

## 测试

```bash
# 全部测试
cargo test -p agent-config

# 单个测试
cargo test -p agent-config precedence_and_project_proximity_are_deterministic
cargo test -p agent-config trusted_project_cannot_override_machine_local_keys -- --nocapture
```
