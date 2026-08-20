# POC 验证报告

> 文档状态：待执行 | 阶段：可行性评估 | 属性：可选

---

## 说明

本报告记录立项前需完成的最小原型（POC）验证结果。每个 POC 针对一个高风险技术点，用最少的代码验证关键假设。

---

## POC-01：sqlite-vec 向量检索性能

### POC-01 验证目标

验证 sqlite-vec 在 10 万文档块场景下，向量相似度检索延迟是否满足需求（< 200ms）。

### POC-01 验证方案

```rust
// 伪代码：向量检索性能测试
use sqlite_vec::*;

async fn poc_sqlite_vec() {
    let db = rusqlite::Connection::open("poc.db")?;
    sqlite_vec::load(&db)?;

    // 写入 10 万条 1536 维向量（OpenAI embedding 维度）
    for i in 0..100_000 {
        let vec: Vec<f32> = generate_random_vec(1536);
        db.execute("INSERT INTO embeddings(vec) VALUES(?)", [vec])?;
    }

    // 测试检索延迟
    let query_vec: Vec<f32> = generate_random_vec(1536);
    let start = std::time::Instant::now();
    let results = db.query(
        "SELECT rowid, distance FROM embeddings ORDER BY vec <-> ? LIMIT 10",
        [query_vec]
    )?;
    println!("检索耗时: {:?}", start.elapsed());
}
```

### POC-01 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| 10 万块检索延迟（Top-10） | < 200ms | 待测 |
| 内存占用（索引加载后） | < 500MB | 待测 |
| 写入速度（1 万块） | < 30s | 待测 |

### POC-01 结论

- [ ] ✅ 通过 — 使用 sqlite-vec 作为向量存储
- [ ] ❌ 不通过 — 改用 qdrant 本地模式

---

## POC-02：多 Provider API 统一抽象

### POC-02 验证目标

验证统一 `TextClient` trait 可以同时支持 Anthropic Claude 和 OpenAI 的流式文本对话，且切换 Provider 时上层代码无需修改。

### POC-02 验证方案

```rust
#[async_trait]
pub trait TextClient: Send + Sync {
    async fn chat_stream(
        &self,
        messages: &[Message],
        options: &ChatOptions,
    ) -> Result<impl Stream<Item = Result<String>>>;
}

// 分别实现 AnthropicClient 和 OpenAiClient
// 验证：同一段调用代码，切换 Provider 只需换实现体
```

### POC-02 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| Claude 流式输出首字延迟 | < 800ms | 待测 |
| OpenAI 流式输出首字延迟 | < 800ms | 待测 |
| Provider 切换是否需要修改调用代码 | 否 | 待测 |
| 错误类型统一映射 | 是 | 待测 |

### POC-02 结论

- [ ] ✅ 通过
- [ ] ❌ 不通过 — 重新设计 trait 抽象

---

## POC-03：Tauri v2 多平台打包

### POC-03 验证目标

验证 Tauri v2 可以在 macOS（arm64 + x86_64）和 Windows（x86_64）上正确打包并运行，包含系统托盘、自定义窗口栏、文件拖拽、系统原生分享框等桌面特性。

### POC-03 验证方案

构建最小 Tauri 应用，包含：

- 自定义标题栏（移除系统默认标题栏）
- 系统托盘图标和右键菜单
- 文件拖拽接收（接收图片文件）
- 单实例检测（防止多开）
- 自动更新检查（Tauri Updater）
- 系统原生分享框（`tauri-plugin-share`，传递 .html / .zip 文件）

### POC-03 验收标准

| 平台 | 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- | ---- |
| macOS arm64 | 安装包大小 | < 20MB | 待测 |
| macOS arm64 | 冷启动时间 | < 2s | 待测 |
| Windows x64 | 安装包大小 | < 25MB | 待测 |
| Windows x64 | 冷启动时间 | < 3s | 待测 |
| 全平台 | 文件拖拽接收 | 正常 | 待测 |
| 全平台 | 系统托盘 | 正常 | 待测 |
| macOS | 系统原生分享框（Share Sheet）可唤起并传递 .html/.zip 文件 | 正常 | 待测 |
| Windows | 系统原生分享框（Share 菜单）可唤起并传递 .html/.zip 文件 | 正常 | 待测 |

