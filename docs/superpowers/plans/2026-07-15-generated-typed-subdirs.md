# generated 分类子目录 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 工作区初始化时创建 `generated/{images,videos,audio,code,project,docs,html,other}`，媒体生成落盘进对应子目录；旧扁平文件不迁移。

**Architecture:** 在 `memory-paths` 增加 `GeneratedKind` + `generated_dir` + `GENERATED_SUBDIRS` 单一真相；`ensure_agent_space` 脚手架创建目录；`image_gen` / `video_gen` / `tts` 与 Tauri `generate_image` 改用该助手；模板轻量引导 Agent 自写产物。

**Tech Stack:** Rust workspace（memory-paths / memory / tools / astro-agent Tauri）；既有 Node 测 `resolveMediaSrc`。

**Spec:** [`docs/superpowers/specs/2026-07-15-generated-typed-subdirs-design.md`](../specs/2026-07-15-generated-typed-subdirs-design.md)

## Global Constraints

- 办公文档扁平 `generated/docs/`，无二级 pdf/word 目录。
- **不**自动迁移 `generated/` 根下已有文件。
- 本轮硬改写入点：`image_gen`、`video_gen`、`tts`、Tauri `generate_image`。
- 不强制拦截 `write_file` / 终端。
- 子目录名固定英文小写：`images` | `videos` | `audio` | `code` | `project` | `docs` | `html` | `other`。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Create: `memory-paths/src/workspace/generated.rs` | `GeneratedKind`、`GENERATED_SUBDIRS`、`generated_dir` |
| Modify: `memory-paths/src/workspace/mod.rs` | 导出 generated 模块 |
| Modify: `crates/agent-memory/src/agent/workspace/lifecycle.rs` | ensure 时创建 GENERATED_SUBDIRS |
| Modify: `crates/agent-memory/src/lib.rs` | 再导出 `GeneratedKind` / `generated_dir` / `GENERATED_SUBDIRS` |
| Modify: `crates/agent-tools/src/builtin/image_gen.rs` | 写入 `images/` |
| Modify: `crates/agent-tools/src/builtin/video_gen.rs` | 写入 `videos/` |
| Modify: `crates/agent-tools/src/builtin/tts.rs` | 写入 `audio/` |
| Modify: `apps/desktop/src-tauri/src/commands.rs` | `generate_image` 对齐 |
| Modify: `memory/.../templates.rs` | TOOLS.md 一句引导 |
| Modify: 媒体 gen design 路径表述（可选同 commit） |
| Modify: `apps/desktop/src/lib/resolveMediaSrc.test.ts` | 嵌套相对路径用例 |

---

### Task 1: memory-paths — GeneratedKind + generated_dir

**Files:**
- Create: `memory-paths/src/workspace/generated.rs`
- Modify: `memory-paths/src/workspace/mod.rs`
- Test: 同文件 `#[cfg(test)]` 或 `memory-paths` 内已有测试风格

**Interfaces:**
- Produces:
  - `pub enum GeneratedKind { Images, Videos, Audio, Code, Project, Docs, Html, Other }`
  - `pub fn dir_name(&self) -> &'static str`
  - `pub const GENERATED_SUBDIRS: &[&str]` — 相对工作区：`"generated/images"` … `"generated/other"`
  - `pub fn generated_dir(workspace: &Path, kind: GeneratedKind) -> PathBuf`

- [x] **Step 1: Write the failing test**

在 `generated.rs` 底部：

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn generated_dir_joins_kind_subdir() {
        let ws = Path::new("/Users/a/.astro/workspace");
        assert_eq!(
            generated_dir(ws, GeneratedKind::Images),
            Path::new("/Users/a/.astro/workspace/generated/images")
        );
        assert_eq!(
            generated_dir(ws, GeneratedKind::Videos),
            Path::new("/Users/a/.astro/workspace/generated/videos")
        );
        assert_eq!(
            generated_dir(ws, GeneratedKind::Audio),
            Path::new("/Users/a/.astro/workspace/generated/audio")
        );
    }

    #[test]
    fn generated_subdirs_lists_all_seed_folders() {
        let expected = [
            "generated/images",
            "generated/videos",
            "generated/audio",
            "generated/code",
            "generated/project",
            "generated/docs",
            "generated/html",
            "generated/other",
        ];
        assert_eq!(GENERATED_SUBDIRS, &expected);
    }
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p memory-paths generated_dir_joins_kind_subdir -- --nocapture`  
Expected: FAIL（模块/符号不存在）

- [x] **Step 3: Write minimal implementation**

`memory-paths/src/workspace/generated.rs`:

```rust
//! Agent 工作区 `generated/` 分类落盘约定。

