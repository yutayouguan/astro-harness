# Plugin SDK 开发者文档

> **Harness 边界（2026-08-29）**：Plugin 通过注册工具、Skill 或 Hook 扩展 Harness，但不拥有 Session/turn loop、不能伪造 `StepContext` 可见性，也不能绕过 approval/sandbox/rollout。未有当前 Host API 实现的 WASM/市场示例应视为目标设计。

> 版本：v1.0 | 日期：2026-08-12 | 状态：草稿
> 面向对象：第三方插件开发者
> 前置文档：[04-WASM插件沙箱API设计.md](../_v0.3规划/04-WASM插件沙箱API设计.md)、[06-Agent市场详细设计.md](../_v0.3规划/06-Agent市场详细设计.md)、[08-Hooks系统详细设计.md](../01-核心引擎层/08-Hooks系统详细设计.md)、[01-Skills系统详细设计.md](01-Skills系统详细设计.md)

本文档面向希望为 Astro Agent 平台构建插件的第三方开发者。你将学到如何从零开始创建插件、开发自定义工具和 Hook、打包 Skill、测试调试，以及最终发布到市场。

---

## 1. 插件开发快速入门

### 1.1 什么是插件

Astro Agent 插件是一个运行在 WASM 沙箱中的扩展包。通过插件，你可以为 Agent 添加以下能力：

- **自定义工具（Tool）**：让 Agent 能调用你编写的函数，例如翻译、代码分析、数据查询等
- **Hook（钩子）**：在 Agent 执行管线的关键节点（如工具调用前、LLM 调用前）介入处理
- **Skill（技能）**：打包结构化提示词模块，教会 Agent 在特定场景下如何行动
- **MCP Server 集成**：声明插件依赖的 MCP Server，由宿主统一管理

插件以 `.agent` 包格式分发，可通过 Astro Agent 市场安装，也可本地加载。

### 1.2 插件运行原理

插件代码编译为 WebAssembly（WASM），在 wasmtime 运行时的沙箱中执行。插件不能直接访问宿主文件系统、网络或数据库，只能通过宿主提供的一组 `astro_*` 函数（Host Functions）与外界交互。这一设计保证了安全性：即使插件代码存在 bug 或恶意行为，也无法突破沙箱边界。

```text
┌─────────────────────────────────────────────┐
│                 Astro Agent 宿主              │
│                                               │
│  ┌─────────────┐     ┌──────────────────┐    │
│  │ ToolRegistry │     │   HookRegistry   │    │
│  └──────┬──────┘     └────────┬─────────┘    │
│         │                     │              │
│    astro_* Host Functions（日志/KV/网络/工具） │
│         │                     │              │
│  ┌──────┴─────────────────────┴──────────┐   │
│  │          WASM 沙箱 (wasmtime)          │   │
│  │  ┌────────────────────────────────┐   │   │
│  │  │        你的插件代码 (.wasm)       │   │   │
│  │  └────────────────────────────────┘   │   │
│  └───────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

### 1.3 开发环境准备

**Rust 开发者（推荐）**：

```bash
# 安装 Rust（如尚未安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 添加 WASM 编译目标
rustup target add wasm32-wasip1

# 安装 Astro Agent CLI（包含插件开发工具链）
cargo install astro-agent-cli

# 验证安装
astro --version
astro plugin --help
```

**其他语言**：

任何能编译到 WASM 的语言都可以用来开发插件（如 C/C++、Go、AssemblyScript）。但 Rust SDK 提供最完善的类型安全封装，推荐优先使用。本文档以 Rust 为主要示例语言。

### 1.4 创建第一个插件

```bash
# 用脚手架创建插件项目
astro plugin init my-first-plugin
cd my-first-plugin

# 查看生成的目录结构
tree .
```

脚手架会生成一个完整的插件项目骨架，包含 `plugin.toml`、Rust 源码和构建配置。接下来我们详细了解每个部分。

---

## 2. 插件目录结构

一个完整的插件项目目录如下：

```text
my-plugin/
├── plugin.toml                 # 插件清单文件（必须）
├── Cargo.toml                  # Rust 项目配置
├── src/
│   └── lib.rs                  # WASM 插件主代码（必须）
├── skills/                     # 内嵌 Skill 文件（可选）
│   ├── translate/
│   │   └── SKILL.md
│   └── summarize/
│       └── SKILL.md
├── assets/                     # 静态资源（可选）
│   ├── icon.png                # 市场展示图标（256x256 PNG）
│   └── screenshots/
│       └── demo.png
├── tests/                      # 测试代码（可选）
│   └── integration_test.rs
├── scripts/                    # 辅助脚本（可选）
│   └── build.sh
└── README.md                   # 使用说明（可选但推荐）
```

各部分说明：

| 文件/目录 | 必须 | 说明 |
|-----------|------|------|
| `plugin.toml` | 是 | 插件元数据、权限声明、工具/Hook 注册 |
| `Cargo.toml` | 是 | Rust 依赖和编译配置 |
| `src/lib.rs` | 是 | 插件主逻辑，编译为 `plugin.wasm` |
| `skills/` | 否 | 随插件分发的 SKILL.md 文件 |
| `assets/` | 否 | 图标、截图等静态资源 |
| `tests/` | 否 | 插件测试代码 |
| `README.md` | 否 | 使用文档（发布到市场时强烈推荐） |

---

## 3. plugin.toml 清单规范

`plugin.toml` 是插件的核心配置文件，声明插件的身份、能力和资源限制。

### 3.1 完整字段参考

```toml
[plugin]
# === 基础信息 ===
id = "my-translator"                    # 必须。全局唯一标识，kebab-case
name = "智能翻译插件"                     # 必须。人类可读名称
version = "1.0.0"                       # 必须。semver 版本号
description = "支持 50+ 语言的智能翻译"    # 必须。简短描述（市场展示用）
author = "your-name"                    # 必须。作者名或组织名
license = "MIT"                         # 推荐。开源许可证
homepage = "https://github.com/you/my-translator"  # 可选。项目主页
entry = "plugin.wasm"                   # 必须。WASM 主文件路径

# === 兼容性 ===
min_host_version = "0.5.0"             # 必须。最低兼容的 Astro Agent 版本

[capabilities]
# === 权限声明 ===
# 安装时会向用户展示权限列表，用户确认后才授予。
# 仅声明你真正需要的权限，过多权限会降低用户信任度。

# 网络访问（限白名单域名）
network = { allow_domains = ["api.example.com", "translate.googleapis.com"] }

# 插件私有 KV 存储
storage = true

# 允许调用的内置工具列表
tools = ["file_read"]

# 允许订阅的事件列表
events = ["tool_called"]

[limits]
# === 资源限制 ===
memory_mb = 32                          # WASM 线性内存上限（默认 32 MiB，最大 128 MiB）
fuel = 10_000_000                       # 每次调用的 CPU 燃料上限
timeout_secs = 5                        # 单次调用超时（默认 5 秒，最大 30 秒）

