const { useEffect: useEffectHome, useMemo: useMemoHome, useRef: useRefHome, useState: useStateHome } = React;
const { HOME_CARDS, HOME_SESSIONS } = window;
const { Button, Tag } = window;

function HomeSidebar({ notify, onNewChat }) {
  const [query, setQuery] = useStateHome("");
  const [projectOpen, setProjectOpen] = useStateHome(true);
  const filtered = HOME_SESSIONS.filter((session) => session.title.toLowerCase().includes(query.toLowerCase()));
  return (
    <aside className="home-sidebar" aria-label="项目与会话">
      <div className="home-sidebar-head">
        <div><span className="home-sidebar-kicker">WORKSPACE</span><h2>Astro</h2></div>
        <button type="button" className="home-sidebar-icon" aria-label="新建会话" onClick={onNewChat}>＋</button>
      </div>
      <div className="search-wrap home-search"><span className="search-mark" aria-hidden>⌕</span><input className="settings-search" placeholder="搜索会话" value={query} onChange={(event) => setQuery(event.target.value)} />{query ? <button className="search-clear" aria-label="清空搜索" onClick={() => setQuery("")}>×</button> : null}</div>
      <button type="button" className="home-project" aria-expanded={projectOpen} onClick={() => setProjectOpen((value) => !value)}>
        <span className="home-project-mark">A</span><span className="home-project-copy"><strong>Astro</strong><small>Rust · Tauri · React</small></span><span aria-hidden>{projectOpen ? "⌄" : "›"}</span>
      </button>
      {projectOpen ? <div className="home-project-actions"><button onClick={() => notify("已打开项目文件") }>▧ 文件</button><button onClick={() => notify("已打开 Agent Tree") }>◇ Agent Tree</button><button onClick={() => notify("已打开项目终端") }>&gt;_ 终端</button></div> : null}
      <div className="home-sidebar-label"><span>最近会话</span><button onClick={() => notify("会话筛选器已打开") }>≡</button></div>
      <div className="home-session-list">
        {filtered.map((session) => <button type="button" key={session.id} className={`home-session ${session.active ? "active" : ""}`} onClick={() => notify(`已打开：${session.title}`)}><span className="home-session-dot" aria-hidden></span><span><strong>{session.title}</strong><small>{session.meta}</small></span><i aria-hidden>···</i></button>)}
        {filtered.length === 0 ? <div className="nav-empty">没有匹配的会话</div> : null}
      </div>
      <div className="home-sidebar-footer"><span className="home-agent-avatar">A</span><span><strong>root</strong><small>GPT-5.6 · priority</small></span><button aria-label="Agent 设置" onClick={() => notify("已打开 Agent 设置") }>⚙</button></div>
    </aside>
  );
}

function HomeCard({ card, onPick, duplicate = false }) {
  return <button type="button" tabIndex={duplicate ? -1 : undefined} className="home-capability-card" data-tone={card.tone} onClick={() => onPick(card)}><span className="home-card-icon" aria-hidden>{card.glyph}</span><span className="home-card-copy"><strong>{card.title}</strong><small>{card.description}</small></span><span className="home-card-arrow" aria-hidden>→</span></button>;
}

function HomeMarquee({ cards, reverse, paused, onPick }) {
  return <div className={`home-marquee ${reverse ? "reverse" : ""} ${paused ? "paused" : ""}`}><div className="home-marquee-track"><div className="home-marquee-group">{cards.map((card) => <HomeCard card={card} onPick={onPick} key={card.id} />)}</div><div className="home-marquee-group" aria-hidden>{cards.map((card) => <HomeCard card={card} onPick={onPick} duplicate key={`${card.id}-copy`} />)}</div></div></div>;
}

