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
  HomeSidebar,
  HomePage,
} = window;

const STORAGE_KEY = "astro-settings-prototype.v3";

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

function RailButton({ children, active = false, label, onClick }) {
  return <button type="button" className={`rail-button ${active ? "active" : ""}`} aria-label={label} title={label} onClick={onClick}>{children}</button>;
}

function NavRow({ item, active, onClick }) {
  return (
    <button type="button" className={`nav-row ${active ? "active" : ""}`} aria-current={active ? "page" : undefined} onClick={onClick}>
      <span className="nav-row-icon" aria-hidden>{item.icon}</span>
      <span className="nav-row-label">{item.label}</span>
    </button>
  );
}

function GenerateDialog({ open, onClose, onFinish, onRefine }) {
  const [prompt, setPrompt] = useState("宁静的未来山谷，柔和晨雾与紫蓝色天光，画面干净克制，适合作为桌面应用背景");
  const [style, setStyle] = useState("natural");
  const [generating, setGenerating] = useState(false);
  const [candidate, setCandidate] = useState(null);

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
      setCandidate(WALLPAPERS.iridescence);
    }, 1500);
  }

  return (
    <div className="backdrop" role="presentation" onMouseDown={(event) => { if (event.target === event.currentTarget && !generating) onClose(); }}>
      <section className="dialog" role="dialog" aria-modal="true" aria-labelledby="generate-title">
        {generating ? (
          <div className="progress-box" role="status">
            <div className="spinner" aria-hidden></div>
            <div><h2 id="generate-title">正在生成壁纸</h2><p>当前壁纸会一直保留，确认后才会替换。</p></div>
            <div className="progress-line"><i></i></div>
          </div>
        ) : candidate ? (
          <>
            <div className="dialog-head">
              <div className="card-icon" aria-hidden>✦</div>
              <div className="dialog-head-copy"><h2 id="generate-title">壁纸已生成</h2><p>确认后应用到界面，也可以继续调整。</p></div>
              <Button className="icon-button" variant="ghost" aria-label="关闭" onClick={onClose}>×</Button>
            </div>
            <div className="wallpaper-preview" style={{ backgroundImage: `url('${candidate.src}')`, backgroundSize: "cover", aspectRatio: "3 / 2" }} />
            <div className="detail-panel" style={{ marginTop: 12 }}><p>「{prompt.trim()}」</p></div>
            <div className="dialog-actions">
              <Button onClick={() => setCandidate(null)}>再生成一张</Button>
              <Button onClick={() => onRefine?.(candidate)}>在对话中微调</Button>
              <Button variant="primary" onClick={() => onFinish(candidate)}>应用为壁纸</Button>
            </div>
          </>
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
            <div className="detail-panel" style={{ marginTop: 12 }}><p>生成后先预览，确认再应用；生成失败时保留当前壁纸。</p></div>
            <div className="dialog-actions"><Button onClick={onClose}>取消</Button><Button variant="primary" disabled={!prompt.trim()} onClick={generate}>生成</Button></div>
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
  const initialHash = location.hash.slice(1);
  const initialTab = ALL_TABS.some((tab) => tab.id === initialHash) ? initialHash : "appearance";
  const [surface, setSurface] = useState(initialHash === "home" ? "home" : "settings");
  const [activeTab, setActiveTab] = useState(initialTab);
  const [query, setQuery] = useState("");
  const [toast, setToast] = useState("");
  const [generateOpen, setGenerateOpen] = useState(false);
  const [homeSessionKey, setHomeSessionKey] = useState(0);
  const [windowOffset, setWindowOffset] = useState({ x: 0, y: 0 });
  const [windowDragging, setWindowDragging] = useState(false);
  const windowDragRef = useRef(null);
  const appWindowRef = useRef(null);
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

  function beginWindowDrag(event) {
    if (event.button !== 0 || !appWindowRef.current) return;
    const rect = appWindowRef.current.getBoundingClientRect();
    windowDragRef.current = {
      pointerId: event.pointerId,
      startX: event.clientX,
      startY: event.clientY,
      left: rect.left,
      top: rect.top,
      originX: windowOffset.x,
      originY: windowOffset.y,
    };
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
    setWindowDragging(true);
  }

  function moveWindow(event) {
    const drag = windowDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId || !appWindowRef.current) return;
    const rect = appWindowRef.current.getBoundingClientRect();
    const nextLeft = Math.min(
      window.innerWidth - 140,
      Math.max(140 - rect.width, drag.left + event.clientX - drag.startX),
    );
    const nextTop = Math.min(
      window.innerHeight - 44,
      Math.max(0, drag.top + event.clientY - drag.startY),
    );
    setWindowOffset({
      x: drag.originX + nextLeft - drag.left,
      y: drag.originY + nextTop - drag.top,
    });
  }

  function endWindowDrag(event) {
    const drag = windowDragRef.current;
    if (!drag || drag.pointerId !== event.pointerId) return;
    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }
    windowDragRef.current = null;
    setWindowDragging(false);
  }

  useEffect(() => {
    const syncHash = () => {
      const hash = location.hash.slice(1);
      if (hash === "home") {
        setSurface("home");
      } else if (ALL_TABS.some((item) => item.id === hash)) {
        setActiveTab(hash);
        setSurface("settings");
      }
    };
    window.addEventListener("hashchange", syncHash);
    return () => window.removeEventListener("hashchange", syncHash);
  }, []);

  function patch(next) {
    setSettings((current) => ({ ...current, ...next }));
  }

  function navigate(id) {
    setSurface("settings");
    setActiveTab(id);
    history.replaceState(null, "", `#${id}`);
    contentRef.current?.scrollTo({ top: 0, behavior: "smooth" });
  }

  function openHome() {
    setSurface("home");
    history.replaceState(null, "", "#home");
  }

  function openSettings(id = activeTab) {
    setSurface("settings");
    navigate(id);
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
    <main className="stage" lang="zh" data-screen-label="Astro 桌面应用">
      <section
        ref={appWindowRef}
        className={`app-window ${windowDragging ? "is-dragging" : ""}`}
        data-preview-theme={settings.theme}
        data-surface={surface}
        aria-label="Astro 桌面应用原型"
        style={{
          "--window-offset-x": `${windowOffset.x}px`,
          "--window-offset-y": `${windowOffset.y}px`,
        }}
      >
        <div className="wallpaper-layer" style={wallpaperStyle}></div>
        <div className="wallpaper-shade" style={{ "--shade": settings.shade / 100 }}></div>
        <div className="shell">
          <header
            className="titlebar"
            onPointerDown={beginWindowDrag}
            onPointerMove={moveWindow}
            onPointerUp={endWindowDrag}
            onPointerCancel={endWindowDrag}
            onDoubleClick={() => setWindowOffset({ x: 0, y: 0 })}
          >
            <span className="traffic"><i></i><i></i><i></i></span>
            <span className="window-title">Astro · {surface === "home" ? "主页" : "设置"}</span>
            <span className="titlebar-status" data-dirty={dirty}>{dirty ? "设置草稿未保存" : surface === "home" ? "本地运行" : "已保存"}</span>
          </header>
          <aside className="rail" aria-label="主导航">
            <img className="brand" src="assets/astro-app-icon.png" alt="Astro" />
            <RailButton active={surface === "home"} label="对话" onClick={openHome}>⌁</RailButton><RailButton label="文件" onClick={() => notify("已打开文件空间") }>▱</RailButton><RailButton label="自动化" onClick={() => notify("已打开自动化") }>↻</RailButton>
            <div style={{ flex: 1 }}></div><RailButton active={surface === "settings"} label="设置" onClick={() => openSettings()}>⚙</RailButton>
          </aside>
          {surface === "settings" ? <aside className="settings-nav" aria-label="设置导航">
            <div className="settings-heading-row"><h2 className="settings-heading">设置</h2><span className="settings-count">{ALL_TABS.length} 个页面</span></div>
            <div className="search-wrap"><span className="search-mark" aria-hidden>⌕</span><input ref={searchRef} className="settings-search" placeholder="搜索设置  ⌘K" aria-label="搜索设置" value={query} onChange={(event) => setQuery(event.target.value)} />{query ? <button className="search-clear" onClick={() => setQuery("")} aria-label="清空搜索">×</button> : null}</div>
            {filteredGroups.map((group) => <div className="nav-group" key={group.id}><div className="nav-label">{group.label}</div>{group.items.map((item) => <NavRow key={item.id} item={item} active={activeTab === item.id} onClick={() => navigate(item.id)} />)}</div>)}
            {filteredGroups.length === 0 ? <div className="nav-empty">没有匹配的设置<br />试试“模型”或“外观”</div> : null}
          </aside> : <HomeSidebar notify={notify} onNewChat={() => { setHomeSessionKey((value) => value + 1); notify("已创建新会话"); }} />}
          {surface === "settings" ? <section className="content" ref={contentRef}>{pages[activeTab]}</section> : <HomePage key={homeSessionKey} notify={notify} />}
        </div>
        {dirty && surface === "settings" ? <div className="save-bar" role="status" aria-live="polite" aria-atomic="true"><div className="save-copy"><strong>你有未保存的更改</strong><small>可以继续切换页面，草稿不会丢失。</small></div><Button variant="ghost" onClick={reset}>撤销</Button><Button variant="primary" onClick={save}>保存设置</Button></div> : null}
        <GenerateDialog
          open={generateOpen}
          onClose={() => setGenerateOpen(false)}
          onFinish={(wallpaper) => { patch({ wallpaper, backgroundMode: "wallpaper" }); setGenerateOpen(false); notify("AI 壁纸已生成，保存后应用"); }}
          onRefine={(wallpaper) => { patch({ wallpaper, backgroundMode: "wallpaper" }); setGenerateOpen(false); setSurface("home"); notify("已在对话里继续微调这张壁纸"); }}
        />
        {toast ? <div className="toast" role="status"><span className="toast-mark">✓</span><span>{toast}</span></div> : null}
      </section>
    </main>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