# === 工具注册 ===
[[tools]]
name = "translate"                      # 工具名称（Agent 调用时使用）
handler = "tool_translate"              # WASM 导出函数名
description = "将文本翻译为指定语言"
risk_level = "L1"                       # 风险等级：L0（无风险）L1（低）L2（中）L3（高）

[tools.schema]                          # JSON Schema 格式的输入参数定义
type = "object"
required = ["text", "target_lang"]

[tools.schema.properties.text]
type = "string"
description = "要翻译的文本"

[tools.schema.properties.target_lang]
type = "string"
description = "目标语言代码，如 en, zh, ja"

[tools.schema.properties.source_lang]
type = "string"
description = "源语言代码（可选，自动检测）"

# === Hook 注册 ===
[[hooks]]
event = "before_tool_execute"           # 监听的事件
handler = "on_before_tool"              # WASM 导出函数名
priority = 550                          # 优先级（插件 Hook 范围：500-999）
tool = "file_write"                     # 可选，仅在特定工具调用时触发

# === Skill 声明 ===
[[skills]]
file = "skills/translate/SKILL.md"      # Skill 文件路径（相对于插件根目录）

[[skills]]
file = "skills/summarize/SKILL.md"
```

### 3.2 字段说明

**`[plugin]` 部分**：

| 字段 | 类型 | 必须 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 全局唯一标识，仅允许小写字母、数字和连字符 |
| `name` | string | 是 | 显示名称 |
| `version` | string | 是 | 语义化版本号（如 `1.2.3`） |
| `description` | string | 是 | 一句话描述，至少 10 个字符 |
| `author` | string | 是 | 作者名 |
| `license` | string | 否 | 许可证标识（如 `MIT`、`Apache-2.0`） |
| `homepage` | string | 否 | 项目主页 URL |
| `entry` | string | 是 | 编译后的 WASM 文件名 |
| `min_host_version` | string | 是 | 最低兼容的 Astro Agent 版本 |

**`[capabilities]` 部分**：

| 字段 | 类型 | 说明 |
|------|------|------|
| `network` | object | 网络访问权限，`allow_domains` 指定允许访问的域名列表 |
| `storage` | bool | 是否需要插件私有 KV 存储 |
| `tools` | array | 允许调用的宿主内置工具列表 |
| `events` | array | 允许订阅的事件列表 |

**`[limits]` 部分**：

| 字段 | 默认值 | 最大值 | 说明 |
|------|--------|--------|------|
| `memory_mb` | 32 | 128 | WASM 线性内存上限（MiB） |
| `fuel` | 10,000,000 | 50,000,000 | CPU 燃料上限（约等于 WASM 指令数） |
| `timeout_secs` | 5 | 30 | 单次函数调用超时（秒） |

---

## 4. Host API 参考

宿主函数是插件与外界交互的唯一通道。所有宿主函数以 `astro_` 为前缀，通过 WASM 导入函数机制提供。

使用 Rust SDK 时，你无需直接操作裸内存指针，SDK 将宿主函数封装为安全的 Rust API。

### 4.1 astro_log -- 写入日志

将日志消息输出到宿主的日志系统。调试时必备，无需任何权限声明。

**SDK 接口**：

```rust
// 无需 Capabilities 声明
ctx.log_debug("调试信息");
ctx.log_info("一般信息");
ctx.log_warn("警告信息");
ctx.log_error("错误信息");
```

**底层签名**（了解即可，SDK 已封装）：

```text
astro_log(level: i32, msg_ptr: i32, msg_len: i32) → void
  level: 0=debug, 1=info, 2=warn, 3=error
```

**示例**：

```rust
#[astro_plugin_sdk::export]
fn my_tool(ctx: &mut Context, input: String) -> Result<String> {
    ctx.log_info(&format!("收到输入：{}", input));
    // ... 处理逻辑 ...
    ctx.log_debug("处理完成");
    Ok(result)
}
```

### 4.2 astro_kv_get / astro_kv_set / astro_kv_delete -- KV 存储

插件私有的键值存储，数据在插件实例之间持久化。每个插件的 KV 空间完全隔离，不同插件之间无法互相访问。

**需要权限**：`storage = true`

**SDK 接口**：

```rust
// 写入
ctx.kv_set("user_prefs", b"{'lang': 'zh'}")?;

// 读取
let value: Option<Vec<u8>> = ctx.kv_get("user_prefs")?;

// 删除
ctx.kv_delete("user_prefs")?;
```

**底层签名**：

```text
astro_kv_get(key_ptr, key_len, out_ptr, out_max) → i32   // 返回写入字节数，0=不存在
astro_kv_set(key_ptr, key_len, val_ptr, val_len) → void
astro_kv_delete(key_ptr, key_len) → void
```

**示例：缓存翻译结果**

```rust
use astro_plugin_sdk::{Context, Result};
use sha2::{Sha256, Digest};

fn translate_with_cache(ctx: &mut Context, text: &str, lang: &str) -> Result<String> {
    // 生成缓存 key
    let cache_key = format!("cache:{}:{}", lang, hex::encode(Sha256::digest(text.as_bytes())));

    // 查缓存
    if let Some(cached) = ctx.kv_get(&cache_key)? {
        ctx.log_debug("命中翻译缓存");
        return Ok(String::from_utf8(cached)?);
    }

    // 调用翻译 API
    let translated = call_translate_api(ctx, text, lang)?;

    // 写入缓存
    ctx.kv_set(&cache_key, translated.as_bytes())?;

    Ok(translated)
}
```

### 4.3 astro_http_fetch -- HTTP 请求

发送 HTTP 请求。仅允许访问 `capabilities.network.allow_domains` 中声明的域名。

**需要权限**：`network = { allow_domains = [...] }`

**SDK 接口**：

```rust
// GET 请求
let response = ctx.http_get("https://api.example.com/data")?;
// response.status: u16
// response.body: Vec<u8>

// POST 请求
let response = ctx.http_post(
    "https://api.example.com/translate",
    "application/json",
    b"{\"text\": \"hello\", \"lang\": \"zh\"}",
)?;
```

**底层签名**：

```text
astro_http_fetch(req_ptr, req_len, out_ptr, out_max) → i32
  req: MessagePack 编码的 HttpRequest { method, url, headers, body }
  返回值: 正数=响应字节数, -1=域名不在白名单, -2=网络错误
