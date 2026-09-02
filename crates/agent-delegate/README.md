# worktree

`agent-delegate` 只负责显式桌面多任务的 Git worktree 隔离。Subagent 继承父任务的
checkout，不会隐式调用本 crate。

## 核心契约

- `WorktreeManager::create` 从 `HEAD` 或显式 base 解析 commit，默认创建 detached checkout。
- `ManagedWorktree.cwd` 保留源 checkout 内的嵌套工作目录；base 不包含该目录时创建失败。
- Git 子进程清除继承的 `GIT_DIR` / `GIT_WORK_TREE` / index 等 repository selector，
  并在单次命令中禁用 hooks、filesystem monitor 和已配置 clean/smudge/process filters。
- 创建或物化失败时删除不完整 worktree 与空 allocation bucket。
- `cleanup()` 只自动移除干净 worktree；脏树保留供人工恢复。
- 兼容入口 `create_task_worktree` 会复制 `.worktreeinclude` 中的普通文件/目录，
  但跳过绝对路径、`..` 与符号链接。

## 验证

```bash
cargo test -p worktree
cargo clippy -p worktree --all-targets --no-deps -- -D warnings
```

测试覆盖 detached HEAD、显式 base、嵌套 cwd、Git 环境隔离、安全 include 复制和脏树保留。
