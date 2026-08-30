import { useState, type CSSProperties } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import DynamicPaletteButton from "../components/ui/DynamicPaletteButton";

const palettes = [
  { primary: "#2563eb", secondary: "#7c3aed" },
  { primary: "#0891b2", secondary: "#16a34a" },
  { primary: "#db2777", secondary: "#ea580c" },
];

function DynamicPalettePreview() {
  const [paletteIndex, setPaletteIndex] = useState(0);
  const palette = palettes[paletteIndex];
  const style = {
    "--tone": palette.primary,
    "--tone-soft": `color-mix(in srgb, ${palette.primary} 24%, transparent)`,
    minHeight: "100vh",
    position: "relative",
    overflow: "hidden",
    color: "var(--ink)",
    background: `
      radial-gradient(circle at 82% 86%, color-mix(in srgb, ${palette.secondary} 24%, transparent), transparent 44%),
      radial-gradient(circle at 18% 12%, color-mix(in srgb, ${palette.primary} 28%, transparent), transparent 48%),
      var(--shell-bg)
    `,
  } as CSSProperties;

  return (
    <main style={style}>
      <div style={{ padding: 32 }}>
        <strong>灵动配色 {paletteIndex + 1}</strong>
        <p style={{ color: "var(--ink-mute)" }}>点击右下角小风车重新生成配色</p>
      </div>
      <DynamicPaletteButton
        label="重新生成配色"
        onReshuffle={() => setPaletteIndex((current) => (current + 1) % palettes.length)}
      />
    </main>
  );
}

const meta = {
  title: "Shell/Dynamic Palette Button",
  component: DynamicPaletteButton,
  args: {
    label: "重新生成配色",
    onReshuffle: () => {},
  },
  parameters: { layout: "fullscreen" },
  render: () => <DynamicPalettePreview />,
} satisfies Meta<typeof DynamicPaletteButton>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
