/** 应用入口：主题预热、错误边界、Theme / Locale Provider 与 App 挂载。 */
// Establish the CSS layer order before components can import their own styles.
import "./styles/index.css";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import DesktopPetSurface from "./components/desktop-pet/DesktopPetSurface";
import PetTaskSurface from "./components/desktop-pet/PetTaskSurface";
import OnboardingGate from "./components/onboarding/OnboardingGate";
import { LocaleProvider } from "./i18n/LocaleContext";
import { ActiveAgentProvider } from "./hooks/app/useActiveAgent";
import { MorphiconProvider } from "./hooks/app/useMorphicons";
import { ThemeProvider } from "./hooks/app/useTheme";
import { DialogProvider } from "./hooks/ui/DialogContext";
import { installContextMenuGuard } from "./lib/ui/contextMenuGuard";
import {
  applyInterfaceMaterial,
  readStoredInterfaceMaterial,
} from "./lib/ui/interfaceMaterial";
import {
  applyGlassIntensity,
  DEFAULT_GLASS_INTENSITY,
  readStoredGlassIntensity,
} from "./lib/ui/glassIntensity";
import {
  applyInterfaceScale,
  DEFAULT_INTERFACE_SCALE,
  readStoredInterfaceScale,
} from "./lib/ui/interfaceScale";
import "./styles/features/settings-material-unified.css";
import "./styles/features/pet-tasks.css";

installContextMenuGuard();

const isDesktopPetWindow =
  new URLSearchParams(window.location.search).get("surface") === "desktop-pet";
const taskSurface = new URLSearchParams(window.location.search).get("surface");
const isPetTaskWindow =
  taskSurface === "pet-task-badge" || taskSurface === "pet-task-popup";
if (isPetTaskWindow)
  document.documentElement.dataset.windowSurface = "pet-task";
if (isDesktopPetWindow) {
  document.documentElement.dataset.windowSurface = "desktop-pet";
  document.documentElement.style.colorScheme = "normal";
}

(() => {
  // Material is independent of wallpaper/theme, and must exist before first paint.
  // Companion windows keep their transparent canvas and existing material.
  if (!isDesktopPetWindow && !isPetTaskWindow) {
    applyInterfaceMaterial(
      document.documentElement,
      readStoredInterfaceMaterial(),
    );
  }
  try {
    const mode = localStorage.getItem("astro-theme-mode");
    const resolved =
      mode === "light"
        ? "light"
        : mode === "dark"
          ? "dark"
          : window.matchMedia("(prefers-color-scheme: dark)").matches
            ? "dark"
            : "light";
    document.documentElement.dataset.theme = resolved;
    // A native transparent WebView must not receive an opaque UA canvas color.
    document.documentElement.style.colorScheme = isDesktopPetWindow
      ? "normal"
      : resolved;
    applyGlassIntensity(
      document.documentElement,
      readStoredGlassIntensity(window.localStorage),
    );
    applyInterfaceScale(
      document.documentElement,
      readStoredInterfaceScale(window.localStorage),
    );
  } catch {
    document.documentElement.dataset.theme = "dark";
    applyGlassIntensity(document.documentElement, DEFAULT_GLASS_INTENSITY);
    applyInterfaceScale(document.documentElement, DEFAULT_INTERFACE_SCALE);
  }
})();

class RootErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            padding: 24,
            fontFamily:
              'system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
            color: "var(--ink, #e2e8f0)",
            background: "var(--bg0, #0f172a)",
            minHeight: "100%",
            boxSizing: "border-box",
          }}
        >
          <h1 style={{ fontSize: 18, margin: "0 0 8px" }}>界面渲染失败</h1>
          <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, opacity: 0.85 }}>
            {this.state.error.message}
          </pre>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <RootErrorBoundary>
      {isDesktopPetWindow ? (
        <DesktopPetSurface />
      ) : isPetTaskWindow ? (
        <LocaleProvider>
          <PetTaskSurface badge={taskSurface === "pet-task-badge"} />
        </LocaleProvider>
      ) : (
        <ThemeProvider>
          <MorphiconProvider>
            <LocaleProvider>
              <OnboardingGate>
                <ActiveAgentProvider>
                  <DialogProvider>
                    <App />
                  </DialogProvider>
                </ActiveAgentProvider>
              </OnboardingGate>
            </LocaleProvider>
          </MorphiconProvider>
        </ThemeProvider>
      )}
    </RootErrorBoundary>
  </React.StrictMode>,
);
