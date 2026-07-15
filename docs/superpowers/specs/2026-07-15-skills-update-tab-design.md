# Skills 更新 Tab 设计

**日期:** 2026-07-15  
**状态:** 待实现  
**范围:** Skills 面板「更新」Tab、安装来源清单（origin）、从原源一键/批量更新  
**相关:** `skills` crate 安装/扫描、`SkillsPanel` 已安装 / 本机 / 商店 Tab

## 目标

技能变多后，用户需要集中查看与更新已装技能。顶栏增加「更新」Tab，按**原安装源**（SkillHub / ClawHub / skills.sh / GitHub 等）一键或批量更新；无来源的旧技能明确标为不可溯源。

## 用户决策摘要

| 决策项 | 选择 |
|--------|------|
| 入口形态 | 独立「更新」Tab（技能多时比卡片内嵌更清晰） |
| 顶栏结构 | `已安装 \| 本机 \| 更新 \| 商店`（角标规则见下行） |
| 默认列表 | 筛选三态：可更新 / 有来源 / 无来源；**v1 默认「有来源」**，**v2+ 默认「可更新」** |
| 角标 | v1 不刷数量（避免全是「有来源」误导）；v2+ 为 outdated 数量 |
| 已安装/本机 | 有来源时可保留小型「更新」入口，与更新 Tab 同源逻辑 |
| 更新语义（v1） | 用记录的 `install_ref` **强制重装覆盖** |
| 「有更新」检测 | v2：对比远端 version / `updated_at` |
| 本地改动 | v1 直接覆盖（文案标明将覆盖）；v3 再加确认/备份 |
| 无来源 | 不可一键更新；提示去商店重装或补录来源（补录非本期必做） |

## 现状缺口

`InstalledSkill` 仅由目录扫描 + `SKILL.md` 得到，**不持久化** `store` / `install_ref` / version。  
因此「从原来下载源更新」必须以 **origin 清单** 为前提；商店名与 frontmatter `name` / 目录名不一致时，沿用既有匹配策略（目录名 + slug，见 `skillInstalledMatch`）。

## 数据模型：Origin 清单

存储建议：`~/.astro/skill-origins.json`（或按 Agent 分文件，如 `skill-origins.{agentId}.json`）。  
键建议以 **安装目录 folder**（稳定、可扫）为主，必要时附 `skill_id`（扫描得到的绝对 id）。

```ts
type SkillOriginRecord = {
  /** 安装目录名，如 ppt-generator-skill */
  folder: string;
  /** 可选：扫描 id，绝对路径 …/skills/{folder} */
  skill_id?: string;
  /** 显示名（安装时商店名或 frontmatter name） */
  name: string;
  store: string; // skillhub | clawhub | skillsdotsh | …
  install_ref: string;
  agent_id?: string; // 装到哪个 Agent workspace；全局根可省略或用约定值
  scope?: "astro" | "machine";
  installed_at: number; // unix ms
  last_updated_at?: number;
  /** v2：安装/上次更新时记下的远端元数据 */
  remote_version?: string | null;
  remote_updated_at?: number | null;
};

type SkillOriginsFile = {
  version: 1;
  records: SkillOriginRecord[];
};
```

### 写入时机

- `install_store_skill` / `install_from_ref` **成功后**：upsert 对应 folder + install_ref + store + timestamps。
- **更新成功后**：刷新 `last_updated_at` 及远端元数据（若有）。
- **卸载**（若有卸载命令）：删除对应 record；无卸载命令则清单可留脏数据，列表以扫盘为准、origin 仅作注解（孤儿 record 在「无扫描对应」时忽略）。

### 与扫描列表合并

前端/后端列出「更新」视图时：

1. 扫已安装（astro）+ 本机已链接（可选：本机全部有 origin 的项）。
2. 用 folder / name / id 与 origin 匹配（同 `skillInstalledMatch` 思路）。
3. 状态：
   - `no_origin`：扫到了但无清单记录
   - `updatable`（v1）：有 origin → 可作为强制更新目标；角标策略见「v1 角标」
   - `outdated`（v2）：有 origin 且远端更新于本地
   - `current`（v2）：有 origin 且已最新

