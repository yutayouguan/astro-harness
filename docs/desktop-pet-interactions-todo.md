# 桌宠任务与交互 TODO

- [x] 活 HITL/MCP 请求快照与生命周期通知、逐请求校验/提交。
- [x] 独立徽标和弹窗、免打扰、当前可见请求去重、屏幕边界/焦点策略。
- [x] 主会话共享请求控制器、原问题表单、长期授权二次确认、内存草稿。
- [x] 安全会话跳转，保留当前草稿、附件、排队输入和临时会话。
- [x] 多请求/过期/重复提交/断线回归测试及接口文档。
- [ ] 原生验收（下方按实测结果记录，不以 Storybook 替代）。
- [x] 完成最终构建和范围内提交，不推送。

范围：不调用图片模型，不改变宠物动画；任务/回答不写入桌宠设置文件。

## 接口与事实源

`SessionTask → live_interactions 弱引用注册表 + HitlRegistry / MCP broker →
Get/WatchPendingInteractions → Tauri PetTasks → shared usePendingInteractions →
InteractionCard（主会话和桌宠）`。

- `GetPendingInteractions(Empty)` 返回 `PendingInteractionsJson`；内部为共享 `InteractionSnapshot`。
- `WatchPendingInteractions(Empty)` 订阅同一快照。gate/broker 的新增、解决、取消、超时触发无内容 invalidation，服务端重新读取活对象；每秒额外核对任务状态、标题和过期，不从历史消息推断可操作请求。
- 快照包含随机服务 `epoch`、递增 `revision`、任务和请求。服务重启后新 epoch 接管，迟到的旧 epoch/revision 不可使待办复活。断线时保留草稿、禁用提交。
- `RespondPendingInteraction` 接收 `key/sessionId/turnId/action/payload/confirmedPersistent`。服务端再次读取活请求，校验回合、有效期和允许动作；审批 payload 由服务端选项决定，不信任客户端附加权限字段。
- HITL 沿用 `interrupt_resume` 和原 gate waiter；MCP 使用同一 broker 的 generation-checked resolve，防止服务端重用 request ID 后旧回答误命中新请求。后台、子任务的实际 Session 也进入 home-scoped 弱引用注册表。
- 完整响应 schema 随请求发送。只支持共享校验器明确实现的子集；未知约束、复杂表单和外部 URL 授权回退“请到会话处理”，不显示无效快捷提交按钮。原始问题与操作说明不经过 AI 改写。
- 原主会话卡片优先转入共享逐请求控制器；无法唯一绑定时拒绝批量提交。长期权限在主会话和桌宠都需要二次确认。

## 窗口和交互约定

- `desktop-pet` 不变；新建 `pet-task-badge`（166×38）与 `pet-task-popup`（440×560）两个独立窗口，按当前屏幕工作区缩放、贴边换侧和裁切。
- 自动出现时 `focusable=false/focused=false`；只有显式点击输入框才申请键盘焦点。“查看会话”才主动显示主窗口。
- 原生窗口查询不可在持有请求状态 mutex 时调用，避免与同步 WebView IPC 形成主线程互等。
- 新请求进入队列，不覆盖正在填写的请求。选择请求同时同步到原生窗口状态；关闭、Escape、拖动和“稍后处理”只隐藏，草稿留在当前进程内。
- 首次连接/重新建立快照仅显示数量；全屏、演示和桌宠隐藏期间不会积累恢复后的连续补弹。主窗口聚焦且同一请求实际可见时去重。
- 导航不发送模型请求、不丢弃临时会话、不清理当前 worktree；会话草稿/附件/待发队列按会话保留。无会话的新草稿在再次新建对话时恢复，恢复后删除缓存，避免旧草稿复活。输入正在提交时暂不切换。

## 验证记录（2026-09-11）

- 服务端请求测试：6 项通过，覆盖权限不可扩大、长期确认、schema 限制、活后台 gate 两请求仅消费一条、跨会话同名请求绑定、MCP 请求 ID 重用。
- 前端请求模型/回归契约：7 项通过；Storybook Playwright：3 项通过（多步原问题及失败重试、暗色表单、长期确认、同会话兄弟请求、无默认答案和草稿保留）。
- 已通过的范围测试：types 请求类型 1 项、core control 59 项（含过期和重复响应不可消费 waiter）、MCP elicitation 4 项、原生窗口策略/负坐标定位 2 项。
- 较早的前端全套 925 项中 923 通过；2 个既有 sidebarInformationArchitecture/windowChromeSafeArea 断言失败，与本功能无关。
- 较宽的 runtime/background 套件出现超时。`replacement_collects_only_the_new_background_turn` 在未修改 HEAD 和本改动均失败；HITL exact 测试在基线及本改动单独重跑均通过，未放宽原 2 秒断言。不得据此声称全套通过。
- 原生 QA 使用独立 marker-guarded home，无凭证导入、无模型请求。测试副本注入两个真实 gate waiter，批准仅消费测试响应，不执行命令或写授权规则；注入代码不进入仓库或普通应用包。

