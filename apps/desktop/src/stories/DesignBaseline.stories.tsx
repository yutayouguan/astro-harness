import type { Meta, StoryObj } from "@storybook/react-vite";

function DesignBaseline() {
  return (
    <main
      data-testid="design-baseline"
      style={{
        minHeight: "100vh",
        padding: "48px",
        background: "var(--shell-bg)",
        color: "var(--ink)",
      }}
    >
      <div style={{ width: "min(920px, 100%)", margin: "0 auto" }}>
        <header style={{ marginBottom: 28 }}>
          <p
            style={{
              margin: "0 0 8px",
              color: "var(--tone)",
              fontSize: 12,
              fontWeight: 700,
              letterSpacing: "0.12em",
              textTransform: "uppercase",
            }}
          >
            Astro design baseline
          </p>
          <h1 style={{ margin: 0, fontSize: 30, letterSpacing: "-0.03em" }}>
            当前设计语言
          </h1>
          <p style={{ margin: "10px 0 0", color: "var(--ink-mute)" }}>
            直接使用产品现有 token 与样式类，作为后续组件原语迁移前的视觉基准。
          </p>
        </header>

        <section
          aria-label="按钮与表面"
          style={{
            display: "grid",
            gridTemplateColumns: "minmax(0, 1fr) minmax(0, 1fr)",
            gap: 20,
            marginBottom: 20,
          }}
        >
          <div
            className="app-dialog-emphasis"
            style={{ margin: 0, padding: 20 }}
          >
            <span className="app-dialog-emphasis-label">Surface</span>
            <strong className="app-dialog-emphasis-value">柔和玻璃表面</strong>
            <span
              style={{
                color: "var(--ink-mute)",
                fontSize: 13,
                lineHeight: 1.5,
              }}
            >
              表面、边框和文字均来自当前主题的语义 token。
            </span>
          </div>

          <div
            className="app-dialog-emphasis"
            style={{ margin: 0, padding: 20 }}
          >
            <span className="app-dialog-emphasis-label">Button</span>
            <div
              className="app-dialog-actions"
              style={{ justifyContent: "flex-start" }}
            >
              <button className="app-dialog-btn is-cancel" type="button">
                次要操作
              </button>
              <button className="app-dialog-btn is-confirm" type="button">
                主要操作
              </button>
            </div>
          </div>
        </section>

        <section aria-label="标签页" style={{ marginBottom: 20 }}>
          <div
            className="skills-main-tabs"
            role="tablist"
            aria-label="基线标签页"
          >
            <button
              className="skills-main-tab active"
              role="tab"
              aria-selected="true"
            >
              概览
            </button>
            <button
              className="skills-main-tab"
              role="tab"
              aria-selected="false"
            >
              活动
            </button>
            <button
              className="skills-main-tab"
              role="tab"
              aria-selected="false"
            >
              设置
            </button>
          </div>
        </section>

        <section
          aria-label="浮层"
          style={{
            minHeight: 270,
            display: "grid",
            placeItems: "center",
            padding: 24,
            borderRadius: "var(--radius-xl)",
            background: "color-mix(in srgb, var(--bg-base) 36%, transparent)",
            backdropFilter: "blur(8px)",
          }}
        >
          <div className="app-dialog" style={{ width: "min(400px, 100%)" }}>
            <div className="app-dialog-head">
              <div className="app-dialog-icon" aria-hidden="true">
                ✦
              </div>
              <div className="app-dialog-copy">
                <h3>浮层与操作</h3>
                <p>验证浮层玻璃、主题描边、层次阴影与操作按钮的组合表现。</p>
              </div>
            </div>
            <div className="app-dialog-actions">
              <button className="app-dialog-btn is-cancel" type="button">
                取消
              </button>
              <button className="app-dialog-btn is-confirm" type="button">
                确认
              </button>
            </div>
          </div>
        </section>
      </div>
    </main>
  );
}

const meta = {
  id: "design-baseline",
  title: "Design/Baseline",
  component: DesignBaseline,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof DesignBaseline>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CurrentLanguage: Story = {};
