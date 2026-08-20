# skills

Astro Skills 领域层：本机扫描、商店搜索与安装、运行时注册表、更新检查、备份/快照、使用统计。从磁盘 `SKILL.md` 文件到内存 `SkillRegistry` 的完整生命周期管理。

## 核心职责

- **本机扫描** -- 扫描 `~/.astro/skills/` 和 Agent 级目录下的 `SKILL.md`，解析 frontmatter 元数据（name / description / astro_tools），支持启用/禁用状态管理
- **商店集成** -- 通过 SkillHub API 搜索和获取详情（`search()` / `fetch_detail()`），爬取 skills.sh 商店页面
- **安装** -- `install_from_ref()` 支持 SkillHub HTTP 安装和 `npx skills add` 本地安装，`InstallOriginHint` 记录来源
- **运行时注册表** -- `SkillRegistry` 内存字典，按名称索引已加载的 `LoadedSkill`，支持 register / list / get
- **更新检查** -- `check_updates_for_agent()` 对比本地版本与商店版本，`update_installed_skill()` 执行增量更新
- **备份与快照** -- 更新前自动备份旧版、`save_snapshot()` / `restore_skill_snapshot()` 管理快照历史
- **使用统计** -- `record_skill_load()` 记录加载时间，`curate_report()` 生成使用报告
- **种子技能** -- `seed_default_public_skills()` 预装默认公共 Skill 集合，`BUNDLED_SKILLS` 内嵌 Skill
- **Soft-alias** -- 模型将 skill 名当工具调用时，自动改写为 `skills(action=load, skill_id=...)`

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | workspace 目录覆盖、公共 re-exports |
| `models.rs` | DTO 定义：`InstalledSkill` / `StoreSkill` / `StoreSkillDetail` / `SkillBundle` / `SkillFileEntry` / `SkillStoreFilter` / 更新检查结果类型 |
| `installed.rs` | 本机扫描核心（39KB）：`list_installed()` / `load_skill_by_name()` / `list_enabled_for_prompt()` / `parse_skill_frontmatter_full()` / 启用状态管理 / 文件操作 |
| `store.rs` | SkillHub 商店集成（28KB）：`search()` / `fetch_detail()` / API 请求与响应解析 |
| `install.rs` | 安装逻辑（18KB）：`install_from_ref()` / `InstallOriginHint` / SkillHub HTTP + npx 两条路径 |
| `origins.rs` | 来源追踪（17KB）：安装来源记录、来源匹配与验证 |
| `seed.rs` | 种子技能（16KB）：`seed_default_public_skills()` / `seed_bundled_into()` / `BUNDLED_SKILLS` / `DEFAULT_PUBLIC_SKILLS` |
| `check.rs` | 更新检查（12KB）：`check_updates_for_agent()` / `classify_update_status()` / `filter_outdated_folders()` |
| `update.rs` | 更新执行（11KB）：`update_installed_skill()` / `update_outdated_skills()` / `backup_skill_dir()` |
| `backups.rs` | 备份管理：`list_skill_backups()` / `reveal_skill_backup()` / `SkillBackupEntry` |
| `snapshots.rs` | 快照管理：`save_snapshot()` / `list_snapshots()` / `restore_latest()` / `SkillSnapshot` |
| `digest.rs` | Skill 摘要生成：内容哈希与变更检测 |
| `preview.rs` | 更新预览：`preview_skill_update()` -- 预览更新内容差异 |
| `registry.rs` | `SkillRegistry` 内存注册表：`new()` / `register()` / `list()` / `get()` |
| `skill.rs` | `LoadedSkill` / `SkillMetadata` -- 已加载 Skill 的运行时表示 |
| `usage.rs` | 使用统计：`record_skill_load()` / `curate_report()` / `last_loaded_at()` |
| `agent_id.rs` | Agent ID 辅助工具 |

## 核心类型与 API

- `SkillRegistry` -- 内存字典，按名称索引 `LoadedSkill`，支持 register / list / get
- `LoadedSkill` -- 已加载 Skill：`SkillMetadata`（name / description / astro_tools）+ content（SKILL.md 正文）+ path
- `InstalledSkill` -- 磁盘上已安装的 Skill 信息（含启用状态、路径、frontmatter）
- `StoreSkill` / `StoreSkillDetail` -- SkillHub 商店中的 Skill 摘要与详情
- `install_from_ref(ref, hint)` -- 从引用安装 Skill
- `list_installed()` / `list_installed_for_agent()` -- 列出已安装 Skill
- `list_enabled_for_prompt()` -- 列出启用的 Skill（供 prompt 组装使用）
- `search(filter)` / `fetch_detail(id)` -- 商店搜索与详情查询
- `set_workspace_override(path)` -- 设置 workspace 目录覆盖（线程安全）

## Crate 关系

| 方向 | crate |
|------|-------|
| 依赖 | `home`（数据根路径、Agent 目录） |
| 被依赖 | `agent`（运行时加载和注册 Skill）、`tools`（`skills` 工具实现）、`server`（gRPC ListSkills / ExecuteSkill） |

## 测试

```bash
# 全部测试
cargo test -p skills

# 单个测试
cargo test -p skills frontmatter_parsing -- --nocapture
```
