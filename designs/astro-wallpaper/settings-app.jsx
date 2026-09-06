const { useEffect, useMemo, useRef, useState } = React;
const { SETTINGS_GROUPS, ALL_TABS, WALLPAPERS, INITIAL_SETTINGS } = window;
const {
  Button,
  Segmented,
  PreferencesPage,
  AppearancePage,
  ConversationPage,
  TerminalPage,
  ContextPage,
  ProvidersPage,
  ToolsPage,
  MemoryPage,
  BrowserPage,
  ModelsPage,
  InsightsPage,
  DiagnosticsPage,
  AboutPage,
} = window;

const STORAGE_KEY = "astro-settings-prototype.v2";

function loadSettings() {
  try {
    const stored = JSON.parse(localStorage.getItem(STORAGE_KEY) || "null");
    if (!stored || typeof stored !== "object") return { ...INITIAL_SETTINGS };
    const merged = { ...INITIAL_SETTINGS };
    for (const key of Object.keys(INITIAL_SETTINGS)) {
      if (stored[key] !== undefined) merged[key] = stored[key];
    }
    merged.wallpaper = stored.wallpaper?.src?.startsWith("blob:") ? WALLPAPERS.valley : (stored.wallpaper ?? WALLPAPERS.valley);
    return merged;
  } catch {
    return { ...INITIAL_SETTINGS };
  }
}

function RailButton({ children, active = false, label }) {
  return <button type="button" className={`rail-button ${active ? "active" : ""}`} aria-label={label} title={label}>{children}</button>;
}

function NavRow({ item, active, onClick }) {
  return (
    <button type="button" className={`nav-row ${active ? "active" : ""}`} aria-current={active ? "page" : undefined} onClick={onClick}>
      <span className="nav-row-icon" aria-hidden>{item.icon}</span>
      <span className="nav-row-label">{item.label}</span>
    </button>
  );
}

