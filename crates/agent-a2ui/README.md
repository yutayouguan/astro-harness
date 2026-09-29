# a2ui

AG-UI 声明式生成式 UI 表面 -- 定义 Agent 可向前端推送的结构化 UI 组件目录、模板构建函数与校验逻辑。

属于 [Astro Harness](../../README.md) workspace，详见根目录 `CLAUDE.md` 的 Crate Map。

## 核心职责

1. **组件目录** -- 维护 22 种允许的 AG-UI 组件白名单（`ALLOWED_COMPONENTS`），Catalog ID 为 `astro://a2ui/catalog/v2`，保证 Agent 生成的 UI 操作不会包含未注册的组件类型。
2. **模板构建** -- 提供 9 种预置 HITL / 信息展示 Surface 模板函数，生成符合 A2UI v0.9 协议的 `createSurface` + `updateComponents` JSON 操作序列，供 Agent 工具直接使用。
3. **操作校验** -- `validate_operations` 对 Agent 生成的 A2UI 操作序列做结构性校验：版本字段、操作类型唯一性、catalogId 匹配、组件白名单检查。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | Crate 入口，re-export `ASTRO_CATALOG_ID`、`validate_operations`、`Error` |
| `catalog.rs` | 组件目录常量：`ASTRO_CATALOG_ID`（catalog 标识符）和 `ALLOWED_COMPONENTS`（22 种组件白名单） |
| `templates.rs` | 9 种 Surface 模板构建函数，生成 A2UI JSON 操作序列 |
| `validate.rs` | `validate_operations` 校验函数与 `Error` 错误枚举 |

## 核心类型与 API

### 常量

- **`ASTRO_CATALOG_ID`** -- `"astro://a2ui/catalog/v2"`，所有 `createSurface` 操作必须引用此 ID
- **`ALLOWED_COMPONENTS`** -- 22 种组件名称白名单：
  - 基础组件：`Text`、`Icon`、`Divider`、`Card`、`Column`、`Row`、`Button`、`TextField`、`ChoicePicker`、`CheckBox`、`Image`、`Audio`、`Video`、`List`
  - v2 扩展组件：`Badge`、`Chip`、`Metric`、`Avatar`、`Callout`、`Spacer`
  - 多步向导：`ClarifyWizard`

### 模板函数（均返回 `Vec<serde_json::Value>`）

| 函数 | 用途 |
|---|---|
| `build_confirm_surface` | 确认审批卡片（Approve / Deny，可选 Always allow） |
| `build_confirm_surface_ex` | 带 `allow_always` 参数的扩展确认卡片 |
| `build_clarify_surface` | 多步澄清向导（ClarifyWizard），支持多 tab 步骤 |
| `build_location_request_surface` | 位置授权请求卡片（共享 GPS / 手动填写城市） |
| `build_info_surface` | 只读信息卡片（标题 + 正文 + 可选图片） |
| `build_metrics_surface` | 指标列表卡片（标题 + Metric 行，支持 hint） |
| `build_callout_surface` | 提示/警告 Callout 卡片（info/warn 变体） |
| `build_result_surface` | 结果状态卡片（success/warn/danger/info 四状态） |
| `build_delete_surface` | 删除 Surface 操作 |
| `build_form_surface` | 表单卡片（TextField/CheckBox 字段 + Submit 按钮） |
| `build_network_approval_surface` | 网络访问审批卡片（allow-once / allow-session / always / deny） |
| `build_chip_list_surface` | 标签/Chip 列表卡片 |

### 辅助结构体

- **`ClarifyStep`** -- 澄清向导的单个步骤：`id`（答案键）、`question`、`options`
- **`FormField`** -- 表单字段描述：`id`、`kind`（`"text"` / `"checkbox"`）、`label`、`required`

### 校验

- **`validate_operations(ops: &[Value]) -> Result<(), Error>`** -- 校验操作序列
- **`Error`** 枚举：`MissingVersion` / `InvalidOperationKind` / `InvalidCatalogId` / `UnknownComponent` / `InvalidStructure`

## 与其他 crate 的关系

- **`agent-tools`** -- HITL 审批工具（dangerous command、switch_mode 等）调用模板函数生成 A2UI 操作推送给前端
- **`agent-core`** -- Agent 运行时的 HITL bridge 使用 confirm/clarify surface 与用户交互
- **`agent-network-proxy`** -- 网络拦截后通过 `build_network_approval_surface` 生成审批卡片
- **`agent-server`** -- gRPC 服务端透传 A2UI 操作到前端
- 前端 React 应用（`apps/desktop/`）负责渲染这些 A2UI 组件

## 测试运行命令

```bash
# 全部测试（单元 + 集成）
cargo test -p a2ui

# 集成测试（tests/ 目录）
cargo test -p a2ui --test recipes_test
cargo test -p a2ui --test templates_test
cargo test -p a2ui --test validate_test
```
