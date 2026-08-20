# Skills 生态扩展设计

> v0.3 规划 | 状态：规划中 | 覆盖：Agent 自动写 Skill、外部目录、Skills Hub、版本管理

---

## 1. 概述

当前 Skills 系统支持手动编写 `SKILL.md` 文件并放置在 `.astro/skills/` 目录下。v0.3 将在此基础上扩展四项能力：

- Agent 基于任务模式自动生成 Skill 草稿
- 支持外部 Skill 目录，多项目共享
- Skills Hub 信任体系，安全引入社区 Skill
- Skill 版本管理与回滚

**设计原则**：Skill 是用户可审计的纯文本（Markdown），不引入编译或打包步骤。

---

## 2. Agent 自动写 Skill

### 2.1 触发条件

Agent 不会主动生成 Skill，需同时满足以下条件：

| 条件 | 说明 |
|------|------|
| 用户正向反馈 | 任务完成后用户点击 👍 或显式评价 "好用" |
| 相似任务 ≥ 3 次 | 近 30 天内执行过 ≥ 3 次 goal 相似度 > 0.85 的任务 |
| 任务含可复用步骤 | 步骤数 ≥ 2，且涉及工具调用（非纯对话） |

相似度判断基于 goal 文本的 embedding 余弦距离，复用记忆系统的 embedding 模型。

### 2.2 生成流程

```
任务完成 + 👍
    │
    ▼
检查相似任务历史（≥ 3 次？）
    │ 是
    ▼
LLM 提取可复用模式
    │
    ▼
生成 SKILL.md 草稿
    │
    ▼
弹出预览面板 → 用户编辑 / 确认 / 放弃
    │ 确认
    ▼
保存到 .astro/skills/{skill_name}/SKILL.md
```

### 2.3 LLM 提取逻辑

提取 prompt 包含：

- 最近 3-5 次相似任务的完整执行记录（goal + steps + tools + result）
- 指令：抽象出通用 goal、参数化可变部分、列出所需 tools

生成的 SKILL.md 填充以下 frontmatter：

```yaml
---
name: skill_name
description: 一句话描述
version: 0.1.0
author: auto-generated
triggers:
  - "关键词1"
  - "关键词2"
tools_required:
  - tool_name
parameters:
  - name: param1
    type: string
    description: 参数说明
    required: true
---
```

### 2.4 用户审批 UI

- 桌面端弹出 Skill 预览面板，显示生成的 SKILL.md 全文
- 用户可直接编辑 Markdown 内容
- 三个按钮：**保存** / **编辑后保存** / **放弃**
- 放弃后 30 天内不再为该模式重复建议

---

## 3. 外部 Skill 目录

### 3.1 配置方式

在 `~/.astro/config.yaml` 或项目级 `.astro/config.yaml` 中添加：

```yaml
extra_skill_dirs:
  - "/Users/shared/team-skills"
  - "/opt/company/astro-skills"
```

### 3.2 加载优先级

Skill 名称冲突时，按以下优先级选择（高优先级覆盖低优先级）：

```
1. 项目级    .astro/skills/          （最高）
2. 全局级    ~/.astro/skills/
3. 外部目录  extra_skill_dirs（按配置顺序）
4. 内置      built-in skills         （最低）
```

### 3.3 加载行为

- 启动时扫描所有目录，构建 Skill 索引（name → path 映射）
- 外部目录不存在时记录 warn 日志，不阻塞启动
- 文件变更通过 `notify` crate 监听，热加载无需重启
- 外部目录的 Skill 标记为 `source: external`，在 UI 中显示来源路径

### 3.4 团队共享场景

```
/opt/team-skills/
├── deploy-k8s/
│   └── SKILL.md
├── code-review/
│   └── SKILL.md
└── db-migration/
    └── SKILL.md
```

团队通过 Git 仓库管理共享 Skill 目录，各成员配置 `extra_skill_dirs` 指向本地 clone 路径。

---

## 4. Skills Hub 信任体系

### 4.1 信任等级

| 等级 | 标识 | 来源 | 安装行为 |
|------|------|------|----------|
| `official` | 🟢 | 内置 / Astro 团队维护 | 直接可用，无需安装 |
| `verified` | 🔵 | GitHub 审核通过 | 安装后直接可用 |
| `community` | 🟡 | 用户上传 | 安装后首次执行需审批 |

### 4.2 安装安全扫描