### POC-03 结论

- [ ] ✅ 通过 — Tauri v2 方案确认
- [ ] ❌ 不通过 — 评估 Electron 作为备选

---

## POC-04：Rhai 脚本动态工具执行

### POC-04 验证目标

验证 Rhai 脚本可以在 Rust 中安全执行，可以调用注册的 Rust 函数，且执行时间和内存可控。

### POC-04 验证方案

```rust
use rhai::{Engine, Scope};

fn poc_rhai() {
    let mut engine = Engine::new();

    // 注册 Rust 函数供 Rhai 调用
    engine.register_fn("fetch_url", |url: &str| -> String {
        // 模拟 HTTP 请求
        format!("content of {}", url)
    });

    // 执行 Rhai 脚本
    let script = r#"
        let result = fetch_url("https://example.com");
        result + " processed"
    "#;

    let result = engine.eval::<String>(script)?;
    println!("结果: {}", result);
}
```

### POC-04 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| 脚本执行无 panic | 是 | 待测 |
| 可调用注册的 Rust 函数 | 是 | 待测 |
| 恶意脚本（死循环）可被超时终止 | 是 | 待测 |
| 内存使用可控 | < 10MB/脚本 | 待测 |

### POC-04 结论

- [ ] ✅ 通过
- [ ] ❌ 不通过 — 改用 deno_core（JS 引擎）

---

## POC-05：MCP 客户端（stdio 模式）

### POC-05 验证目标

验证可以通过 stdio 与外部 MCP Server 进程通信，发现工具列表并调用工具。

### POC-05 验证方案

启动一个公开的 MCP Server（如 `@modelcontextprotocol/server-filesystem`），通过 Rust 代码：

1. 生成子进程
2. 通过 stdin/stdout 发送 JSON-RPC 消息
3. 接收 `tools/list` 响应
4. 调用一个工具并接收结果

### POC-05 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| 成功启动 MCP Server 进程 | 是 | 待测 |
| 正确解析 tools/list 响应 | 是 | 待测 |
| 成功调用工具并获取结果 | 是 | 待测 |
| 进程异常退出时不影响主进程 | 是 | 待测 |

### POC-05 结论

- [ ] ✅ 通过
- [ ] ❌ 不通过 — 排查 MCP 协议实现问题

---

## POC-06：Tool Search 按需披露机制

### POC-06 验证目标

验证在 100+ 工具场景下，BM25 检索层可以正确找到目标工具，并通过三桥接工具完成"搜索 → 描述 → 调用"完整链路，额外模型往返次数 ≤ 2 次。

### POC-06 验证方案

```rust
// 伪代码：BM25 工具检索原型
use bm25::Index;

struct ToolRegistry {
    tools: Vec<ToolDef>,
    index: bm25::Index,
}

impl ToolRegistry {
    // 桥接工具 1：搜索
    fn tool_search(&self, query: &str, limit: usize) -> Vec<ToolSummary> {
        let results = self.index.search(query, limit);
        // 零分时降级为子串匹配
        if results.is_empty() {
            return self.tools.iter()
                .filter(|t| t.name.contains(query))
                .map(|t| t.summary())
                .collect();
        }
        results
    }

    // 桥接工具 2：按需加载完整 Schema
    fn tool_describe(&self, name: &str) -> Option<ToolSchema> {
        self.tools.iter()
            .find(|t| t.name == name)
            .map(|t| t.full_schema())
    }

    // 桥接工具 3：调用（解包到真实工具）
    async fn tool_call(&self, name: &str, args: Value) -> Result<Value> {
        let tool = self.tools.iter().find(|t| t.name == name)?;
        tool.execute(args).await
    }

    // Token 预算评估：决定使用哪个分级
    fn disclosure_level(&self, context_budget: usize) -> DisclosureLevel {
        let schema_tokens: usize = self.tools.iter().map(|t| t.schema_tokens()).sum();
        let listing_tokens: usize = self.tools.iter().map(|t| t.summary_tokens()).sum();

        if schema_tokens <= context_budget {
            DisclosureLevel::Eager
        } else if listing_tokens <= context_budget {
            DisclosureLevel::LazyWithListing
        } else {
            DisclosureLevel::Barebridge
        }
    }
}
```

