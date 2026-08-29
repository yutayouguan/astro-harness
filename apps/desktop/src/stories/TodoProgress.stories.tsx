import type { Meta, StoryObj } from "@storybook/react-vite";
import { Plus, Square } from "lucide-react";
import { useState } from "react";
import ChatReviewPanel, {
  type ProjectGitDiff,
  type ReviewDiffLoader,
} from "../components/chat/ChatReviewPanel";
import TodoProgress from "../components/chat/TodoProgress";
import type { FileChangeItem } from "../lib/chat/taskProgress";
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

const storyFiles: FileChangeItem[] = [
  { path: "crates/agent-core/src/runtime/mod.rs", additions: 2, deletions: 1 },
  { path: "crates/agent-core/src/streaming/tool_dispatch.rs", additions: 3, deletions: 1 },
  { path: "crates/agent-core/src/streaming/tool_router.rs", additions: 4, deletions: 2 },
  { path: "crates/agent-tools/src/tool_registry_test.rs", additions: 1, deletions: 1 },
  { path: "crates/agent-providers/src/completion.rs", additions: 2, deletions: 2 },
  { path: "crates/agent-providers/src/responses.rs", additions: 1, deletions: 1 },
];

const loadStoryDiff: ReviewDiffLoader = async (_projectId, path) => {
  const patch = `diff --git a/${path} b/${path}
index 0f223e1..4cc38f2 100644
--- a/${path}
+++ b/${path}
@@ -18,6 +18,9 @@ export function registerTools(registry: ToolRegistry) {
   registry.register(coreTools);
-  registry.register(legacyTool);
+  registry.register(nativeToolSearch);
+  registry.register(namespaceRouter);
+  registry.enableDeferredTools();
   return registry;
 }
@@ -42,3 +45,4 @@ export function toolName(value: string) {
   return value.trim();
+  // Keep provider-native names visible in the review surface.
 }`;
  return {
    path,
    relativePath: path,
    patch,
    additions: 4,
    deletions: 1,
    isBinary: false,
  } satisfies ProjectGitDiff;
};

function TodoProgressPreview({ initialReview = false }: { initialReview?: boolean }) {
  const [review, setReview] = useState<{
    files: FileChangeItem[];
    selectedPath: string;
  } | null>(() => initialReview
    ? { files: storyFiles, selectedPath: storyFiles[0].path }
    : null);

  return (
    <main className="app-shell" data-tone="blue" style={{ height: "100vh" }}>
      <div className="chat-layout-with-right">
        <div className="chat-main">
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
              <TodoProgress
                messages={messages}
                onOpenFileReview={(file, files) =>
                  setReview({ files, selectedPath: file.path })
                }
              />
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
        </div>
        {review && (
          <ChatReviewPanel
            projectId="storybook"
            files={review.files}
            selectedPath={review.selectedPath}
            onSelectPath={(selectedPath) =>
              setReview((current) =>
                current ? { ...current, selectedPath } : current,
              )
            }
            onClose={() => setReview(null)}
            loadDiff={loadStoryDiff}
          />
        )}
      </div>
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

export const ReviewPanelOpen: Story = {
  render: () => <TodoProgressPreview initialReview />,
};
