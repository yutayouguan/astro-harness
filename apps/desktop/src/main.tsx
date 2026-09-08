/** 应用入口：主题预热、错误边界、Theme / Locale Provider 与 App 挂载。 */
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import DesktopPetSurface from "./components/desktop-pet/DesktopPetSurface";
import OnboardingGate from "./components/onboarding/OnboardingGate";
import { LocaleProvider } from "./i18n/LocaleContext";
import { ActiveAgentProvider } from "./hooks/app/useActiveAgent";
import { MorphiconProvider } from "./hooks/app/useMorphicons";
import { ThemeProvider } from "./hooks/app/useTheme";
import { DialogProvider } from "./hooks/ui/DialogContext";
import { installContextMenuGuard } from "./lib/ui/contextMenuGuard";
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
import "./styles/index.css";
import "./styles/features/settings-material-unified.css";

installContextMenuGuard();

const isDesktopPetWindow =
  new URLSearchParams(window.location.search).get("surface") === "desktop-pet";
if (isDesktopPetWindow) {
  document.documentElement.dataset.windowSurface = "desktop-pet";
}

(() => {
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
    document.documentElement.style.colorScheme = resolved;
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
      ) : (
        <ThemeProvider>
          <MorphiconProvider>
            <LocaleProvider>
              <ActiveAgentProvider>
                <DialogProvider>
                  <OnboardingGate>
                    <App />
                  </OnboardingGate>
                </DialogProvider>
              </ActiveAgentProvider>
            </LocaleProvider>
          </MorphiconProvider>
        </ThemeProvider>
      )}
    </RootErrorBoundary>
  </React.StrictMode>,
);
