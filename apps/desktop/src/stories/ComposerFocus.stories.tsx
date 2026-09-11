import type { Meta, StoryObj } from "@storybook/react-vite";
import { useTheme } from "../hooks/app/useTheme";
import softMeta from "./SoftMaterial.stories";

/** Uses the production editor structure/classes, without sending real messages. */
function ComposerFocusSample() {
  const { setMode, material, setMaterial } = useTheme();
  return (
    <div>
      <div style={{ display: "flex", gap: 12 }}>
        <button type="button" onClick={() => setMode("light")}>
          亮色
        </button>
        <button type="button" onClick={() => setMode("dark")}>
          暗色
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
      </div>
      {(["expanded", "capsule"] as const).map((layout) => (
        <section
          key={layout}
          aria-label={layout}
          style={{ position: "relative", height: 210 }}
        >
          <h2>{layout === "capsule" ? "胶囊输入" : "展开输入"}</h2>
          <form
            className={`composer-shell ${layout === "capsule" ? "is-capsule" : ""}`}
            onSubmit={(event) => event.preventDefault()}
          >
            <div className="composer composer--stacked">
              <div className="composer-input-wrap">
                <textarea
                  className="composer-input"
                  aria-label={`${layout} 输入框`}
                  rows={2}
                  placeholder="点击或用 Tab 聚焦，不应出现内部方框"
                />
              </div>
              <div className="composer-bar">
                <div className="composer-bar-left">
                  <button type="button" className="composer-mode-pill">
                    默认模式
                  </button>
                </div>
                <div className="composer-bar-right">
                  <button
                    type="button"
                    className="send-btn send-btn--round"
                    aria-label={`${layout} 发送`}
                  >
                    ↑
                  </button>
                </div>
              </div>
            </div>
          </form>
        </section>
      ))}
      <label>
        独立文本框（保留自身焦点框）
        <textarea aria-label="独立文本框" />
      </label>
    </div>
  );
}

const meta = {
  title: "Design/Composer Focus",
  component: ComposerFocusSample,
  decorators: softMeta.decorators,
  beforeEach: softMeta.beforeEach,
} satisfies Meta<typeof ComposerFocusSample>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Regression: Story = {};
