# generated 按类型分子目录

日期：2026-07-15  
状态：已实现  
相关：[Google 媒体生成工具](./2026-07-15-google-media-gen-tools-design.md)、[聊天媒体预览](./2026-07-15-chat-media-preview-design.md)

## 背景

`image_gen` / `video_gen` / `tts` 与部分 Tauri 命令将产物扁平写入 `workspace/generated/`（如 `generated/img-….png`）。文件一多难以浏览；Agent 在 Markdown 里也常写相对路径。需要在生成侧按类型落盘子目录，并在工作区初始化时建好常用分类空壳。

## 目标

1. 工作区确保存在 `generated/` 及一组**初始化即创建**的子目录。
2. 媒体生成工具（及等价 Tauri 出图）写入对应子目录，返回路径带分类前缀。
3. 办公类（pdf / word / pptx / excel）统一进扁平的 `docs/`，不再二级拆分。
4. **不迁移**已有落在 `generated/` 根下的旧文件（避免扯断历史消息里的路径）。

## 非目标

- 自动搬迁历史扁平文件
- `docs/{pdf,word,…}` 二级目录
- 强制拦截所有 `write_file` / 终端写入（Agent 仍可自选路径；仅生成工具与文档约定引导）
- 改 File Space 分类 UI（已有按扩展名筛选即可）

## 已确认决策

| 项 | 选择 |
|----|------|
| 策略 | C：常用目录初始化建好；冷门类型按写入再建（本轮冷门仅 `other` 等同初始化建好） |
| 办公文档 | C1：扁平 `generated/docs/` |
| 旧文件 | 不自动迁移 |

## 目录约定

相对工作区根：

```
generated/
  images/     # 图：png jpg webp gif …
  videos/     # 视频：mp4 webm mov …
  audio/      # 音：wav mp3 m4a …
  code/       # 单文件脚本 / 代码片段
  project/    # 多文件小工程、脚手架目录
  docs/       # pdf / doc(x) / ppt(x) / xls(x) 等办公文档（扁平）
  html/       # html / htm 预览页
  other/      # 对不上上述类型的产物
```

初始化时创建上述全部（含空的 `docs` / `other`），打开 `generated` 即可看到结构。

可选后续（非本轮）：按扩展名第一次写入时 `create_dir_all` 已足够；初始化列表可再增删，不改路由表即可。

## 方案对比（已选 ★）

| 方案 | 做法 | 优点 | 缺点 |
|------|------|------|------|
| **★ 1. 脚手架 + 分类写入助手** | `ensure_agent_space` 建好子目录；`generated_subdir(kind)` 供各 gen 工具与 Tauri 共用 | 单一真相、与现有 `AGENT_SUBDIRS` 模式一致 | 需改若干写入点 |
| 2. 各工具各自硬编码子路径 | 每处 `join("generated/images")` | 改动面直观 | 易漂移、难测一致 |
| 3. 仅文档约定、不改工具 | 改 AGENT/TOOLS 文案让模型写进子目录 | 零代码 | 不可靠；工具结果仍扁平 |

采用 **方案 1**。

## 架构

```
memory (ensure_agent_space)
  └─ AGENT_SUBDIRS 增补 generated 与子目录（或 GENERATED_SUBDIRS）

tools / tauri
  └─ generated_dir(workspace, GeneratedKind) → PathBuf
        ├─ image_gen  → images/
        ├─ video_gen  → videos/
        ├─ tts        → audio/
        └─ tauri 出图 → images/
```

### 类型枚举（建议）

```text
GeneratedKind = Images | Videos | Audio | Code | Project | Docs | Html | Other
```

扩展名 → Kind 映射（供将来通用落盘 / 文档；本轮 gen 工具可直接指定 Kind）：

| Kind | 扩展名示例 |
|------|------------|
| Images | png jpg jpeg webp gif bmp svg avif |
| Videos | mp4 webm mov mkv m4v |
| Audio | wav mp3 m4a aac ogg flac opus |
| Html | html htm |
| Docs | pdf doc docx ppt pptx xls xlsx |
| Code | rs ts tsx js py go …（可选，本轮可不强制） |
| Other | 其余 |

本轮**硬要求**：三个媒体工具 + Tauri 出图路径改用 Images / Videos / Audio。`Code` / `Project` / `Docs` / `Html` / `Other` 以目录脚手架为主；若无写入点可只 `create_dir_all`。

## 路径与兼容

| 场景 | 行为 |
|------|------|
| 新生成 | 绝对路径如 `…/workspace/generated/images/img-….png`；工具文案仍「图片已生成：…」 |
| Markdown 相对路径 | `generated/images/….png`；现有 `resolveMediaSrc(baseDir)` 已支持嵌套相对路径 |
| 旧扁平路径 | `generated/img-….png` 仍可读可预览；不搬迁 |
| `parseGeneratedMedia` | 仍认绝对路径；无需改正则即可吃到新路径 |

## 引导文案（轻量）

在 `TOOLS.md` 或 `AGENT.md` 模板中加一句：Agent 自建产物优先写入上述 `generated/<type>/`；单文件代码 → `code/`，多文件工程 → `project/`，办公文档 → `docs/`。不实现强制改写。

## 错误处理

- `create_dir_all` 失败：工具返回明确错误（与现有一致）
- 未知 Kind：落 `other/`（若提供通用 API）

## 测试

1. `ensure_agent_space` / 相关测试：断言 `generated/images` 等目录存在。
2. `image_gen` / `video_gen` / `tts`（或单元测路径助手）：`generated_dir` 落点正确。
3. 前端：`absolutizeMediaPath("generated/images/a.png", base)` 拼对（可扩一条现有用例）。

## 实现要点（供 plan）

1. `memory`：扩展子目录列表（注意 `AGENT_SUBDIRS` 当前为 `mermaid` / `skills`——`generated/...` 可用新常量一并 ensure）。
2. `tools`：抽出 `generated_dir`（crate 内公共即可）；改 `image_gen` / `video_gen` / `tts`。
3. `frontend/src-tauri/commands.rs` 出图写入对齐。
4. 更新媒体 gen 设计文档中的输出路径表述；模板 TOOLS/AGENT 一句引导。
5. **不**跑批量迁文件脚本。