### POC-06 验证场景

场景一：模拟 5 个 MCP Server，每个 20 个工具，共 100 个工具。

场景二：模拟 Cloudflare 级别，3000+ 工具，名称清单约 32K tokens。

### POC-06 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| BM25 检索准确率（Top-5 包含目标工具） | > 90% | 待测 |
| 100 工具场景检索延迟 | < 10ms | 待测 |
| 三桥接工具完整链路（搜索→描述→调用） | 正常 | 待测 |
| 额外模型往返次数 | ≤ 2 次 | 待测 |
| 裸桥接模式（3000+ 工具）Token 节省率 | > 80% | 待测 |
| 目录无状态：工具注册变更后目录正确重建 | 是 | 待测 |

### POC-06 不可消除的权衡（记录存档）

| 权衡项 | 说明 |
| ---- | ---- |
| 冷工具首次调用延迟 | 需额外 1-2 次模型往返（搜索+描述），热工具可跳过搜索直接描述 |
| Prompt Cache 失效 | 延迟加载的 Schema 无法享受系统提示前缀缓存优化 |
| 依赖模型检索质量 | 小模型的检索 query 质量较差，可能找不到正确工具 |
| 工具集变更破坏缓存 | 新增/删除工具后 Prompt Cache 全量失效 |

### POC-06 结论

- [ ] ✅ 通过 — Tool Search 机制确认纳入 MCP 和 Skills 实现
- [ ] ❌ 不通过 — 重新评估 BM25 算法或调整分级阈值策略

---

## POC-07：delegate_task 子 Agent 隔离验证

### POC-07 验证目标

验证通过 `delegate_task` 派生的子 AgentCore 实例与父 Agent 完全隔离，`max_depth` / `max_concurrent` / `max_iterations` 限制按预期工作，且 5 类强制屏蔽工具在子 Agent 中确实不可调用。

### POC-07 验证方案

```rust
// 伪代码：子 Agent 派生隔离测试
use agent_core::{Supervisor, SpawnConfig, AgentTask};

async fn poc_delegate_task() {
    let mut supervisor = Supervisor::new(SpawnConfig {
        max_depth: 1,
        max_concurrent: 3,
        max_iterations: 50,
    });

    // 场景1：正常派生
    let handle = supervisor.spawn(
        "researcher",
        AgentTask { goal: "调研 Rust 异步生态".into(), context: "...".into() },
        SpawnConfig::default(),
    ).await.unwrap();

    // 场景2：超出 max_concurrent 应返回错误
    let results = futures::join_all((0..5).map(|_| supervisor.spawn(...))).await;
    assert!(results[3].is_err()); // 第4个开始应报错

    // 场景3：子 Agent 尝试调用 delegate_task 应被屏蔽
    // 子 Agent 工具列表中不应包含 delegate_task / memory_write / git_push

    // 场景4：超出 max_iterations 应自动停止
    let result = supervisor.spawn("looper", endless_task, config).await;
    assert!(result.unwrap().iterations <= 50);
}
```

### POC-07 验收标准

| 指标 | 目标 | 实测结果 |
| ---- | ---- | ---- |
| 子 Agent 无法访问父 Agent 对话历史 | 是 | 待测 |
| 超出 `max_concurrent`（默认 3）时返回工具错误 | 是 | 待测 |
| 超出 `max_depth`（默认 1）时返回工具错误 | 是 | 待测 |
| 超出 `max_iterations`（50次）时自动停止 | 是 | 待测 |
| 子 Agent 工具列表中无 `delegate_task` / `memory_write` / `git_push` | 是 | 待测 |
| 并发 3 个子 Agent 时内存增量 | < 200MB | 待测 |
| 子 Agent 完成后返回结构化摘要（actions/findings/modified_files/issues） | 是 | 待测 |

### POC-07 结论

- [ ] ✅ 通过 — delegate_task 隔离机制确认可用
- [ ] ❌ 不通过 — 排查 Supervisor 并发控制或工具屏蔽逻辑

---

## POC-08：SQLCipher 与 sqlite-vec 扩展兼容性验证

