import { useState, type ComponentProps } from "react";
import type { Meta, StoryObj } from "@storybook/react-vite";
import { ThemeProvider, useTheme } from "../hooks/app/useTheme";
import { LocaleProvider } from "../i18n/LocaleContext";
import { MorphiconProvider } from "../hooks/app/useMorphicons";
import { DialogProvider } from "../hooks/ui/DialogContext";
import dockMeta from "./ChatDockLayout.stories";
import sidebarMeta from "./SidebarSessionStates.stories";
import providersMeta from "./ProvidersPanel.stories";

const Dock = dockMeta.component;
const Sidebar = sidebarMeta.component;
const Providers = providersMeta.component;

function SoftWorkbench({
  surface,
}: {
  surface: "dock" | "sidebar" | "providers";
}) {
  const { mode, setMode, material, setMaterial } = useTheme();
  const [dock, setDock] =
    useState<ComponentProps<typeof Dock>["kind"]>("files");
  return (
    <>
      {surface === "dock" ? <Dock kind={dock} onSelect={setDock} /> : null}
      {surface === "sidebar" ? <Sidebar /> : null}
      {surface === "providers" ? (
        <main
          className="settings-content-inline"
          style={{
            height: "100vh",
            padding: 20,
            background: "var(--soft-base, var(--bg))",
          }}
        >
          <Providers {...providersMeta.args} />
        </main>
      ) : null}
      <nav
        aria-label="样板外观"
        style={{
          position: "fixed",
          right: 16,
          bottom: 12,
          zIndex: 10000,
          display: "flex",
          gap: 8,
          padding: 8,
          borderRadius: 12,
          background: "var(--surface-panel-background)",
          color: "var(--ink)",
        }}
      >
        <button
          type="button"
          aria-pressed={mode === "light"}
          onClick={() => setMode("light")}
        >
          亮色
        </button>
        <button
          type="button"
          aria-pressed={mode === "dark"}
          onClick={() => setMode("dark")}
        >
          暗色
        </button>
        <button
          type="button"
          onClick={() => setMaterial(material === "soft" ? "glass" : "soft")}
        >
          材质：{material}
        </button>
      </nav>
    </>
  );
}

const meta = {
  title: "Design/Soft Workbench",
  component: SoftWorkbench,
  args: { surface: "dock" },
  decorators: [
    (Story) => (
      <ThemeProvider>
        <LocaleProvider>
          <MorphiconProvider>
            <DialogProvider>
              <Story />
            </DialogProvider>
          </MorphiconProvider>
        </LocaleProvider>
      </ThemeProvider>
    ),
  ],
} satisfies Meta<typeof SoftWorkbench>;
export default meta;
type Story = StoryObj<typeof meta>;
export const Docks: Story = { beforeEach: () => sidebarMeta.beforeEach() };
export const SidebarChrome: Story = {
  args: { surface: "sidebar" },
  beforeEach: () => sidebarMeta.beforeEach(),
};
export const ProviderSettings: Story = {
  args: { surface: "providers" },
  beforeEach: (context) => providersMeta.beforeEach(context),
};
