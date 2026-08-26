import type { Meta, StoryObj } from "@storybook/react-vite";

/** 玻璃强度档；与 useTheme 的 GlassLevel 对齐，rich 用空值表示 token 基线。 */
const VARIANTS = [
  { id: undefined, label: "rich（旧磨砂）" },
  { id: "liquid-soft", label: "liquid-soft（克制）" },
  { id: "liquid", label: "liquid（贴近参考稿）" },
] as const;

/** 背后的彩色图标块：玻璃要有高频细节可透，模糊和折光才看得出差别。 */
const SWATCHES = [
  "linear-gradient(150deg, #ff6b6b, #ee0979)",
  "linear-gradient(150deg, #f7971e, #ffd200)",
  "linear-gradient(150deg, #11998e, #38ef7d)",
  "linear-gradient(150deg, #2193b0, #6dd5ed)",
  "linear-gradient(150deg, #a044ff, #6a3093)",
  "linear-gradient(150deg, #232526, #414345)",
];

/** 白色点阵，模拟应用图标里的细节；没有它模糊前后几乎一个样。 */
const DOTS =
  "radial-gradient(rgba(255,255,255,0.85) 1.6px, transparent 1.7px) 0 0 / 9px 9px";

function ColorBed() {
  return (
    <div
      aria-hidden
      style={{
        position: "absolute",
        inset: 0,
        display: "grid",
        gridTemplateColumns: "repeat(5, 1fr)",
        gridAutoRows: "1fr",
        gap: 10,
        padding: 14,
      }}
    >
      {Array.from({ length: 20 }, (_, index) => (
        <div
          key={index}
          style={{
            borderRadius: 16,
            background:
              index % 3 === 0
                ? `${DOTS}, ${SWATCHES[index % SWATCHES.length]}`
                : SWATCHES[index % SWATCHES.length],
          }}
        />
      ))}
    </div>
  );
}

function GlassStack() {
  return (
    <div
      style={{
        position: "relative",
        display: "flex",
        flexDirection: "column",
        gap: 12,
        padding: 26,
      }}
    >
      <div className="msg-activity" data-kind="tool" style={{ margin: 0 }}>
        <div className="msg-activity-body">
          <div className="msg-activity-summary" style={{ padding: "10px 12px" }}>
            <span className="msg-activity-title">terminal</span>
            <span className="msg-activity-duration">1.2s</span>
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
        {["The Best", "Mac Launchpad", "Alternative"].map((text) => (
          <span
            key={text}
            style={{
              padding: "8px 16px",
              borderRadius: 999,
              fontSize: 13,
              fontWeight: 600,
              color: "var(--ink)",
              background: "var(--chip-bg)",
              border: "1px solid var(--chip-border)",
              boxShadow: "var(--glass-rim)",
              backdropFilter: "var(--backdrop-glass)",
              WebkitBackdropFilter: "var(--backdrop-glass)",
            }}
          >
            {text}
          </span>
        ))}
      </div>

      <div className="app-dialog" style={{ width: "100%" }}>
        <div className="app-dialog-head">
          <div className="app-dialog-icon" aria-hidden>
            ✦
          </div>
          <div className="app-dialog-copy">
            <h3>浮层玻璃</h3>
            <p>验证折光边缘、透视饱和度与文字可读性。</p>
          </div>
        </div>
      </div>
    </div>
  );
}

function LiquidGlassComparison() {
  return (
    <main
      style={{
        minHeight: "100vh",
        padding: 32,
        background: "var(--shell-bg)",
        color: "var(--ink)",
      }}
      data-testid="liquid-glass-comparison"
    >
      <div
        style={{
          display: "grid",
          gridTemplateColumns: "repeat(3, minmax(0, 1fr))",
          gap: 24,
        }}
      >
        {VARIANTS.map(({ id, label }) => (
          <section key={label} data-glass={id}>
            <p
              style={{
                margin: "0 0 10px",
                fontSize: 12,
                fontWeight: 700,
                letterSpacing: "0.08em",
                color: "var(--ink-mute)",
              }}
            >
              {label}
            </p>
            <div
              style={{
                position: "relative",
                minHeight: 420,
                borderRadius: 26,
                overflow: "hidden",
                background: "var(--bg-base)",
              }}
            >
              <ColorBed />
              <GlassStack />
            </div>
          </section>
        ))}
      </div>
    </main>
  );
}

const meta = {
  id: "liquid-glass",
  title: "Design/Liquid Glass",
  component: LiquidGlassComparison,
  parameters: {
    controls: { disable: true },
  },
} satisfies Meta<typeof LiquidGlassComparison>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Comparison: Story = {};
