import type { Meta, StoryObj } from "@storybook/react-vite";
import { Plus, Square } from "lucide-react";
import TodoProgress from "../components/chat/TodoProgress";
import type { ChatMessage } from "../types";

const messages: ChatMessage[] = [
  {
    id: "task-progress-user",
    role: "user",
    content: "按计划继续完成工具协议对齐。",
  },
  {
    id: "task-progress-assistant",
    role: "assistant",
    content: "继续按分批闭环推进，完成当前实现后会运行针对性验证。",
    activities: [
      {
        id: "task-progress-todo",
        kind: "tool",
        title: "todo",
        input: JSON.stringify({
          action: "create",
          title: "工具协议对齐",
          items: [
            { text: "审计工具注册、构建链与调用链", done: true },
            { text: "对齐 Provider 原生工具规格", done: true },
            { text: "补齐 tool_search 激活链路", done: true },
            { text: "打通 Responses 原生请求与流事件", done: false },
            { text: "对齐 exec / wait 编排工具", done: false },
            { text: "执行跨 crate 定向验证", done: false },
          ],
        }),
        status: "done",
      },
      {
        id: "task-progress-patch",
        kind: "tool",
        title: "apply_patch",
        input: JSON.stringify(`*** Begin Patch
*** Update File: crates/agent-core/src/runtime/mod.rs
-old
+new
+extra
*** Update File: crates/agent-core/src/streaming/tool_dispatch.rs
-old
+new
+line
+line
*** Update File: crates/agent-core/src/streaming/tool_router.rs
-old
-old
+new
+line
+line
+line
*** Update File: crates/agent-tools/src/tool_registry_test.rs
-old
+new
*** Update File: crates/agent-providers/src/completion.rs
-old
-old
+new
+line
*** Update File: crates/agent-providers/src/responses.rs
-old
+new
*** End Patch`),
        status: "done",
      },
    ],
  },
];

function TodoProgressPreview() {
  return (
    <main className="app-shell" data-tone="blue" style={{ height: "100vh" }}>
      <section className="chat-pane">
        <div className="message-list" style={{ paddingTop: 120 }}>
          <div className="msg-row user">
            <div className="msg-stack">
              <article className="bubble user">
                按计划继续完成工具协议对齐。
              </article>
            </div>
          </div>
          <div className="msg-row assistant">
            <div className="avatar" aria-hidden>
              AI
            </div>
            <div className="msg-stack">
              <article className="bubble assistant">
                继续按分批闭环推进，完成当前实现后会运行针对性验证。
              </article>
            </div>
          </div>
        </div>
        <form
          className="composer-shell"
          onSubmit={(event) => event.preventDefault()}
        >
          <TodoProgress messages={messages} />
          <div className="composer composer--stacked">
            <div className="composer-input-wrap">
              <textarea
                className="composer-input"
                placeholder="随心输入"
                rows={2}
              />
            </div>
            <div className="composer-bar">
              <div className="composer-bar-left">
                <button
                  type="button"
                  className="composer-icon-btn"
                  aria-label="添加与插件"
                >
                  <Plus size={17} />
                </button>
              </div>
              <div className="composer-bar-right">
                <button
                  type="button"
                  className="composer-send"
                  aria-label="停止生成"
                >
                  <Square size={13} fill="currentColor" />
                </button>
              </div>
            </div>
          </div>
        </form>
      </section>
    </main>
  );
}

const meta = {
  title: "Chat/Todo Progress",
  component: TodoProgressPreview,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof TodoProgressPreview>;

export default meta;
type Story = StoryObj<typeof meta>;

export const HoverPanels: Story = {};