```

**示例：调用外部翻译 API**

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
struct TranslateRequest {
    text: String,
    target: String,
}

#[derive(Deserialize)]
struct TranslateResponse {
    translated_text: String,
}

fn call_translate_api(ctx: &mut Context, text: &str, lang: &str) -> Result<String> {
    let req_body = serde_json::to_vec(&TranslateRequest {
        text: text.to_string(),
        target: lang.to_string(),
    })?;

    let resp = ctx.http_post(
        "https://api.example.com/v1/translate",
        "application/json",
        &req_body,
    )?;

    if resp.status != 200 {
        return Err(format!("翻译 API 返回状态码 {}", resp.status).into());
    }

    let result: TranslateResponse = serde_json::from_slice(&resp.body)?;
    Ok(result.translated_text)
}
```

### 4.4 astro_invoke_tool -- 调用内置工具

调用宿主注册的内置工具（如 `file_read`、`file_write`、`web_search` 等）。只能调用在 `capabilities.tools` 中声明的工具。

**需要权限**：`tools = ["file_read", ...]`

**SDK 接口**：

```rust
use serde_json::json;

// 调用 file_read 工具
let result = ctx.invoke_tool("file_read", json!({
    "path": "/tmp/data.txt"
}))?;

// result 是 serde_json::Value
println!("文件内容: {}", result);
```

**底层签名**：

```text
astro_invoke_tool(name_ptr, name_len, params_ptr, params_len, out_ptr, out_max) → i32
  返回值: 正数=结果字节数, -1=工具不存在或无权限, -2=执行错误
```

**示例：读取文件并处理**

```rust
#[astro_plugin_sdk::export]
fn analyze_file(ctx: &mut Context, path: String) -> Result<String> {
    // 读取文件
    let content = ctx.invoke_tool("file_read", json!({ "path": path }))?;

    let text = content.as_str().unwrap_or("");
    let line_count = text.lines().count();
    let word_count = text.split_whitespace().count();

    Ok(format!("文件分析结果：{} 行，{} 个词", line_count, word_count))
}
```

### 4.5 astro_emit / astro_subscribe -- 事件系统

发布和订阅事件，实现插件与宿主或其他插件之间的松耦合通信。

**需要权限**：`events = ["event_name", ...]`

**SDK 接口**：

```rust
// 发布事件
ctx.emit_event("translation_completed", json!({
    "source_lang": "en",
    "target_lang": "zh",
    "char_count": 1500,
}))?;

// 订阅事件（在 plugin_init 中调用）
let sub_id = ctx.subscribe("tool_called")?;
```

**底层签名**：

```text
astro_emit(name_ptr, name_len, data_ptr, data_len) → void
astro_subscribe(name_ptr, name_len) → i32  // 返回 subscription_id
```

### 4.6 宿主函数速查表

| 函数 | 用途 | 需要权限 | 返回值 |
|------|------|---------|--------|
| `astro_log` | 写入日志 | 无 | void |
| `astro_kv_get` | 读取 KV | `storage = true` | 字节数 / 0 |
| `astro_kv_set` | 写入 KV | `storage = true` | void |
| `astro_kv_delete` | 删除 KV | `storage = true` | void |
| `astro_http_fetch` | HTTP 请求 | `network.allow_domains` | 字节数 / -1 / -2 |
| `astro_invoke_tool` | 调用内置工具 | `tools = [...]` | 字节数 / -1 / -2 |
| `astro_emit` | 发布事件 | `events = [...]` | void |
| `astro_subscribe` | 订阅事件 | `events = [...]` | subscription_id |

---

## 5. 自定义工具开发

自定义工具是插件最常见的能力。注册一个工具后，Agent 可以像调用内置工具一样调用你的函数。

### 5.1 工具开发流程

1. 在 `plugin.toml` 的 `[[tools]]` 部分声明工具元数据和 JSON Schema
2. 在 Rust 代码中实现对应的处理函数
3. 编译并测试

### 5.2 Rust SDK 项目配置

```toml
# Cargo.toml
[package]
name = "my-translator-plugin"
version = "1.0.0"
edition = "2021"

[dependencies]
astro-plugin-sdk = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[lib]
crate-type = ["cdylib"]    # 编译为动态库（.wasm）
```

### 5.3 实现工具函数

```rust
// src/lib.rs

use astro_plugin_sdk::{plugin_main, Context, Result, Plugin};
use serde::{Deserialize, Serialize};
use serde_json::json;

// 声明插件入口
plugin_main!(TranslatorPlugin);

struct TranslatorPlugin;

impl Plugin for TranslatorPlugin {
    fn on_init(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.log_info("翻译插件已初始化");
        Ok(())
    }
}

// === 工具输入/输出结构体 ===

#[derive(Deserialize)]
struct TranslateInput {
    text: String,
    target_lang: String,
    source_lang: Option<String>,
}

#[derive(Serialize)]
struct TranslateOutput {
    translated_text: String,
    source_lang: String,
    target_lang: String,
    confidence: f64,
}

// === 导出工具处理函数 ===

/// 函数名必须与 plugin.toml 中 [[tools]] 的 handler 字段一致
#[astro_plugin_sdk::export]
fn tool_translate(ctx: &mut Context, input: TranslateInput) -> Result<TranslateOutput> {
    ctx.log_info(&format!(
        "翻译请求：{} -> {}，文本长度 {}",
        input.source_lang.as_deref().unwrap_or("auto"),
        input.target_lang,
        input.text.len()
    ));

    // 先查缓存
    let cache_key = format!("tr:{}:{}", input.target_lang, &input.text[..input.text.len().min(64)]);
    if let Some(cached) = ctx.kv_get(&cache_key)? {
        ctx.log_debug("命中翻译缓存");
        let result: TranslateOutput = serde_json::from_slice(&cached)?;
        return Ok(result);
    }

    // 调用外部翻译 API
    let req_body = serde_json::to_vec(&json!({
        "q": input.text,
        "target": input.target_lang,
        "source": input.source_lang,
    }))?;

    let resp = ctx.http_post(
        "https://api.example.com/v1/translate",
        "application/json",
        &req_body,
    )?;

    if resp.status != 200 {
        return Err(format!("翻译 API 错误：HTTP {}", resp.status).into());
    }

    let api_result: serde_json::Value = serde_json::from_slice(&resp.body)?;

    let output = TranslateOutput {
        translated_text: api_result["translated_text"].as_str().unwrap_or("").to_string(),
        source_lang: api_result["detected_source"].as_str().unwrap_or("unknown").to_string(),
        target_lang: input.target_lang,
        confidence: api_result["confidence"].as_f64().unwrap_or(0.0),
    };

    // 写入缓存
    let cached_bytes = serde_json::to_vec(&output)?;
    ctx.kv_set(&cache_key, &cached_bytes)?;

    Ok(output)
}
```

### 5.4 对应的 plugin.toml 配置