安装 community 级 Skill 时自动执行静态扫描：

```rust
fn scan_skill(content: &str) -> ScanResult {
    let mut warnings = Vec::new();

    // 检测 shell 注入 pattern
    let dangerous_patterns = [
        r"rm\s+-rf\s+/",
        r"curl\s+.*\|\s*sh",
        r"eval\s*\(",
        r"exec\s*\(",
        r"sudo\s+",
        r"chmod\s+777",
    ];

    for pattern in &dangerous_patterns {
        if Regex::new(pattern).unwrap().is_match(content) {
            warnings.push(Warning::DangerousCommand(pattern.to_string()));
        }
    }

    // 检测是否请求高风险工具
    let high_risk_tools = ["shell_execute", "file_delete", "network_raw"];
    // ...

    ScanResult { warnings, passed: warnings.is_empty() }
}
```

扫描结果展示给用户：

- 无警告 → 显示 "安全扫描通过 ✓"
- 有警告 → 列出具体警告，用户确认后才安装

### 4.3 隔离区机制

- 新安装的 community Skill 进入隔离区（`quarantine: true`）
- 隔离区内的 Skill 首次执行时弹出确认：  
  "Skill `{name}` 来自社区，首次执行需要您的确认。查看详情 / 执行 / 取消"
- 用户确认执行后解除隔离
- 隔离状态存储在 `~/.astro/skills_registry.json`

### 4.4 Skills Hub MVP

v0.3 MVP 不建设独立平台，直接基于 GitHub：

- 官方仓库 `astro-agent/skills-hub` 收录 verified Skill
- 安装命令：`skill_manage install github:user/repo/skill_name`
- 搜索：`skill_manage search "关键词"`（搜索官方仓库 README 索引）

---

## 5. Skill 版本管理

### 5.1 版本号规范

遵循 semver（`major.minor.patch`），在 SKILL.md frontmatter 的 `version` 字段声明：

```yaml
---
name: deploy-k8s
version: 1.2.0
min_astro_version: 0.3.0
---
```

### 5.2 版本检查与更新

```bash
# 检查所有已安装 Skill 的更新
skill_manage update --check

# 更新指定 Skill
skill_manage update deploy-k8s

# 更新所有
skill_manage update --all
```

更新流程：

1. 比较本地 `version` 与上游 `version`
2. 上游版本更高 → 下载新版本
3. 如果 `major` 版本变化 → 警告可能有 breaking change，需用户确认
4. 备份当前版本到 `~/.astro/skills_versions/{name}/{version}/`
5. 替换为新版本

### 5.3 回滚

保留最近 3 个版本的完整备份：

```bash
# 查看可回滚版本
skill_manage versions deploy-k8s
# 输出：
#   1.2.0 (current)
#   1.1.0
#   1.0.0

# 回滚到指定版本
skill_manage rollback deploy-k8s 1.1.0
```

### 5.4 存储结构

```
~/.astro/
├── skills/                          # 当前活跃版本
│   └── deploy-k8s/
│       └── SKILL.md
├── skills_versions/                 # 历史版本备份
│   └── deploy-k8s/
│       ├── 1.0.0/SKILL.md
│       └── 1.1.0/SKILL.md
└── skills_registry.json             # 安装来源、版本、信任等级
```

`skills_registry.json` 结构：

```json
{
  "deploy-k8s": {
    "version": "1.2.0",
    "source": "github:astro-agent/skills-hub/deploy-k8s",
    "trust_level": "verified",
    "installed_at": "2026-07-15T10:30:00Z",
    "quarantine": false
  }
}
```

---

## 6. MVP 范围与排期

| 功能 | MVP 范围 | 优先级 |
|------|----------|--------|
| Agent 自动写 Skill | 触发 + 生成 + 审批流程 | P1 |
| 外部 Skill 目录 | config.yaml 配置 + 加载优先级 | P1 |
| Skills Hub | 基于 GitHub 的安装/搜索 | P2 |
| 安全扫描 | 正则 pattern 检测 | P2 |
| 版本管理 | update + rollback 基本流程 | P2 |

---

## 7. 风险与约束

- **自动生成质量**：LLM 生成的 Skill 可能不够通用，依赖用户审批把关
- **安全扫描局限**：正则匹配无法覆盖所有注入手法，MVP 阶段接受此局限
- **版本存储**：3 个历史版本的磁盘占用极小（纯文本），无需担心存储压力
