import type { Meta, StoryObj } from "@storybook/react-vite";
import {
  ArrowUp,
  Hand,
  Infinity as InfinityIcon,
  Maximize2,
  Mic,
  Minimize2,
  Plus,
} from "lucide-react";
import type { CSSProperties } from "react";
import softMeta from "./SoftMaterial.stories";

/**
 * 胶囊输入（生产结构/类名）：展开控件在输入区右侧留白内，
 * 与右侧动作组同高同尺寸，不压卡片上沿、不遮麦克风。
 */
function CapsuleComposerSample({ text = "" }: { text?: string }) {
  return (
    <div
      className="app-shell"
      data-tone="blue"
      style={
        {
          position: "relative",
          height: 220,
          background: "var(--shell-bg)",
        } as CSSProperties
      }
    >
      <form
        className="composer-shell is-capsule"
        onSubmit={(event) => event.preventDefault()}
      >
        <div className="composer composer--stacked">
          <div className="composer-input-wrap">
            <textarea
              className="composer-input"
              aria-label="胶囊输入"
              rows={2}
              defaultValue={text}
              placeholder="输入消息，或 / 唤醒指令"
            />
            <button
              type="button"
              className="composer-expand-btn"
              aria-expanded={false}
              aria-label="展开输入框"
              title="展开输入框"
            >
              <svg
                className="composer-expand-indicator"
                viewBox="0 0 44 44"
                aria-hidden
              >
                <path d="M 22 11 A 11 11 0 0 1 33 22" pathLength={1} />
              </svg>
              <span className="composer-expand-glyph" aria-hidden>
                <Maximize2
                  className="composer-expand-icon is-expand"
                  size={12}
                  strokeWidth={2.2}
                />
                <Minimize2
                  className="composer-expand-icon is-collapse"
                  size={12}
                  strokeWidth={2.2}
                />
              </span>
            </button>
          </div>
          <div className="composer-bar">
            <div className="composer-bar-left">
              <button
                type="button"
                className="composer-mode-pill composer-policy-pill"
                aria-label="模式与审批"
              >
                <InfinityIcon
                  className="composer-mode-current-icon"
                  size={15}
                  strokeWidth={2.2}
                  aria-hidden
                />
                <span className="composer-mode-pill-label">Agent</span>
                <span className="composer-policy-separator" aria-hidden>
                  ·
                </span>
                <Hand
                  className="composer-policy-current-icon"
                  size={15}
                  strokeWidth={2.2}
                  aria-hidden
                />
                <span className="composer-policy-approval">按需审批</span>
              </button>
              <button
                type="button"
                className="composer-icon-btn composer-plus-btn"
                aria-label="添加"
              >
                <Plus size={17} strokeWidth={2.2} />
              </button>
            </div>
            <div className="composer-bar-right">
              <button
                type="button"
                className="composer-icon-btn composer-realtime-btn"
                aria-label="语音通话"
              >
                <Mic size={17} strokeWidth={2.1} />
              </button>
              <div className="composer-context-wrap">
                <button
                  type="button"
                  className="composer-icon-btn composer-context-btn"
                  aria-label="上下文使用 2%"
                >
                  <svg
                    className="composer-context-ring"
                    viewBox="0 0 24 24"
                    aria-hidden
                  >
                    <circle
                      className="composer-context-ring-track"
                      cx="12"
                      cy="12"
                      r="9"
                      pathLength="100"
                    />
                    <circle
                      className="composer-context-ring-value"
                      cx="12"
                      cy="12"
                      r="9"
                      pathLength="100"
                      strokeDasharray="2 100"
                    />
                  </svg>
                </button>
              </div>
              <button
                className="send-btn send-btn--round"
                type="submit"
                aria-label="发送"
              >
                <ArrowUp size={19} strokeWidth={2.2} aria-hidden />
              </button>
            </div>
          </div>
        </div>
      </form>
    </div>
  );
}

const meta = {
  title: "Design/Composer Capsule",
  component: CapsuleComposerSample,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
  parameters: { layout: "fullscreen" },
} satisfies Meta<typeof CapsuleComposerSample>;
export default meta;
type Story = StoryObj<typeof meta>;

export const Empty: Story = {};

export const WithText: Story = {
  args: { text: "帮我梳理一下今天的定时任务执行情况" },
};