```toml
[plugin]
id = "my-translator"
name = "智能翻译插件"
version = "1.0.0"
description = "支持 50+ 语言的智能翻译，带缓存加速"
author = "your-name"
license = "MIT"
entry = "plugin.wasm"
min_host_version = "0.5.0"

[capabilities]
network = { allow_domains = ["api.example.com"] }
storage = true
tools = []
events = []

[limits]
memory_mb = 32
fuel = 10_000_000
timeout_secs = 10

[[tools]]
name = "translate"
handler = "tool_translate"
description = "将文本翻译为指定语言"
risk_level = "L1"

[tools.schema]
type = "object"
required = ["text", "target_lang"]

[tools.schema.properties.text]
type = "string"
description = "要翻译的文本"

[tools.schema.properties.target_lang]
type = "string"
description = "目标语言代码（如 en、zh、ja、ko、fr、de）"

[tools.schema.properties.source_lang]
type = "string"
description = "源语言代码（可选，自动检测）"
```

### 5.5 Agent 如何调用你的工具

注册完成后，Agent 可以在对话中自动发现并调用你的工具：

```text
用户: 帮我把这段话翻译成日语："你好，很高兴认识你"

Agent: 我来调用翻译工具。

[调用工具 translate]
输入: { "text": "你好，很高兴认识你", "target_lang": "ja" }
输出: { "translated_text": "こんにちは、お会いできて嬉しいです",
        "source_lang": "zh", "target_lang": "ja", "confidence": 0.95 }

翻译结果：こんにちは、お会いできて嬉しいです
```

---

## 6. Hook 开发

Hook 让你在 Agent 执行管线的关键节点介入处理。比如在工具调用前执行安全检查、在 LLM 调用前注入额外上下文、在工具调用后记录审计日志等。

### 6.1 可监听的事件

插件 Hook 可以监听以下事件：

| 事件 | 触发时机 | 典型用途 |
|------|---------|---------|
| `before_tool_execute` | 工具执行前 | 参数校验、安全检查、速率限制 |
| `after_tool_execute` | 工具执行后 | 输出过滤、审计记录 |
| `before_llm_call` | LLM 调用前 | 上下文注入、隐私过滤 |
| `after_llm_call` | LLM 调用后 | 响应审计、内容过滤 |
| `on_tool_error` | 工具执行出错 | 错误分类、告警 |
| `on_user_message` | 收到用户消息 | 输入预处理 |
| `on_assistant_message` | Agent 生成回复 | 输出后处理 |

### 6.2 在 plugin.toml 中注册 Hook

```toml
[[hooks]]
event = "before_tool_execute"
handler = "on_before_tool"          # WASM 导出函数名
priority = 550                      # 500-999 范围内，数值越小越先执行
tool = "file_write"                 # 可选：仅在特定工具调用时触发

[[hooks]]
event = "after_tool_execute"
handler = "on_after_tool"
priority = 560
```

### 6.3 实现 Hook 处理函数

Hook 处理函数接收一个 `HookContext`，包含当前事件的负载数据。函数返回一个 `HookAction` 指示宿主如何继续执行。

```rust
// src/lib.rs

use astro_plugin_sdk::{HookContext, HookAction, HookPayload};

/// 代码质量检查 Hook：在 file_write 前检查代码质量
#[astro_plugin_sdk::hook]
fn on_before_tool(ctx: &mut Context, hook_ctx: &mut HookContext) -> Result<HookAction> {
    // 获取工具调用信息
    if let HookPayload::ToolRequest { tool_name, arguments, .. } = &hook_ctx.payload {
        // 仅处理 file_write
        if tool_name != "file_write" {
            return Ok(HookAction::Continue);
        }

        ctx.log_info(&format!("检查文件写入：{:?}", arguments));

        // 获取待写入的内容
        let content = arguments
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // 检查是否包含调试代码
        let debug_patterns = ["console.log", "println!", "print(", "debugger"];
        let mut warnings = Vec::new();
        for pattern in &debug_patterns {
            if content.contains(pattern) {
                warnings.push(format!("检测到调试代码：{}", pattern));
            }
        }

        if !warnings.is_empty() {
            ctx.log_warn(&format!("代码质量警告：{}", warnings.join(", ")));
            // 不阻止执行，但记录警告
            hook_ctx.set_metadata(
                "lint_warnings",
                serde_json::json!(warnings),
            );
        }
    }

    Ok(HookAction::Continue)
}

/// 审计日志 Hook：在工具执行后记录日志
#[astro_plugin_sdk::hook]
fn on_after_tool(ctx: &mut Context, hook_ctx: &mut HookContext) -> Result<HookAction> {
    if let HookPayload::ToolResponse { tool_name, duration_ms, is_error, .. } = &hook_ctx.payload {
        // 记录到插件私有存储
        let log_entry = serde_json::json!({
            "tool": tool_name,
            "duration_ms": duration_ms,
            "is_error": is_error,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });

        let log_key = format!("audit:{}", chrono::Utc::now().timestamp_millis());
        ctx.kv_set(&log_key, log_entry.to_string().as_bytes())?;

        ctx.log_debug(&format!("审计记录：{} 工具 {} ({}ms)",
            if *is_error { "失败" } else { "成功" },
            tool_name,
            duration_ms
        ));
    }

    Ok(HookAction::Continue)
}
```

### 6.4 HookAction 返回值

| 返回值 | 效果 |
|--------|------|
| `HookAction::Continue` | 继续执行后续 Hook 和被拦截的操作 |
| `HookAction::Skip` | 跳过当前被拦截的操作（如跳过工具执行），后续 Hook 继续 |
| `HookAction::Abort { reason }` | 中止整个管线，后续 Hook 不再执行 |

```rust
// 阻止危险操作的示例
#[astro_plugin_sdk::hook]
fn on_before_tool(ctx: &mut Context, hook_ctx: &mut HookContext) -> Result<HookAction> {
    if let HookPayload::ToolRequest { tool_name, arguments, .. } = &hook_ctx.payload {
        if tool_name == "shell_exec" {
            let cmd = arguments.get("command").and_then(|v| v.as_str()).unwrap_or("");
            if cmd.contains("rm -rf") {
                return Ok(HookAction::Abort {
                    reason: "安全策略：禁止执行 rm -rf 命令".to_string(),
                });
            }
        }
    }
    Ok(HookAction::Continue)
}
```

### 6.5 Hook 优先级

插件 Hook 的优先级必须在 500-999 范围内。系统按数值从小到大的顺序执行 Hook。完整的优先级分段如下：

```text
0-99    系统级（可观测性、基础设施） -- 系统保留
100-199 安全级（权限检查、风险评估） -- 系统保留
200-299 隐私级（数据过滤、脱敏）    -- 系统保留
300-399 业务级（预算、审计）        -- 系统保留
400-499 用户配置级（hooks.toml）   -- 用户保留
500-999 插件级                    -- 你的 Hook 在这里
```

你的 Hook 始终在系统内置 Hook 和用户自定义 Hook 之后执行。

---

## 7. Skill 开发

Skill 是结构化的提示词模块，打包在插件中随插件一起分发。Skill 不是可执行代码，而是教会 Agent "在特定场景下如何行动"的指令文件。