### POC-08 验证目标

确认 SQLCipher 加密数据库与 sqlite-vec 向量检索扩展可以共存运行。

### POC-08 背景

- `07-隐私与合规.md` 要求使用 SQLCipher 加密本地数据库（Argon2 密钥派生 + AES-256）
- `08-知识库RAG.md` 使用 sqlite-vec 扩展实现向量相似度搜索
- 两者共存的技术可行性未经验证：SQLCipher 替换了 SQLite 的底层 IO 层，可能与 sqlite-vec 的虚拟表机制产生冲突

### POC-08 验证方案

```rust
// 1. 编译 sqlcipher + sqlite-vec 混合链接
// 需确认 libsqlite3-sys 的 sqlcipher feature 与 sqlite-vec 的加载方式兼容

// 2. 创建加密数据库
let pool = SqlitePool::connect("sqlite:test_encrypted.db").await?;
sqlx::query("PRAGMA key = 'test_passphrase'").execute(&pool).await?;

// 3. 加载 sqlite-vec 扩展
sqlx::query("SELECT load_extension('vec0')").execute(&pool).await?;

// 4. 创建向量虚拟表
sqlx::query("CREATE VIRTUAL TABLE test_vec USING vec0(embedding float[384])").execute(&pool).await?;

// 5. 写入测试向量
sqlx::query("INSERT INTO test_vec(rowid, embedding) VALUES (1, ?)")
    .bind(&test_vector)
    .execute(&pool).await?;

// 6. KNN 查询
let results = sqlx::query("SELECT rowid, distance FROM test_vec WHERE embedding MATCH ? ORDER BY distance LIMIT 5")
    .bind(&query_vector)
    .fetch_all(&pool).await?;
```

### POC-08 验收标准

| 指标 | 目标值 | 实测结果 |
| ---- | ---- | ---- |
| 扩展加载 | load_extension 成功，无 crash | 待测 |
| 虚拟表创建 | CREATE VIRTUAL TABLE 成功 | 待测 |
| 向量写入 | 1000 条 384 维写入 < 5s | 待测 |
| KNN 查询 | Top-10 查询 < 50ms | 待测 |
| 数据加密验证 | 无密钥打开 .db 文件报错（非明文） | 待测 |
| FTS5 共存 | FTS5 虚拟表在加密库中正常工作 | 待测 |

### POC-08 不通过时的备选方案

1. **分库方案**：主业务数据使用 SQLCipher 加密，知识库向量数据使用独立的未加密 SQLite（`knowledge.db`），文件系统权限保护
2. **应用层加密**：不使用 SQLCipher，改为对敏感字段（API Key、对话内容）在应用层加密后存入普通 SQLite
3. **切换向量存储**：知识库向量改用 qdrant 本地模式（独立进程），与 SQLCipher 主库解耦

### POC-08 关联文档

- `03-系统设计阶段/07-隐私与合规.md`（SQLCipher 加密要求）
- `03-系统设计阶段/08-知识库RAG.md`（sqlite-vec 向量检索）
- `03-系统设计阶段/数据库设计/01-Schema设计.md`（embeddings 虚拟表定义）

### POC-08 结论

- [ ] ✅ 通过 — SQLCipher 与 sqlite-vec 兼容，加密向量库方案确认
- [ ] ❌ 不通过 — 采用备选方案（分库 / 应用层加密 / qdrant）

---

## POC 总结

| POC | 状态 | 结论 |
| ---- | ---- | ---- |
| POC-01 sqlite-vec 性能 | 🔲 待执行 | — |
| POC-02 多 Provider 抽象 | 🔲 待执行 | — |
| POC-03 Tauri 跨平台打包 | 🔲 待执行 | — |
| POC-04 Rhai 脚本执行 | 🔲 待执行 | — |
| POC-05 MCP 客户端 | 🔲 待执行 | — |
| POC-06 Tool Search BM25 | 🔲 待执行 | — |
| POC-07 子 Agent 隔离 | 🔲 待执行 | — |
| POC-08 SQLCipher + sqlite-vec 兼容性 | 🔲 待执行 | — |

**全部 POC 通过后，可正式进入详细设计阶段。**