原生 QA 已构建并启动；gRPC 实查返回两个活请求，窗口循环记录 `visible=true tasks=1 requests=2 open=true`。随后 Mac 锁屏，CUA 明确无法继续，因此不把日志当作点击/焦点验收证据。QA 进程已结束，未触碰正式会话与凭证。

最终普通包通过 `tsc -b`、Vite 和 Tauri debug bundle 构建，产物为 `target/debug/bundle/macos/Astro Agent.app`。构建源为 HEAD 加本轮 36 个相关文件的隔离快照，不包含 QA 注入；未自动替换/重启正在使用的正式应用。Vite 仍有既有大 chunk 与混合动态导入警告。

尚待解锁后原生逐项确认：自动弹出不打断其他应用输入、单击填写、收起/恢复草稿、审批 ACK 后下一项、会话跳转/主窗去重、多屏边缘与透明区域穿透。

## 后续 Review 修复（2026-09-11）

本轮使用 code-review 审查上述提交，修复以下缺陷：

- **P1 — 旧 MCP 卡片动作误路由**：旧 A2UI 的 `deny/cancel` 原先被共享控制器统一当成 `submit`；向导 `answers/value` 也未还原成 MCP 的 schema 对象。现在保留 accept/decline/cancel 语义及字段类型；schema 真正声明的 `value/approved` 字段不再被误删。拒绝/取消不附带回答内容，普通 HITL 问题不能伪造未提供的取消操作。
- **P1 — 会话导航串流**：跳转前未撤销旧 UI listener，旧任务的 token/done 回调可能写入新会话状态。现在切换时提升 UI generation、解绑 listener 并清空旧缓冲，不取消后台任务。仅观察任务的会话串行刷新持久化历史；请求失败保留界面，迟到响应和新发送 generation 不可覆盖当前会话。确认任务结束后再读取一次最终历史，随后释放排队输入；同步结束/只读状态。点击当前会话只定位，不重建流。
- **P2 — MCP 同名请求冲突**：实际传输保留原始 request ID，服务器限定的标识在 `tool_call_id`。匹配及主窗待办清理由“会话＋MCP 服务器限定标识”确定，避免两个 MCP server 使用相同 request ID 时串用或残留。测试经过实际 `parseHitlRunFinished`，不只使用手造前端 ID。
- **P2 — 弹窗竞态**：拖动/隐藏会清理尚未触发的自动弹出计划；新候选过期不会重新弹出已稍后处理的旧请求。窗口刷新单独串行化；弹窗打开时隐藏重叠徽标；屏幕暂不可用时仍执行隐藏。
- **P2 — 迟到桌面响应覆盖选择**：桌面 envelope 增加 `uiRevision`，与后端 epoch/revision 分开；相同后端快照下的旧选择响应不再覆盖当前表单。快照获取的迟到错误也不能覆盖已经收到的有效实时状态。
- **P2 — 脱敏遗漏**：原正则漏掉 JSON 带引号的 key 和含空格的凭证值；已补齐对应处理和回归测试。

验证：前端行为/契约测试 17 项、服务端交互 6 项、原生窗口策略 4 项、Playwright 桌宠表单流程 3 项通过。隔离快照的 TypeScript、Vite 和普通 Tauri debug app bundle 构建通过；共享工作区另一个材质设置改动曾暂时缺少 i18n key，未纳入本次修改。

本轮前端全套：938 项中 936 通过、2 个既有侧栏断言失败（`sidebarInformationArchitecture.test.mjs` 的 icon-only controls、`windowChromeSafeArea.test.mjs` 的 sidebar occupancy）。测试读取的 `App.tsx` 与 HEAD blob 一致，未将基线失败混入本轮修复。

原生点击/键盘/多屏验收仍保留未完成状态；本轮不调用图片模型、不修改动画、不导入凭证、不自动重启正式应用。