function GenerateDialog({ open, onClose, onFinish }) {
  const [prompt, setPrompt] = useState("宁静的未来山谷，柔和晨雾与紫蓝色天光，画面干净克制，适合作为桌面应用背景");
  const [style, setStyle] = useState("natural");
  const [generating, setGenerating] = useState(false);

  useEffect(() => {
    if (!open) return undefined;
    const onKeyDown = (event) => {
      if (event.key === "Escape" && !generating) onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, generating, onClose]);

  if (!open) return null;

  function generate() {
    if (!prompt.trim() || generating) return;
    setGenerating(true);
    window.setTimeout(() => {
      setGenerating(false);
      onFinish(WALLPAPERS.iridescence);
    }, 1500);
  }

  return (
    <div className="backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !generating) onClose(); }}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="generate-title">
        {generating ? (
          <div className="progress-box" role="status">
            <div className="spinner" aria-hidden></div>
            <div><h2 id="generate-title">正在生成壁纸</h2><p>当前壁纸会一直保留，生成成功后再自动替换。</p></div>
            <div className="progress-line"><i></i></div>
          </div>
        ) : (
          <>
            <div className="dialog-head">
              <div className="card-icon" aria-hidden>✦</div>
              <div className="dialog-head-copy"><h2 id="generate-title">AI 生成壁纸</h2><p>使用“模型服务”中选定的图片模型，按当前窗口比例生成。</p></div>
              <Button className="icon-button" variant="ghost" aria-label="关闭" onClick={onClose}>×</Button>
            </div>
            <label className="dialog-label" htmlFor="wallpaper-prompt">描述你想要的画面</label>
            <textarea id="wallpaper-prompt" className="text-area" value={prompt} onChange={(event) => setPrompt(event.target.value)} autoFocus></textarea>
            <div style={{ marginTop: 10 }}>
              <Segmented
                label="壁纸风格"
                value={style}
                options={[{ value: "natural", label: "自然摄影" }, { value: "abstract", label: "抽象流体" }, { value: "minimal", label: "极简插画" }, { value: "cinematic", label: "电影感" }]}
                onChange={setStyle}
              />
            </div>
            <div className="detail-panel" style={{ marginTop: 12 }}><p>生成成功后自动应用；生成失败时保留当前壁纸。</p></div>
            <div className="dialog-actions"><Button onClick={onClose}>取消</Button><Button variant="primary" disabled={!prompt.trim()} onClick={generate}>生成并应用</Button></div>
          </>
        )}
      </section>
    </div>
  );
}

function App() {
  const initial = useMemo(() => loadSettings(), []);
  const [saved, setSaved] = useState(initial);
  const [settings, setSettings] = useState(initial);
  const initialTab = ALL_TABS.some((tab) => tab.id === location.hash.slice(1)) ? location.hash.slice(1) : "appearance";
  const [activeTab, setActiveTab] = useState(initialTab);
  const [query, setQuery] = useState("");
  const [toast, setToast] = useState("");
  const [generateOpen, setGenerateOpen] = useState(false);
  const searchRef = useRef(null);
  const contentRef = useRef(null);
  const tab = ALL_TABS.find((item) => item.id === activeTab) ?? ALL_TABS[0];
  const dirty = JSON.stringify(settings) !== JSON.stringify(saved);

  useEffect(() => {
    document.documentElement.style.setProperty("--tone", settings.accent);
  }, [settings.accent]);

  useEffect(() => {
    if (!toast) return undefined;
    const timer = window.setTimeout(() => setToast(""), 2500);
    return () => window.clearTimeout(timer);
  }, [toast]);

  useEffect(() => {
    const onKeyDown = (event) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        searchRef.current?.focus();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  function patch(next) {
    setSettings((current) => ({ ...current, ...next }));
  }

  function navigate(id) {
    setActiveTab(id);
    history.replaceState(null, "", `#${id}`);
    contentRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }

  function notify(message) {
    setToast("");
    window.requestAnimationFrame(() => setToast(message));
  }

  function save() {
    setSaved(settings);
    try { localStorage.setItem(STORAGE_KEY, JSON.stringify(settings)); } catch {}
    notify("设置已保存");
  }

  function reset() {
    setSettings(saved);
    notify("已撤销未保存的更改");
  }

  const filteredGroups = SETTINGS_GROUPS.map((group) => ({
    ...group,
    items: group.items.filter((item) => `${item.label} ${item.description}`.toLowerCase().includes(query.trim().toLowerCase())),
  })).filter((group) => group.items.length > 0);

  const wallpaperStyle = {
    backgroundImage: settings.backgroundMode === "wallpaper"
      ? `url('${settings.wallpaper.src}')`
      : "radial-gradient(circle at 18% 14%, #bfcfff, transparent 34%), radial-gradient(circle at 82% 85%, #ccbdf8, transparent 37%), linear-gradient(140deg, #e9eef9, #eae5f5)",
    backgroundSize: settings.fit === "stretch" ? "100% 100%" : settings.fit,
    filter: `blur(${settings.blur}px) saturate(1.06)`,
    opacity: settings.backgroundMode === "wallpaper" ? 1 : 0.92,
  };

  const commonProps = { tab, settings, patch, notify };
  const pages = {
    preferences: <PreferencesPage {...commonProps} />,
    appearance: <AppearancePage {...commonProps} onGenerate={() => setGenerateOpen(true)} />,
    conversation: <ConversationPage {...commonProps} />,
    terminal: <TerminalPage {...commonProps} />,
    context: <ContextPage {...commonProps} />,
    providers: <ProvidersPage {...commonProps} />,
    tools: <ToolsPage {...commonProps} />,
    memory: <MemoryPage {...commonProps} />,
    browser: <BrowserPage {...commonProps} />,
    models: <ModelsPage {...commonProps} />,
    insights: <InsightsPage {...commonProps} />,
    diagnostics: <DiagnosticsPage {...commonProps} />,
    about: <AboutPage {...commonProps} />,
  };

  return (
    <main className="stage" lang="zh" data-screen-label="Astro 设置中心">
      <section className="app-window" data-preview-theme={settings.theme} aria-label="Astro 桌面应用设置原型">
        <div className="wallpaper-layer" style={wallpaperStyle}></div>
        <div className="wallpaper-shade" style={{ "--shade": settings.shade / 100 }}></div>
        <div className="shell">
          <header className="titlebar">
            <span className="traffic"><i></i><i></i><i></i></span>
            <span className="window-title">Astro · 设置</span>
            <span className="titlebar-status" data-dirty={dirty}>{dirty ? "有未保存更改" : "已保存"}</span>
          </header>
          <aside className="rail" aria-label="主导航">
            <img className="brand" src="assets/astro-app-icon.png" alt="Astro" />
            <RailButton label="对话">⌁</RailButton><RailButton label="文件">▱</RailButton><RailButton label="自动化">↻</RailButton>
            <div style={{ flex: 1 }}></div><RailButton active label="设置">⚙</RailButton>
          </aside>
          <aside className="settings-nav" aria-label="设置导航">
            <div className="settings-heading-row"><h2 className="settings-heading">设置</h2><span className="settings-count">{ALL_TABS.length} 个页面</span></div>
            <div className="search-wrap"><span className="search-mark" aria-hidden>⌕</span><input ref={searchRef} className="settings-search" placeholder="搜索设置  ⌘K" aria-label="搜索设置" value={query} onChange={(event) => setQuery(event.target.value)} />{query ? <button className="search-clear" onClick={() => setQuery("")} aria-label="清空搜索">×</button> : null}</div>
            {filteredGroups.map((group) => <div className="nav-group" key={group.id}><div className="nav-label">{group.label}</div>{group.items.map((item) => <NavRow key={item.id} item={item} active={activeTab === item.id} onClick={() => navigate(item.id)} />)}</div>)}
            {filteredGroups.length === 0 ? <div className="nav-empty">没有匹配的设置<br />试试“模型”或“外观”</div> : null}
          </aside>
          <section className="content" ref={contentRef}>{pages[activeTab]}</section>
        </div>
        {dirty ? <div className="save-bar" role="status" aria-live="polite" aria-atomic="true"><div className="save-copy"><strong>你有未保存的更改</strong><small>可以继续切换页面，草稿不会丢失。</small></div><Button variant="ghost" onClick={reset}>撤销</Button><Button variant="primary" onClick={save}>保存设置</Button></div> : null}
        <GenerateDialog open={generateOpen} onClose={() => setGenerateOpen(false)} onFinish={(wallpaper) => { patch({ wallpaper, backgroundMode: "wallpaper" }); setGenerateOpen(false); notify("AI 壁纸已生成，保存后应用"); }} />
        {toast ? <div className="toast" role="status"><span className="toast-mark">✓</span><span>{toast}</span></div> : null}
      </section>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