### 7.1 Skill 目录结构

在插件目录下创建 `skills/` 目录，每个 Skill 是一个子目录：

```text
my-plugin/
├── plugin.toml
├── src/lib.rs
└── skills/
    └── smart-translate/
        ├── SKILL.md              # Skill 入口文件（必须）
        ├── examples/             # 示例输出（可选）
        │   └── translation-example.md
        └── scripts/              # 辅助脚本（可选）
            └── detect-lang.sh
```

### 7.2 SKILL.md 格式

Skill 以 YAML frontmatter + Markdown 正文的格式编写：

```markdown
---
name: smart-translate
description: 使用翻译插件进行高质量多语言翻译，支持术语表和上下文感知
when_to_use: >-
  用户要求翻译文章、文档或代码注释时使用，支持批量翻译和术语一致性检查

# 调用控制
disable-model-invocation: false    # 允许模型自动触发
user-invocable: true               # 用户可通过 /smart-translate 调用

# 工具权限
allowed-tools: translate file_read file_write

# 参数定义
arguments:
  - name: target_lang
    description: 目标语言
    required: true
  - name: file_path
    description: 要翻译的文件路径（可选）
    required: false
argument-hint: "<target_lang> [file_path]"

# Astro 扩展
version: "1.0.0"
tags: [translate, i18n, multilingual]
trigger_patterns:
  - "翻译"
  - "translate"
  - "多语言"
---

# 智能翻译

你正在使用智能翻译 Skill。请按以下步骤操作：

## 步骤

1. 确认目标语言：$target_lang
2. 如果提供了文件路径，先读取文件内容：
   - 文件路径：$file_path
3. 使用 `translate` 工具进行翻译
4. 如果翻译结果中有技术术语，检查术语一致性

## 翻译质量要求

- 保持原文格式（Markdown 标记、代码块等）
- 技术术语保持英文或提供中英对照
- 对于长文本，分段翻译以保证质量

## 输出格式

翻译完成后，输出：
- 翻译结果
- 源语言（自动检测结果）
- 字符数统计
```

### 7.3 在 plugin.toml 中注册 Skill

```toml
[[skills]]
file = "skills/smart-translate/SKILL.md"
```

### 7.4 Skill 中引用插件工具

Skill 正文中可以自然地引用插件注册的工具。当 Skill 被激活时，Agent 会自动使用 `allowed-tools` 中声明的工具。如果你的插件注册了 `translate` 工具，Skill 就可以直接指导 Agent 调用它。

### 7.5 动态上下文注入

Skill 正文支持动态注入命令输出，使 Skill 能携带当前环境的实时信息：

```markdown
当前项目的语言文件：
!`find . -name "*.json" -path "*/locales/*" | head -10`

已有的翻译语言：
!`ls locales/ 2>/dev/null || echo "未检测到 locales 目录"`
```

加载时系统会执行这些命令，将输出替换到 Skill 正文中。

---

## 8. 测试与调试

### 8.1 本地测试

Astro Agent CLI 提供了完整的插件测试工具链：

```bash
# 编译插件
astro plugin build

# 运行所有测试
astro plugin test

# 运行特定测试
astro plugin test --filter "test_translate"

# 以调试模式运行（输出详细日志）
astro plugin test --verbose
```

### 8.2 编译插件

```bash
# 编译为 WASM
cargo build --target wasm32-wasip1 --release

# 或使用 CLI 快捷命令
astro plugin build
# 输出：target/wasm32-wasip1/release/my_plugin.wasm -> plugin.wasm
```

### 8.3 编写单元测试

Rust SDK 提供了 Mock 宿主函数，你可以在纯 Rust 环境中测试插件逻辑：

```rust
// tests/integration_test.rs

use astro_plugin_sdk::testing::{MockContext, MockHttpResponse};
use my_translator_plugin::tool_translate;

#[test]
fn test_translate_basic() {
    let mut ctx = MockContext::new();

    // Mock HTTP 响应
    ctx.mock_http_post(
        "https://api.example.com/v1/translate",
        MockHttpResponse {
            status: 200,
            body: serde_json::to_vec(&serde_json::json!({
                "translated_text": "Hello",
                "detected_source": "zh",
                "confidence": 0.95,
            })).unwrap(),
        },
    );

    // 调用工具函数
    let input = TranslateInput {
        text: "你好".to_string(),
        target_lang: "en".to_string(),
        source_lang: None,
    };

    let result = tool_translate(&mut ctx, input).unwrap();
    assert_eq!(result.translated_text, "Hello");
    assert_eq!(result.source_lang, "zh");
    assert!(result.confidence > 0.9);
}

#[test]
fn test_translate_with_cache() {
    let mut ctx = MockContext::new();

    // 预设缓存
    ctx.mock_kv_set("tr:en:你好", b"{\"translated_text\":\"Hello\",\"source_lang\":\"zh\",\"target_lang\":\"en\",\"confidence\":0.95}");

    let input = TranslateInput {
        text: "你好".to_string(),
        target_lang: "en".to_string(),
        source_lang: None,
    };

    let result = tool_translate(&mut ctx, input).unwrap();
    assert_eq!(result.translated_text, "Hello");

    // 验证未发出 HTTP 请求（命中了缓存）
    assert_eq!(ctx.http_call_count(), 0);
}

#[test]
fn test_translate_api_error() {
    let mut ctx = MockContext::new();

    ctx.mock_http_post(
        "https://api.example.com/v1/translate",
        MockHttpResponse { status: 500, body: b"Internal Server Error".to_vec() },
    );

    let input = TranslateInput {
        text: "你好".to_string(),
        target_lang: "en".to_string(),
        source_lang: None,
    };

    let result = tool_translate(&mut ctx, input);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("500"));
}
```

### 8.4 Mock 宿主函数

`MockContext` 提供以下 mock 能力：

```rust
let mut ctx = MockContext::new();

// Mock KV 存储
ctx.mock_kv_set("key", b"value");
let val = ctx.kv_get("key").unwrap();

// Mock HTTP 响应
ctx.mock_http_get("https://api.example.com/data", MockHttpResponse { ... });
ctx.mock_http_post("https://api.example.com/data", MockHttpResponse { ... });

// Mock 工具调用
ctx.mock_tool_result("file_read", json!("file content here"));

// 验证日志输出
assert!(ctx.logs().iter().any(|l| l.contains("翻译插件已初始化")));

// 验证 HTTP 调用次数
assert_eq!(ctx.http_call_count(), 1);

// 验证事件发布
assert!(ctx.emitted_events().iter().any(|e| e.name == "translation_completed"));
```

### 8.5 调试日志

在开发阶段，使用 `ctx.log_debug()` 输出调试信息。在 `astro plugin test --verbose` 模式下，所有日志级别都会输出到终端。