## 「更新」Tab UI

### 布局

- 与已安装类似的画廊/列表视图复用，减少新组件。
- 顶区：说明文案 + 「检查更新」（v2）+ 「全部更新」。
- 筛选芯片：`可更新`（默认）| `有来源` | `无来源`。
- 角标：可更新条数；为 0 时可不显示数字。

### v1 角标与默认筛选（重要）

v1 **尚无**可靠「远端是否更新」时：

- **推荐**：角标暂不刷「全部有来源数量」（避免永远很多）；默认筛选用 **有来源**，主按钮文案为「更新 / 重新安装」。
- 或：默认「有来源」，角标隐藏，直到 v2 有真正 outdated 再显示角标。
- **选定策略（本设计）**：v1 默认筛选 = **有来源**；角标留空或「·」指示 Tab 存在；v2 起默认筛选与角标改为 **可更新（outdated）**。

### 卡片操作

- 有来源：`更新`（进行中 spinner，复用 `is-spin` 动画）。
- 无来源：禁用 + tooltip「无法追溯安装源」。
- 可选：展示 store 徽章、`install_ref` 缩略、上次更新时间。

### 已安装 / 本机

- 有 origin：次要按钮或菜单「更新」。
- 更新命令与更新 Tab **共用**同一后端 API。

## 后端行为

### 新命令（名称可微调）

| 命令 | 作用 |
|------|------|
| `list_skill_origins` | 读清单（可按 agentId） |
| `update_installed_skill` | 入参：folder 或 skill_id；查 origin → `install_from_ref` 覆盖 → 更新清单 |
| `update_all_skills` | 批量：仅有来源（v1）或 outdated（v2）；串行，返回逐条结果 |
| （可选）`check_skill_updates` | v2：查远端 version/`updated_at` 写回/返回 diff |

覆盖安装应对准 **原 skills 根目录**（与首次安装 `agent_skills_dir` 一致）。  
本机 scope：仅当有 origin 且目标为可写目录时更新物理目录；无 origin 标无来源。

### 错误与 UX

- 网络/CLI 失败：Toast error，该条保持可重试。
- 批量：部分失败汇总（成功 N / 失败 M）。
- 安装成功但无法解析 folder 名：打日志，尽量从 `install_ref` slug / 新扫盘推断后写清单。

## 分阶段交付

| 阶段 | 交付 | 验收 |
|------|------|------|
| **v1** | origin 写入；更新 Tab；筛选；单条/全部强制重装；已安装/本机次要入口 | 新装技能有 origin；可从 Tab 再更新；旧技能显示无来源 |
| **v2** | 远端对比；真正 outdated 角标与默认筛选 | 有新版本时角标 > 0；已最新不出现在默认列表 |
| **v3** | 本地改动检测/确认或备份；失败重试细化 | 有未提交改动时更新前有明确提示 |

## 非目标（本期）

- 手动「补录 install_ref」UI（可后续加）
- Git 式三路合并技能文件
- 自动后台静默更新
- 跨 Agent 一次更新全部 workspace（v1 跟当前 Agent 选择器走）

## 测试要点

- origin upsert：同 folder 重复安装更新 `last_updated_at`，不产生双记录
- `update_installed_skill`：无 origin → 明确错误；有 origin → 调用安装路径与首装一致
- 匹配：商店名 ≠ frontmatter name 时，folder/slug 仍能挂上 origin（回归 `skillInstalledMatch`）
- 批量：一条失败不影响后续；结果汇总正确

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| 旧技能无 origin 比例高 | 无来源态清晰；后续可补「从商店认领」 |
| 强制覆盖丢本地修改 | v1 文案标明「将覆盖本地文件」；v3 再强化 |
| 多源同 slug 撞名 | skill_key 用 folder + agent + source_dir；清单带 store |
| CLI 各 store 行为不一致 | 复用现有 `install_from_ref`，更新不另写下载器 |
