/** 应用入口：主题预热、错误边界、Theme / Locale Provider 与 App 挂载。 */
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { LocaleProvider } from "./i18n/LocaleContext";
import { ThemeProvider } from "./hooks/app/useTheme";
import { installContextMenuGuard } from "./lib/ui/contextMenuGuard";
import "./styles/index.css";

installContextMenuGuard();

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
  } catch {
    document.documentElement.dataset.theme = "dark";
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
            fontFamily: "system-ui, -apple-system, BlinkMacSystemFont, \"Segoe UI\", sans-serif",
            color: "#e2e8f0",
            background: "#0f172a",
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
      <ThemeProvider>
        <LocaleProvider>
          <App />
        </LocaleProvider>
      </ThemeProvider>
    </RootErrorBoundary>
  </React.StrictMode>,
);