```rust
ctx.log_debug(&format!("请求参数：{:?}", input));
ctx.log_debug(&format!("API 响应：{} bytes", resp.body.len()));
```

发布到市场后，debug 级别的日志默认不输出，但用户可在设置中开启插件调试模式查看。

### 8.6 在本地 Astro Agent 中加载测试

```bash
# 构建插件
astro plugin build

# 在本地 Astro Agent 中加载插件（不安装到市场）
astro plugin load ./plugin.wasm

# 查看已加载的插件
astro plugin list

# 卸载本地插件
astro plugin unload my-translator
```

---

## 9. 打包与发布

### 9.1 构建 WASM

```bash
# Release 构建（优化体积和性能）
cargo build --target wasm32-wasip1 --release

# 可选：使用 wasm-opt 进一步优化
wasm-opt -O3 target/wasm32-wasip1/release/my_plugin.wasm -o plugin.wasm
```

### 9.2 创建 .agent 包

`.agent` 包是一个 ZIP 压缩包，包含插件的所有文件。使用 CLI 一键打包：

```bash
# 自动构建 + 打包
astro plugin pack

# 输出：my-translator-1.0.0.agent
```

CLI 会自动执行以下步骤：

1. 编译 WASM（`cargo build --target wasm32-wasip1 --release`）
2. Lint 检查（验证 plugin.toml、检查敏感文件等）
3. 生成 `manifest.json`（从 plugin.toml 转换）
4. 将所有文件打包为 ZIP

打包后的 `.agent` 文件内部结构：

```text
my-translator-1.0.0.agent (ZIP)
├── manifest.json              # 包元数据（自动生成）
├── signature.sig              # 数字签名（可选）
├── plugins/
│   └── my-translator/
│       ├── plugin.toml
│       └── plugin.wasm
├── skills/
│   └── smart-translate/
│       └── SKILL.md
├── assets/
│   └── icon.png
└── README.md
```

### 9.3 签名

使用开发者密钥对包进行签名，证明包的来源和完整性。签名不是强制的，但签名包在市场中获得"已验证"标记，显著提升用户信任度。

```bash
# 首次使用：生成开发者密钥对
astro keys generate
# 生成文件：
#   ~/.astro/keys/publisher.key  (私钥，妥善保管)
#   ~/.astro/keys/publisher.pub  (公钥，发布到注册中心)

# 对包签名
astro plugin sign my-translator-1.0.0.agent

# 验证签名
astro plugin verify my-translator-1.0.0.agent
```

签名使用 Ed25519 算法。私钥文件权限自动设置为 `0600`（仅所有者可读写），请务必妥善保管，不要提交到版本控制系统。

### 9.4 发布到市场

```bash
# 发布到 Astro Agent 市场
astro plugin publish my-translator-1.0.0.agent

# 发布前会执行以下检查：
# 1. Lint 检查（manifest、敏感文件、文件引用完整性）
# 2. 签名验证（如已签名）
# 3. 版本号冲突检查
# 4. 上传到注册中心
```

### 9.5 版本管理

遵循语义化版本（SemVer）规范：

| 版本号变化 | 含义 | 示例 |
|-----------|------|------|
| `1.0.0 -> 1.0.1` | 补丁：bug 修复，无 API 变化 | 修复翻译缓存失效 |
| `1.0.0 -> 1.1.0` | 次版本：新增功能，向后兼容 | 新增语言检测工具 |
| `1.0.0 -> 2.0.0` | 主版本：有破坏性变更 | 工具参数格式改变 |

更新版本时，修改 `plugin.toml` 中的 `version` 字段，然后重新打包发布。

```bash
# 修改版本号后重新发布
astro plugin pack
astro plugin publish my-translator-1.1.0.agent
```

---

## 10. 安全与权限

### 10.1 Capabilities 权限模型

插件的权限通过 `plugin.toml` 的 `[capabilities]` 部分声明。安装时，宿主会向用户展示完整的权限清单，用户确认后才授予。

**设计原则：最小权限**。只声明你真正需要的权限，过多的权限声明会降低用户的安装意愿。

权限按风险等级划分：

| 权限 | 风险等级 | 说明 |
|------|---------|------|
| `storage` | L0（无风险） | 插件私有 KV 存储，完全隔离 |
| `events` | L0（无风险） | 事件订阅/发布 |
| `tools = ["file_read"]` | L1（低风险） | 调用只读工具 |
| `tools = ["file_write"]` | L2（中风险） | 调用可写工具 |
| `network` | L2（中风险） | 网络访问（限白名单域名） |
| `tools = ["shell_exec"]` | L3（高风险） | 调用 Shell 命令执行工具 |

### 10.2 沙箱限制

你的插件运行在以下限制之中：

| 限制项 | 说明 |
|--------|------|
| 内存隔离 | WASM 线性内存隔离，无法读写宿主或其他插件的内存 |
| 无文件系统 | 不能直接访问文件系统，必须通过 `astro_invoke_tool` 调用 `file_read` 等工具 |
| 网络白名单 | 只能请求 `allow_domains` 中声明的域名 |
| CPU 限制 | 通过 wasmtime Fuel 机制限制 CPU 消耗，防止死循环 |
| 内存上限 | 线性内存不超过 `limits.memory_mb` |
| 执行超时 | 单次调用不超过 `limits.timeout_secs` |
| KV 隔离 | 每个插件只能访问自己的 KV 命名空间 |
| 无直接数据库访问 | 数据库操作必须通过宿主工具中转 |

### 10.3 市场审查流程

发布到市场的插件需要通过以下审查：

1. **自动化检查**：Lint 检查（manifest 完整性、敏感文件检测、路径穿越防护）
2. **权限审计**：`plugin.toml` 的 capabilities 与 `manifest.json` 的 permissions 交叉验证
3. **资源限制验证**：memory_mb <= 128、timeout_secs <= 30、fuel <= 50,000,000
4. **未签名包限制**：未签名的包不允许声明 `shell_exec` 等高危权限

### 10.4 安全编码建议

```rust
// 1. 验证所有外部输入
fn tool_handler(ctx: &mut Context, input: UserInput) -> Result<Output> {
    // 检查输入长度
    if input.text.len() > 100_000 {
        return Err("输入文本过长（最大 100KB）".into());
    }

    // 检查路径安全（如果接受文件路径参数）
    if input.path.contains("..") {
        return Err("路径不允许包含 ..".into());
    }

    // ...
}

// 2. 处理所有错误，避免 panic
fn safe_handler(ctx: &mut Context) -> Result<String> {
    let resp = ctx.http_get("https://api.example.com/data")
        .map_err(|e| format!("网络请求失败：{}", e))?;

    if resp.status != 200 {
        return Err(format!("API 返回错误状态码：{}", resp.status).into());
    }

    Ok("success".to_string())
}

// 3. 不要在日志中泄露敏感信息
ctx.log_info("处理请求");         // 好
ctx.log_info(&format!("API Key: {}", api_key));  // 绝对不要这样做
```