use std::path::{Path, PathBuf};

/// 产物类型 → `generated/<dir_name>/`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneratedKind {
    Images,
    Videos,
    Audio,
    Code,
    Project,
    Docs,
    Html,
    Other,
}

impl GeneratedKind {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Images => "images",
            Self::Videos => "videos",
            Self::Audio => "audio",
            Self::Code => "code",
            Self::Project => "project",
            Self::Docs => "docs",
            Self::Html => "html",
            Self::Other => "other",
        }
    }
}

/// 相对 Agent **工作区根** 的路径；`ensure_agent_space` 初始化创建。
pub const GENERATED_SUBDIRS: &[&str] = &[
    "generated/images",
    "generated/videos",
    "generated/audio",
    "generated/code",
    "generated/project",
    "generated/docs",
    "generated/html",
    "generated/other",
];

/// `{workspace}/generated/{kind}`（不 create；调用方 `create_dir_all`）。
pub fn generated_dir(workspace: &Path, kind: GeneratedKind) -> PathBuf {
    workspace.join("generated").join(kind.dir_name())
}
```

`mod.rs`：

```rust
pub mod agent_config;
pub mod generated;
pub mod paths;

pub use agent_config::*;
pub use generated::*;
pub use paths::*;
```

- [x] **Step 4: Run test to verify it passes**

Run: `cargo test -p memory-paths generated_ -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory-paths/src/workspace/generated.rs memory-paths/src/workspace/mod.rs
git commit -m "$(cat <<'EOF'
feat(memory-paths): add GeneratedKind and generated_dir helper

