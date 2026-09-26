/** 语言上下文：zh/en、localStorage 持久化与桌面菜单同步。 */
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  loadLocaleMessages,
  translate,
  zh,
  type Locale,
  type MessageCatalog,
  type MessageKey,
} from "./messages";

const STORAGE_KEY = "astro-locale";

function readStoredLocale(): Locale {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "zh" || v === "en") return v;
  } catch {
    // ignore
  }
  // 默认跟随浏览器语言
  try {
    const lang = navigator.language.toLowerCase();
    if (lang.startsWith("zh")) return "zh";
  } catch {
    // ignore
  }
  return "zh";
}

type I18nContextValue = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: (key: MessageKey, vars?: Record<string, string>) => string;
};

const I18nContext = createContext<I18nContextValue | null>(null);

/**
 * 启动就预取当前语言字典：中文常驻内存，英文走独立 chunk，
 * 首次渲染若还没到就用中文兜底，切过来后立即替换。
 */
function prefetchLocale(locale: Locale) {
  void loadLocaleMessages(locale).catch(() => {});
}

// 模块加载即开始取当前语言字典（英文是独立 chunk），避免首屏与切换时的等待。
if (typeof window !== "undefined") prefetchLocale(readStoredLocale());

export function LocaleProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>(() =>
    typeof window === "undefined" ? "zh" : readStoredLocale(),
  );
  const [catalog, setCatalog] = useState<MessageCatalog>(zh);

  useEffect(() => {
    let disposed = false;
    void loadLocaleMessages(locale)
      .then((next) => {
        if (!disposed) setCatalog(next);
      })
      .catch(() => {
        // 加载失败时保持当前字典
      });
    return () => {
      disposed = true;
    };
  }, [locale]);

  useEffect(() => {
    try {
      localStorage.setItem(STORAGE_KEY, locale);
    } catch {
      // ignore
    }
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
    // 同步 macOS / 桌面菜单栏语言
    void invoke("set_app_menu_locale", { locale }).catch(() => {
      // 非 Tauri 环境忽略
    });
  }, [locale]);

  const setLocale = useCallback((next: Locale) => {
    setLocaleState(next);
  }, []);

  const t = useCallback(
    (key: MessageKey, vars?: Record<string, string>) =>
      translate(catalog, key, vars),
    [catalog],
  );

  const value = useMemo(
    () => ({ locale, setLocale, t }),
    [locale, setLocale, t],
  );

  // 必须用 JSX Provider（与 ThemeProvider 一致），避免 createElement 与 jsx-runtime
  // 在打包后落到不同 React 副本时出现「useI18n must be used within LocaleProvider」
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n() {
  const ctx = useContext(I18nContext);
  if (!ctx) {
    throw new Error("useI18n must be used within LocaleProvider");
  }
  return ctx;
}