---

## 11. 最佳实践

### 11.1 保持插件小巧

一个插件应该专注于一个功能领域。如果你的插件做了太多不相关的事情，考虑拆分为多个插件。

```text
推荐：
  translator-plugin   -> 专注翻译功能
  code-quality-plugin -> 专注代码质量

不推荐：
  super-plugin -> 既翻译又代码检查又发邮件又管理数据库
```

### 11.2 优雅处理错误

永远不要让插件 panic。所有可能失败的操作都应该返回 `Result`，提供有意义的错误消息。

```rust
// 推荐
fn my_tool(ctx: &mut Context, input: Input) -> Result<Output> {
    let data = ctx.http_get(&url)
        .map_err(|e| format!("无法获取数据（{}）：{}", url, e))?;

    let parsed: MyData = serde_json::from_slice(&data.body)
        .map_err(|e| format!("响应数据格式错误：{}", e))?;

    Ok(process(parsed))
}

// 不推荐
fn my_tool(ctx: &mut Context, input: Input) -> Result<Output> {
    let data = ctx.http_get(&url).unwrap();     // 可能 panic
    let parsed: MyData = serde_json::from_slice(&data.body).unwrap();  // 可能 panic
    Ok(process(parsed))
}
```

### 11.3 版本化你的 API

如果你的工具参数格式可能在未来版本中变化，从一开始就考虑向后兼容。

```rust
#[derive(Deserialize)]
struct TranslateInput {
    text: String,
    target_lang: String,
    // v1.1 新增字段，使用 Option 保持向后兼容
    #[serde(default)]
    glossary: Option<HashMap<String, String>>,
    // v1.2 新增字段
    #[serde(default = "default_format")]
    output_format: String,
}

fn default_format() -> String {
    "plain".to_string()
}
```

### 11.4 为用户撰写文档

在 README.md 中至少包含：

- 插件做什么（一句话说明）
- 安装方式
- 提供的工具列表及每个工具的用法示例
- 需要的权限及其用途解释
- 配置说明（如需要 API Key 等）
- 常见问题

### 11.5 性能优化

```rust
// 1. 利用 KV 缓存减少网络请求
if let Some(cached) = ctx.kv_get(&cache_key)? {
    return Ok(cached);
}

// 2. 控制 HTTP 响应体大小（避免 OOM）
let resp = ctx.http_get(&url)?;
if resp.body.len() > 1_000_000 {
    return Err("响应体过大（超过 1MB）".into());
}

// 3. 避免不必要的序列化/反序列化
// 如果只需要 JSON 中的一个字段，不必反序列化整个对象
let value: serde_json::Value = serde_json::from_slice(&resp.body)?;
let name = value["data"]["name"].as_str().unwrap_or("unknown");
```

### 11.6 测试覆盖

为每个工具函数编写至少三类测试：

1. **正常路径**：标准输入，期望正确输出
2. **边界条件**：空输入、超长输入、特殊字符
3. **错误路径**：网络错误、API 错误、无效输入

---

## 12. 示例插件：word-count

下面我们从零开始构建一个完整的 "word-count" 工具插件，实现文本字数统计和文件分析功能。

### 12.1 创建项目

```bash
astro plugin init word-count
cd word-count
```

### 12.2 plugin.toml

```toml
[plugin]
id = "word-count"
name = "字数统计插件"
version = "1.0.0"
description = "统计文本或文件的字数、字符数和行数"
author = "astro-developer"
license = "MIT"
homepage = "https://github.com/astro-developer/word-count"
entry = "plugin.wasm"
min_host_version = "0.5.0"

[capabilities]
storage = true
tools = ["file_read"]
events = []

[limits]
memory_mb = 16
fuel = 5_000_000
timeout_secs = 3

# 工具 1：统计文本字数
[[tools]]
name = "word_count"
handler = "tool_word_count"
description = "统计文本的字数、字符数、行数"
risk_level = "L0"

[tools.schema]
type = "object"
required = ["text"]

[tools.schema.properties.text]
type = "string"
description = "要统计的文本"

# 工具 2：统计文件字数
[[tools]]
name = "file_word_count"
handler = "tool_file_word_count"
description = "统计指定文件的字数、字符数、行数"
risk_level = "L1"

[tools.schema]
type = "object"
required = ["path"]

[tools.schema.properties.path]
type = "string"
description = "文件路径"
```

### 12.3 Cargo.toml

```toml
[package]
name = "word-count-plugin"
version = "1.0.0"
edition = "2021"

[dependencies]
astro-plugin-sdk = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
unicode-segmentation = "1.10"

[lib]
crate-type = ["cdylib"]
```

### 12.4 src/lib.rs