EOF
)"
```

---

### Task 2: ensure_agent_space 脚手架 + 再导出

**Files:**
- Modify: `crates/agent-memory/src/agent/workspace/lifecycle.rs`（`ensure_agent_space` 内循环）
- Modify: `crates/agent-memory/src/lib.rs`（`pub use workspace::{…, GeneratedKind, generated_dir, GENERATED_SUBDIRS}`）
- Test: 扩展 `lifecycle.rs` 现有 `ensure_workspace_creates_core_layout`

**Interfaces:**
- Consumes: `memory_paths::GENERATED_SUBDIRS`（lifecycle 已通过 `paths` / workspace 可达；若仅 `pub use paths::*`，需从 `super::paths` 或 `memory_paths::workspace` 引入 — 实际从 `crate` 侧：在 lifecycle 顶部 `use memory_paths::GENERATED_SUBDIRS;` 或因 paths re-export 后 `use super::paths::GENERATED_SUBDIRS` 若 generated 已 `pub use` 进 paths 模块 — **以 Task1 `workspace/mod.rs` 的 `pub use generated::*` 为准，lifecycle 用 `use super::paths::GENERATED_SUBDIRS` 可能不对**；应用：

```rust
use memory_paths::GENERATED_SUBDIRS;
// 或
use super::super::paths; // 若 memory 的 paths 只 re-export memory_paths paths 子模块
```

稳妥写法（lifecycle 内）：

```rust
use memory_paths::GENERATED_SUBDIRS;
```

（确认 `memory` crate 已依赖 `memory-paths`。）

- Produces: 新 Agent / ensure 后磁盘上存在八个 `generated/*` 目录；`memory::generated_dir` 可供 tools/tauri 调用

- [x] **Step 1: Write the failing assertion**

在 `ensure_workspace_creates_core_layout` 中 `assert!(ws.join("skills").is_dir());` 之后加入：

```rust
for rel in memory_paths::GENERATED_SUBDIRS {
    assert!(
        ws.join(rel).is_dir(),
        "missing workspace/{rel}"
    );
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p memory ensure_workspace_creates_core_layout -- --nocapture`  
Expected: FAIL `missing workspace/generated/images`（或同类）

- [x] **Step 3: Implement ensure loop**

在 `ensure_agent_space` 中现有：

```rust
for sub in AGENT_SUBDIRS {
    fs::create_dir_all(workspace.join(sub))?;
}
```

之后追加：

```rust
for rel in memory_paths::GENERATED_SUBDIRS {
    fs::create_dir_all(workspace.join(rel))?;
}
```

`crates/agent-memory/src/lib.rs` 的 `pub use workspace::{...}` 增加：`generated_dir, GeneratedKind, GENERATED_SUBDIRS`（需在 `crates/agent-memory/src/agent/workspace/mod.rs` 已 `pub use` paths/generated — 因 `pub use paths::*` 且 paths 来自 memory-paths workspace 的 `pub use generated::*`，确认 `crates/agent-memory/src/agent/workspace/paths.rs` 的 `pub use memory_paths::workspace::paths::*` **不会**带上 generated。

**关键：** `crates/agent-memory/src/agent/workspace/paths.rs` 当前只 re-export `memory_paths::workspace::paths::*`。Task2 需改为：

```rust
pub use memory_paths::workspace::paths::*;
pub use memory_paths::workspace::{generated_dir, GeneratedKind, GENERATED_SUBDIRS};
```

或 `pub use memory_paths::workspace::*`（若无名字冲突）。优先显式三项 + paths。

- [x] **Step 4: Run tests**

Run: `cargo test -p memory ensure_workspace_creates_core_layout -- --nocapture`  
Expected: PASS

另跑：`cargo test -p memory-paths generated_ -- --nocapture` 仍绿。

- [x] **Step 5: Commit**

```bash
git add memory/src/agent/workspace/lifecycle.rs memory/src/agent/workspace/paths.rs memory/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(memory): seed generated typed subdirs on ensure_agent_space

EOF
)"
```

---

### Task 3: 媒体工具改写入路径

**Files:**
- Modify: `crates/agent-tools/src/builtin/image_gen.rs`（约 L107：`join("generated")` → `memory::generated_dir(..., Images)`）
- Modify: `crates/agent-tools/src/builtin/video_gen.rs`（同类）
- Modify: `crates/agent-tools/src/builtin/tts.rs`（Google / OpenAI 两处 `join("generated")`）
- Modify: 各文件顶部模块注释路径文案

**Interfaces:**
- Consumes: `memory::generated_dir`, `memory::GeneratedKind`
- Produces: 落盘路径含 `/generated/images/` 等；工具返回字符串仍 `图片已生成：{abs}`

- [x] **Step 1: Write a focused path unit test（tools）**

若 tools 尚无对 generate_one 的单测，在 `crates/agent-tools/src/engine/` 或新建 `tools/tests/generated_dir_wiring_test.rs` **不必**真调 API。最小方案：在 `memory-paths` 已测 helper 后，本任务用编译期接线 + 可选：

在 `crates/agent-tools/src/builtin/image_gen.rs` 同文件不测私有函数时，增加裸测：

```rust
#[cfg(test)]
mod path_tests {
    use memory::{generated_dir, GeneratedKind};
    use std::path::Path;

    #[test]
    fn image_gen_target_dir_is_images() {
        let d = generated_dir(Path::new("/ws"), GeneratedKind::Images);
        assert!(d.ends_with("generated/images"));
    }
}
```

（同类可放 video/tts，或只放一处避免重复。）

- [x] **Step 2: Run — path_tests 应已 PASS（helper 已存在）；再用 grep 确认旧路径仍在，作为改前基线**

Run: `rg 'join\("generated"\)' tools/src/builtin/image_gen.rs tools/src/builtin/video_gen.rs tools/src/builtin/tts.rs`  
Expected: 仍有命中

- [x] **Step 3: Replace writes**

`image_gen.rs`：

```rust
use memory::{generated_dir, GeneratedKind};
// ...
let dir = generated_dir(&ctx.workspace_dir, GeneratedKind::Images);
std::fs::create_dir_all(&dir)?;
```

`video_gen.rs` → `GeneratedKind::Videos`  
`tts.rs` 两处 → `GeneratedKind::Audio`

更新模块注释：`generated/images/` 等。

- [x] **Step 4: Verify**

Run: `rg 'join\("generated"\)' tools/src/builtin/{image_gen,video_gen,tts}.rs`  
Expected: 无命中（或仅注释）

Run: `cargo test -p tools path_tests -- --nocapture`（若加入）及 `cargo check -p tools`  
Expected: OK

- [x] **Step 5: Commit**

```bash
git add tools/src/builtin/image_gen.rs tools/src/builtin/video_gen.rs tools/src/builtin/tts.rs
git commit -m "$(cat <<'EOF'
feat(tools): write media gens into generated/{images,videos,audio}

EOF
)"
```

---

### Task 4: Tauri generate_image 对齐

**Files:**
- Modify: `apps/desktop/src-tauri/src/commands.rs`（`generate_image` 内 `join("generated")` ≈ L1022）

**Interfaces:**
- Consumes: `memory::generated_dir`, `memory::GeneratedKind::Images`

- [x] **Step 1: Locate and replace**

```rust
let dir = memory::generated_dir(
    &memory::default_agent_workspace_dir(),
    memory::GeneratedKind::Images,
);
std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
```

更新 doc comment：`workspace/generated/images/`。

- [x] **Step 2: Check compile**

Run: `cargo check -p astro-agent`  
Expected: OK（包名以 `apps/desktop/src-tauri/Cargo.toml` 的 `name = "astro-agent"` 为准）

- [x] **Step 3: Commit**

```bash
git add apps/desktop/src-tauri/src/commands.rs
git commit -m "$(cat <<'EOF'
feat(tauri): save generate_image under generated/images

EOF
)"
```

---

### Task 5: 模板引导 + 前端相对路径用例 + spec 路径备注

**Files:**
- Modify: `crates/agent-memory/src/agent/workspace/templates.rs` — `TEMPLATE_TOOLS` 增加「生成物目录」小节
- Modify: `apps/desktop/src/lib/resolveMediaSrc.test.ts` — 一条 `generated/images/...`
- Modify: `docs/superpowers/specs/2026-07-15-generated-typed-subdirs-design.md` — 状态改为「已规划/实现中」或实现后改为「已实现」
- Modify: `docs/superpowers/specs/2026-07-15-google-media-gen-tools-design.md` — 输出路径改为 `generated/images/` 等（短注）

- [x] **Step 1: TOOLS.md 模板追加（新 Agent 才有；不回写已有工作区）**

在 `TEMPLATE_TOOLS` 的 `## 写什么` 后或「示例」前加入：

```markdown
## 生成物目录

优先写入工作区 `generated/` 分类目录（勿堆在根下）：

- 图 → `generated/images/`
- 视频 → `generated/videos/`
- 音频 → `generated/audio/`
- 单文件代码 → `generated/code/`
- 多文件小工程 → `generated/project/`
- 办公文档（pdf/word/pptx/excel）→ `generated/docs/`
- HTML → `generated/html/`
- 其它 → `generated/other/`
```

- [x] **Step 2: Frontend test**

在 `absolutizeMediaPath joins relative under workspace baseDir` 测试中追加：

```ts
assert.equal(
  absolutizeMediaPath(
    "generated/images/img-1.png",
    "/Users/a/.astro/workspace",
  ),
  "/Users/a/.astro/workspace/generated/images/img-1.png",
);
```

Run: `cd frontend && node --experimental-strip-types --test src/lib/resolveMediaSrc.test.ts`  
Expected: PASS

- [x] **Step 3: Spec 状态**

将 typed-subdirs design 状态改为 `已实现`（本计划全部 task 完成后）；google-media-gen 中输出行改为：

- `generated/images/img-*.{png|jpg|webp}`
- `generated/videos/vid-*.mp4`
- （音频）`generated/audio/tts-*.wav`

- [x] **Step 4: Commit**

```bash
git add memory/src/agent/workspace/templates.rs \
  apps/desktop/src/lib/resolveMediaSrc.test.ts \
  docs/superpowers/specs/2026-07-15-generated-typed-subdirs-design.md \
  docs/superpowers/specs/2026-07-15-google-media-gen-tools-design.md
git commit -m "$(cat <<'EOF'
docs: guide generated/ typed layout in TOOLS template and specs

EOF
)"
```

---

### Task 6: 存量工作区冒烟（手动）

**不写代码。** 因 `ensure_agent_space` / `ensure_workspace` 会在启动或 `get_config` 时执行，**已有** Agent 工作区应在下次 bootstrap 时补建空子目录（`create_dir_all` 幂等）。

- [x] **Step 1:** 启动应用或调用会 `bootstrap_workspace` 的命令后，确认 `~/.astro/workspace/generated/images` 等存在。
- [x] **Step 2:** 跑一次 `image_gen`（或 UI 出图），确认新文件在 `generated/images/`，且聊天工具卡能预览；Markdown `generated/images/...` 可预览。
- [x] **Step 3:** 确认旧的 `generated/img-*.png`（若有）仍在根下且仍可预览。

---

## Spec coverage (self-review)

| Spec 项 | Task |
|---------|------|
| 初始化子目录列表 | 1–2 |
| 媒体工具写入分类目录 | 3 |
| Tauri 出图对齐 | 4 |
| docs 扁平、不迁旧文件 | Global + 无迁移 task |
| TOOLS 引导 | 5 |
| Markdown 相对嵌套路径 | 5（已有 resolve；补测） |
| 不强制 write_file | Global |

无 placeholder；类型名全表一致：`GeneratedKind::*` / `generated_dir` / `GENERATED_SUBDIRS`。