function HomeComposer({ input, setInput, notify }) {
  const [plusOpen, setPlusOpen] = useStateHome(false);
  const [policyOpen, setPolicyOpen] = useStateHome(false);
  const [mode, setMode] = useStateHome("Agent");
  const [approval, setApproval] = useStateHome("询问批准");
  const [contexts, setContexts] = useStateHome([]);
  const [listening, setListening] = useStateHome(false);
  const inputRef = useRefHome(null);

  useEffectHome(() => { inputRef.current?.focus(); }, []);

  function addContext(kind, name) {
    setContexts((items) => items.some((item) => item.name === name) ? items : [...items, { kind, name }]);
    setPlusOpen(false);
    window.requestAnimationFrame(() => inputRef.current?.focus());
  }

  function submit(event) {
    event.preventDefault();
    if (!input.trim()) return;
    notify("已创建新会话并提交任务");
    setInput("");
    setContexts([]);
  }

  return (
    <form className="home-composer" onSubmit={submit}>
      {contexts.length ? <div className="home-context-strip">{contexts.map((item) => <span className="home-context-token" key={item.name}><small>{item.kind}</small><strong>{item.name}</strong><button type="button" aria-label={`移除 ${item.name}`} onClick={() => setContexts((items) => items.filter((token) => token.name !== item.name))}>×</button></span>)}</div> : null}
      <textarea ref={inputRef} rows="2" value={input} onChange={(event) => setInput(event.target.value)} placeholder="可以描述任务或提问任何问题" onKeyDown={(event) => { if (event.key === "Enter" && !event.shiftKey) { event.preventDefault(); event.currentTarget.form?.requestSubmit(); } }}></textarea>
      <div className="home-composer-bar">
        <div className="home-composer-left">
          <div className="home-popover-anchor"><button type="button" className={`home-policy-pill ${policyOpen ? "open" : ""}`} onClick={() => { setPolicyOpen((value) => !value); setPlusOpen(false); }}><span>◇</span>{mode}<i>·</i><span>◈</span>{approval}<b>⌄</b></button>{policyOpen ? <div className="home-popover home-policy-menu"><small>交互模式</small>{["Agent", "Plan", "Chat"].map((item) => <button type="button" className={mode === item ? "selected" : ""} key={item} onClick={() => { setMode(item); setPolicyOpen(false); }}>{item}<span>{item === "Agent" ? "自主执行工具与任务" : item === "Plan" ? "先规划，再实施" : "仅对话与解释"}</span></button>)}<small>权限策略</small>{["询问批准", "自动审阅", "完全访问"].map((item) => <button type="button" className={approval === item ? "selected" : ""} key={item} onClick={() => { setApproval(item); setPolicyOpen(false); }}>{item}</button>)}</div> : null}</div>
          <div className="home-popover-anchor"><button type="button" className={`home-icon-button ${plusOpen ? "open" : ""}`} aria-label="添加上下文" onClick={() => { setPlusOpen((value) => !value); setPolicyOpen(false); }}>＋</button>{plusOpen ? <div className="home-popover home-plus-menu"><button type="button" onClick={() => addContext("文件", "README.md")}><span>▧</span><b>添加文件</b><small>从工作区选择</small></button><button type="button" onClick={() => addContext("Skill", "code-review")}><span>✧</span><b>选择 Skill</b><small>按需加载专业流程</small></button><button type="button" onClick={() => addContext("MCP", "browser")}><span>◎</span><b>选择 MCP</b><small>使用已连接服务</small></button></div> : null}</div>
        </div>
        <div className="home-composer-right"><button type="button" className={`home-icon-button ${listening ? "active" : ""}`} aria-label="语音对话" onClick={() => { setListening((value) => !value); notify(listening ? "已停止语音对话" : "正在启动 Realtime 语音"); }}>♫</button><button type="button" className="home-context-ring" title="上下文使用 18%" aria-label="上下文使用 18%"><span></span></button><button type="submit" className="home-send" disabled={!input.trim()} aria-label="发送">↑</button></div>
      </div>
    </form>
  );
}

function HomePage({ notify }) {
  const [input, setInput] = useStateHome("");
  const [paused, setPaused] = useStateHome(false);
  const [logoPulse, setLogoPulse] = useStateHome(0);
  const rows = useMemoHome(() => [HOME_CARDS.slice(0, 6), HOME_CARDS.slice(6, 12)], []);
  function pickCard(card) {
    setInput(card.prompt);
    notify(`已加载模板：${card.title}`);
  }
  return (
    <section className="home-page" data-screen-label="Astro 主页">
      <div className="home-hero" aria-hidden><span></span><span></span><i></i><i></i><i></i></div>
      <div className="home-welcome-copy">
        <button type="button" key={logoPulse} className={`home-logo ${logoPulse ? "pulse" : ""}`} onClick={() => setLogoPulse((value) => value + 1)} aria-label="Hi，我是 Astro"><img src="assets/astro-app-icon.png" alt="" /></button>
        <div className="home-wordmark"><strong>Astro</strong><span>Agent</span></div>
        <h1>Hi, 我是 <em>Astro</em></h1>
        <p>随时随地，帮你高效干活</p>
      </div>
      <div className="home-capability-region"><button type="button" className="home-pause" aria-label={paused ? "继续滚动" : "暂停滚动"} aria-pressed={paused} onClick={() => setPaused((value) => !value)}>{paused ? "▶" : "Ⅱ"}</button><HomeMarquee cards={rows[0]} reverse paused={paused} onPick={pickCard} /><HomeMarquee cards={rows[1]} paused={paused} onPick={pickCard} /></div>
      <div className="home-composer-dock"><HomeComposer input={input} setInput={setInput} notify={notify} /></div>
    </section>
  );
}

Object.assign(window, { HomeSidebar, HomePage });
