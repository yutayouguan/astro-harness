# worktree

> 轻量级 Git worktree 隔离助手，为桌面多任务工作流提供独立代码检出与安全清理。

## 核心职责

1. **Git 仓库发现** -- `find_git_root` 通过 `git rev-parse --show-toplevel` 定位仓库根目录；`resolve_project_root` 按优先级解析项目根（显式路径 > `ASTRO_PROJECT_ROOT` 环境变量 > 当前目录的 git root），非 git 目录也可作为项目根返回。
2. **任务级 Worktree 创建** -- `create_task_worktree` 在 `{repo}/.worktrees/astro-{short}/` 创建独立检出，分支命名为 `astro/task-{short}`；路径冲突时自动追加 UUID 后缀。自动将 `.worktrees/` 加入 `.gitignore`。
3. **Worktree 资源复制** -- 检出后自动读取 `.worktreeinclude` 配置文件，将主仓库中列出的文件/目录递归复制到新 worktree（支持非 git 管理的构建产物或配置）。
4. **安全清理** -- `WorktreeHandle::cleanup` 默认仅在 worktree 干净（无未提交变更）时删除 worktree 和对应分支；脏树保留并记录日志以便人工恢复。`cleanup_task_worktree` 提供 `clean_only` 参数控制是否强制清理。

## 模块结构

| 文件 | 职责 |
|---|---|
| `src/lib.rs` | crate 入口；re-export `git_worktree` 模块的公共 API |
| `src/git_worktree.rs` | 全部实现 -- `WorktreeHandle` 结构体、`find_git_root` / `resolve_project_root` / `create_task_worktree` / `cleanup_task_worktree`；内含 `.gitignore` 管理、`.worktreeinclude` 复制、worktree 脏检测等辅助函数 |

## 核心类型与 API

### 结构体

- `WorktreeHandle` -- 任务 worktree 句柄，持有 `path`（检出路径）、`branch`（分支名）、`repo_root`（主仓库路径）；调用 `cleanup()` 消费 self 并执行清理

### 关键函数

- `find_git_root(start: &Path) -> Option<PathBuf>` -- 从给定路径查找 git 仓库根；非 git 目录返回 `None`
- `resolve_project_root(explicit: Option<&Path>) -> Option<PathBuf>` -- 按优先级解析项目根目录
- `create_task_worktree(repo: &Path, task_id: &str) -> Result<WorktreeHandle>` -- 创建任务级独立 worktree
- `cleanup_task_worktree(repo: &Path, path: &Path, branch: &str, clean_only: bool)` -- 清理指定 worktree（函数式入口）

### 内部辅助

- `add_worktree` -- 执行 `git worktree add -b`，分支已存在时回退到无 `-b` 模式
- `copy_worktreeinclude` -- 解析 `.worktreeinclude` 并复制文件到新 worktree
- `ensure_worktrees_gitignore` -- 确保 `.gitignore` 包含 `.worktrees/` 条目
- `is_worktree_dirty` -- 通过 `git status --porcelain` 检测 worktree 是否有未提交变更
- `short_id` / `uuid_short` -- task_id 截短与 UUID 后缀生成

### 设计决策

- **Codex 风格 subagent 不建 worktree** -- 如 `lib.rs` 文档所述，Codex 风格的子 Agent 线程有意不创建隐式 worktree，它们继承父工作区和权限策略。本模块仅服务于桌面端显式多任务工作流。
- **脏树保留** -- 默认 `clean_only = true`，避免意外丢失未提交的工作成果。

## 与其他 crate 的关系

| crate | 关系 |
|---|---|
| `agent` (`agent-core`) | `exec::subagents` 模块在需要隔离的场景下调用 `create_task_worktree` 创建独立检出 |
| `astro-agent` (`apps/desktop`) | 桌面应用并行任务功能通过本模块获得 git 级别的代码隔离 |
| 无 workspace 内依赖 | 本 crate 仅依赖 `anyhow` / `tracing` / `uuid`，不依赖其他 agent-* crate |

## 测试运行命令

```bash
# 全部测试（单元测试在 src/git_worktree.rs 内）
cargo test -p worktree

# 单个测试函数
cargo test -p worktree create_and_cleanup_clean_worktree
cargo test -p worktree dirty_worktree_kept -- --nocapture
```

> 注意：测试会在临时目录中执行 `git init` / `git worktree add` 等命令，需要系统已安装 git。
