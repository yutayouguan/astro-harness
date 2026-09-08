# TOOLS.md — 工具与环境备忘

本文件记录机器与工作区的具体信息，不定义工具权限或替代原生 schema；技能负责可复用流程，运行时决定实际可用工具。

## 本地路径与环境

- 当前 Agent 记忆空间：本文件所在目录
- Astro 数据根目录：~/.astro
- 公共技能：~/.astro/skills
- Agent 专属技能：本目录 skills/
- 项目执行根目录：以当前任务注入的 project_root / workspace_roots 为准，不默认等于记忆空间

按实际需要记录 SSH 主机别名、设备昵称、已确认的 Provider 配置选择和 TTS 音色；不要在这里保存 API key、密码或令牌。

## 生成物目录

新建独立生成物优先写入工作区 generated/ 分类目录；修改已有项目时遵循项目目录结构，不把项目源码搬入 generated/。

- 图：generated/images/
- 视频：generated/videos/
- 音频：generated/audio/
- 单文件代码：generated/code/
- 多文件小工程：generated/project/
- 办公文档（PDF/Word/PPTX/Excel）：generated/docs/
- HTML：generated/html/
- 其它：generated/other/

向用户展示文件时遵循运行时提供的媒体/代码卡片格式，本文件不复制展示协议。

## 上下文工具速查

仅在当前工具目录中可用时使用；具体参数、限制和状态语义以原生 schema 与返回值为准。

- notes：读取或原子替换当前线程检查点；写入前读取 revision，不是长期记忆。
- history：当前线程的只读历史证据；已知引用用 read_item，未知则 list_items / search_contents。继续使用返回的分页游标。
- context_search：按需搜索会话、长期记忆或知识库；不能代替按引用精确回读。
- get_context_remaining：最近采样的上下文占用快照，不是实时剩余生成预算。
- new_context_window：请求压缩；排队不代表完成，后续检查 compaction_status。

## 可选命令行工具

运行时会探测 PATH 并注入可用状态。没有检测到或没有实际验证的命令不默认存在，也不因本备忘而自动安装或升级。

RTK 的可用性和适用命令以运行时注入说明为准；精确/机器可读输出、补丁和交互式命令不要盲目改写。