```rust
use astro_plugin_sdk::{plugin_main, Context, Plugin, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use unicode_segmentation::UnicodeSegmentation;

// ============================================================
// 插件入口
// ============================================================

plugin_main!(WordCountPlugin);

struct WordCountPlugin;

impl Plugin for WordCountPlugin {
    fn on_init(&mut self, ctx: &mut Context) -> Result<()> {
        ctx.log_info("字数统计插件 v1.0.0 已初始化");
        Ok(())
    }
}

// ============================================================
// 数据结构
// ============================================================

#[derive(Deserialize)]
struct TextInput {
    text: String,
}

#[derive(Deserialize)]
struct FileInput {
    path: String,
}

#[derive(Serialize)]
struct WordCountResult {
    /// 总字符数（含空白）
    char_count: usize,
    /// 总字符数（不含空白）
    char_count_no_spaces: usize,
    /// 单词数（英文按空格分词，中文按字分词）
    word_count: usize,
    /// 行数
    line_count: usize,
    /// 段落数（以空行分隔）
    paragraph_count: usize,
    /// 预估阅读时间（分钟，按 250 词/分钟）
    estimated_reading_minutes: f64,
}

// ============================================================
// 核心逻辑
// ============================================================

fn count_words(text: &str) -> WordCountResult {
    let char_count = text.chars().count();
    let char_count_no_spaces = text.chars().filter(|c| !c.is_whitespace()).count();
    let line_count = if text.is_empty() { 0 } else { text.lines().count() };

    // 单词计数：处理中英文混合
    let word_count = text
        .unicode_words()
        .count();

    // 段落计数：以空行分隔
    let paragraph_count = if text.trim().is_empty() {
        0
    } else {
        text.split("\n\n")
            .filter(|p| !p.trim().is_empty())
            .count()
    };

    let estimated_reading_minutes = word_count as f64 / 250.0;

    WordCountResult {
        char_count,
        char_count_no_spaces,
        word_count,
        line_count,
        paragraph_count,
        estimated_reading_minutes,
    }
}

// ============================================================
// 工具导出函数
// ============================================================

/// 统计文本字数
#[astro_plugin_sdk::export]
fn tool_word_count(ctx: &mut Context, input: TextInput) -> Result<WordCountResult> {
    ctx.log_info(&format!("统计文本字数，输入长度：{}", input.text.len()));

    if input.text.is_empty() {
        return Ok(WordCountResult {
            char_count: 0,
            char_count_no_spaces: 0,
            word_count: 0,
            line_count: 0,
            paragraph_count: 0,
            estimated_reading_minutes: 0.0,
        });
    }

    let result = count_words(&input.text);
    ctx.log_debug(&format!(
        "统计结果：{} 字符, {} 词, {} 行",
        result.char_count, result.word_count, result.line_count
    ));

    // 记录累计统计到 KV（可选功能）
    update_cumulative_stats(ctx, &result)?;

    Ok(result)
}

/// 统计文件字数
#[astro_plugin_sdk::export]
fn tool_file_word_count(ctx: &mut Context, input: FileInput) -> Result<WordCountResult> {
    ctx.log_info(&format!("统计文件字数：{}", input.path));

    // 安全检查：不允许路径穿越
    if input.path.contains("..") {
        return Err("文件路径不允许包含 '..'".into());
    }

    // 通过宿主工具读取文件
    let file_content = ctx.invoke_tool("file_read", json!({
        "path": input.path
    }))?;

    let text = file_content
        .as_str()
        .ok_or("file_read 返回的不是字符串")?;

    let result = count_words(text);
    ctx.log_info(&format!(
        "文件 {} 统计结果：{} 字符, {} 词, {} 行, 预估阅读 {:.1} 分钟",
        input.path, result.char_count, result.word_count,
        result.line_count, result.estimated_reading_minutes
    ));

    Ok(result)
}

// ============================================================
// 辅助函数
// ============================================================

/// 更新累计统计（使用 KV 存储持久化）
fn update_cumulative_stats(ctx: &mut Context, result: &WordCountResult) -> Result<()> {
    let key = "cumulative_stats";

    let mut total_words: u64 = 0;
    let mut total_calls: u64 = 0;

    if let Some(existing) = ctx.kv_get(key)? {
        if let Ok(stats) = serde_json::from_slice::<serde_json::Value>(&existing) {
            total_words = stats["total_words"].as_u64().unwrap_or(0);
            total_calls = stats["total_calls"].as_u64().unwrap_or(0);
        }
    }

    total_words += result.word_count as u64;
    total_calls += 1;

    let updated = serde_json::json!({
        "total_words": total_words,
        "total_calls": total_calls,
    });

    ctx.kv_set(key, updated.to_string().as_bytes())?;
    Ok(())
}
```

### 12.5 编写测试

```rust
// tests/integration_test.rs

#[cfg(test)]
mod tests {
    use astro_plugin_sdk::testing::MockContext;

    #[test]
    fn test_word_count_english() {
        let result = super::count_words("Hello world, this is a test.");
        assert_eq!(result.word_count, 6);
        assert_eq!(result.line_count, 1);
        assert_eq!(result.paragraph_count, 1);
    }

    #[test]
    fn test_word_count_chinese() {
        let result = super::count_words("你好世界，这是一个测试。");
        assert!(result.word_count > 0);
        assert_eq!(result.line_count, 1);
    }

    #[test]
    fn test_word_count_multiline() {
        let text = "第一行\n第二行\n\n第二段\n第三行";
        let result = super::count_words(text);
        assert_eq!(result.line_count, 4);
        assert_eq!(result.paragraph_count, 2);
    }

    #[test]
    fn test_word_count_empty() {
        let result = super::count_words("");
        assert_eq!(result.char_count, 0);
        assert_eq!(result.word_count, 0);
        assert_eq!(result.line_count, 0);
    }

    #[test]
    fn test_file_word_count_with_mock() {
        let mut ctx = MockContext::new();

        ctx.mock_tool_result("file_read", serde_json::json!(
            "Hello world.\nThis is line two.\n\nNew paragraph."
        ));

        let input = super::FileInput {
            path: "/tmp/test.txt".to_string(),
        };

        let result = super::tool_file_word_count(&mut ctx, input).unwrap();
        assert!(result.word_count > 0);
        assert_eq!(result.line_count, 4);
        assert_eq!(result.paragraph_count, 2);
    }

    #[test]
    fn test_file_path_traversal_blocked() {
        let mut ctx = MockContext::new();

        let input = super::FileInput {
            path: "../../../etc/passwd".to_string(),
        };

        let result = super::tool_file_word_count(&mut ctx, input);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(".."));
    }

    #[test]
    fn test_reading_time_estimate() {
        // 250 words -> 1 minute
        let words: Vec<&str> = std::iter::repeat("word").take(250).collect();
        let text = words.join(" ");
        let result = super::count_words(&text);
        assert!((result.estimated_reading_minutes - 1.0).abs() < 0.01);
    }
}
```

### 12.6 构建、测试、打包

```bash
# 1. 运行测试
cargo test

# 2. 编译 WASM
astro plugin build

# 3. 本地加载测试
astro plugin load ./plugin.wasm

# 在 Agent 中测试
# 用户: 统计一下这段话的字数："Rust 是一门系统编程语言，注重安全性和性能。"

# 4. 打包为 .agent
astro plugin pack
# 输出: word-count-1.0.0.agent

# 5. 签名（可选）
astro plugin sign word-count-1.0.0.agent

# 6. 发布
astro plugin publish word-count-1.0.0.agent
```

### 12.7 用户安装后的体验

用户从市场安装 word-count 插件后，可以直接在对话中使用：

```text
用户: 帮我统计 src/main.rs 文件有多少行代码

Agent: 我来用文件字数统计工具分析这个文件。

[调用工具 file_word_count]
输入: { "path": "src/main.rs" }
输出: {
  "char_count": 3842,
  "char_count_no_spaces": 2916,
  "word_count": 487,
  "line_count": 142,
  "paragraph_count": 28,
  "estimated_reading_minutes": 1.9
}

文件 src/main.rs 的统计结果：
- 142 行代码
- 487 个词
- 3,842 个字符
- 预估阅读时间约 2 分钟
```

---

## 相关文档

- [04-WASM插件沙箱API设计.md](../_v0.3规划/04-WASM插件沙箱API设计.md) -- WASM 运行时选型、Host Functions 底层实现、Capabilities 权限模型、内存传输协议
- [06-Agent市场详细设计.md](../_v0.3规划/06-Agent市场详细设计.md) -- `.agent` 包格式规范、市场安装/卸载/更新流程、数字签名、安全审查
- [08-Hooks系统详细设计.md](../01-核心引擎层/08-Hooks系统详细设计.md) -- Hook 事件目录、HookRegistry、HookPipeline、优先级分段
- [01-Skills系统详细设计.md](01-Skills系统详细设计.md) -- SKILL.md 完整格式规范、动态上下文注入、BM25 召回机制
