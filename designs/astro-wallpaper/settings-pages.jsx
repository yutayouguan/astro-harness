const { useMemo: useMemoPages, useRef: useRefPages, useState: useStatePages } = React;
const {
  Card,
  PageHeader,
  Toggle,
  Segmented,
  SettingRow,
  SelectControl,
  SliderControl,
  Tag,
  Stat,
  Button,
} = window;
const { WALLPAPERS, PROVIDERS, TOOLS, MODELS, LOG_ROWS } = window;

function PreferencesPage({ tab, settings, patch, notify }) {
  return (
    <div className="page" data-screen-label="偏好设置">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <div className="two-column">
          <Card icon="文" title="语言与地区" subtitle="应用界面与系统通知使用同一语言。">
            <div className="setting-list">
              <SettingRow title="界面语言" description="切换后立即生效。">
                <SelectControl
                  label="界面语言"
                  value={settings.language}
                  options={[
                    { value: "zh-CN", label: "简体中文" },
                    { value: "en-US", label: "English" },
                  ]}
                  onChange={(language) => patch({ language })}
                />
              </SettingRow>
              <SettingRow title="日期与时间" description="跟随 macOS 区域设置。">
                <Tag tone="success">自动</Tag>
              </SettingRow>
            </div>
          </Card>

          <Card icon="↻" title="启动与后台" subtitle="决定 Astro 何时启动以及关闭窗口后的行为。">
            <div className="setting-list">
              <SettingRow title="登录时启动" description="登录系统后在后台启动 Astro。">
                <Toggle label="登录时启动" checked={settings.launchAtLogin} onChange={(launchAtLogin) => patch({ launchAtLogin })} />
              </SettingRow>
              <SettingRow title="关闭后保持运行" description="隐藏窗口，自动化与任务继续运行。">
                <Toggle label="关闭后保持运行" checked={settings.keepRunning} onChange={(keepRunning) => patch({ keepRunning })} />
              </SettingRow>
            </div>
          </Card>
        </div>

        <Card icon="↑" title="软件更新" subtitle="更新会在后台下载，安装前始终会询问。" action={<Tag tone="success">已是最新</Tag>}>
          <div className="setting-list">
            <SettingRow title="更新通道" description="稳定版适合日常使用，预览版会更早获得新能力。">
              <Segmented
                label="更新通道"
                value={settings.updateChannel}
                options={[{ value: "stable", label: "稳定版" }, { value: "preview", label: "预览版" }]}
                onChange={(updateChannel) => patch({ updateChannel })}
              />
            </SettingRow>
            <SettingRow title="当前版本" description="Astro 0.1.0 · Universal Apple Silicon / Intel">
              <Button onClick={() => notify("已检查更新，当前已是最新版本") }>检查更新</Button>
            </SettingRow>
          </div>
        </Card>
      </div>
    </div>
  );
}

function AppearancePage({ tab, settings, patch, notify, onGenerate }) {
  const uploadRef = useRefPages(null);
  const accents = ["#3b82f6", "#6d5ce8", "#0fa67a", "#e36558", "#c38420"];

  function uploadFile(event) {
    const file = event.target.files?.[0];
    if (!file) return;
    patch({
      backgroundMode: "wallpaper",
      wallpaper: { id: `upload-${Date.now()}`, name: file.name, src: URL.createObjectURL(file) },
    });
    notify("已载入本地图片，保存后应用");
  }

  return (
    <div className="page" data-screen-label="外观">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <div className="two-column">
          <Card icon="◒" title="主题与材质" subtitle="控制明暗模式与玻璃强度。">
            <div className="setting-list">
              <SettingRow title="界面主题" description="系统模式会跟随 macOS 自动切换。">
                <Segmented
                  label="界面主题"
                  value={settings.theme}
                  options={[{ value: "light", label: "浅色" }, { value: "system", label: "系统" }, { value: "dark", label: "深色" }]}
                  onChange={(theme) => patch({ theme })}
                />
              </SettingRow>
              <SettingRow title="玻璃强度" description="影响卡片的透明、柔化与景深。">
                <Segmented
                  label="玻璃强度"
                  value={settings.glass}
                  options={[{ value: "solid", label: "实心" }, { value: "soft", label: "柔和" }, { value: "liquid", label: "通透" }]}
                  onChange={(glass) => patch({ glass })}
                />
              </SettingRow>
            </div>
          </Card>

          <Card icon="◉" title="强调色" subtitle="与壁纸分离，确保按钮和状态始终清晰。">
            <div className="setting-list">
              <SettingRow title="色彩策略" description="灵动模式会根据当前背景提取色彩。">
                <Segmented
                  label="色彩策略"
                  value={settings.colorStyle}
                  options={[{ value: "unified", label: "统一" }, { value: "dynamic", label: "灵动" }, { value: "colorful", label: "多彩" }]}
                  onChange={(colorStyle) => patch({ colorStyle })}
                />
              </SettingRow>
              <SettingRow title="基准色" description="用于焦点、选中态和关键操作。">
                <div className="accent-row">
                  {accents.map((accent) => (
                    <button key={accent} className={`accent-dot ${settings.accent === accent ? "active" : ""}`} style={{ background: accent }} aria-label={accent} onClick={() => patch({ accent })}></button>
                  ))}
                </div>
              </SettingRow>
            </div>
          </Card>
        </div>

        <Card icon="▣" title="全局背景" subtitle="背景只位于 Shell 底层，内容表面会自动保持可读性。">
          <div className="choice-grid" style={{ gridTemplateColumns: "repeat(2, minmax(0, 1fr))", marginBottom: 13 }}>
            {[
              ["color", "◒", "氛围配色", "使用应用生成的多层色彩背景"],
              ["wallpaper", "▧", "图片壁纸", "上传图片，或让 AI 生成一张"],
            ].map(([value, icon, title, description]) => (
              <button key={value} className={`choice-card ${settings.backgroundMode === value ? "active" : ""}`} onClick={() => patch({ backgroundMode: value })}>
                <strong>{icon} &nbsp;{title}</strong><small>{description}</small>
              </button>
            ))}
          </div>

          {settings.backgroundMode === "wallpaper" ? (
            <div className="wallpaper-editor">
              <div className="wallpaper-preview" style={{ backgroundImage: `url('${settings.wallpaper.src}')`, backgroundSize: settings.fit === "stretch" ? "100% 100%" : settings.fit, "--preview-shade": settings.shade / 100 }}>
                <div className="mini-shell"><div className="mini-side"></div><div className="mini-main"><div className="mini-line big"></div><div className="mini-line"></div><div className="mini-bubble"></div></div></div>
                <span className="preview-caption">当前：{settings.wallpaper.name}</span>
              </div>
              <div className="wallpaper-controls">
                <div className="action-row">
                  <Button onClick={() => uploadRef.current?.click()}>上传图片</Button>
                  <Button variant="primary" onClick={onGenerate}>AI 生成</Button>
                  <input ref={uploadRef} type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={uploadFile} />
                </div>
                <SettingRow title="填充方式">
                  <Segmented label="填充方式" value={settings.fit} options={[{ value: "cover", label: "填满" }, { value: "contain", label: "适应" }, { value: "stretch", label: "拉伸" }]} onChange={(fit) => patch({ fit })} />
                </SettingRow>
                <SettingRow title="内容保护"><SliderControl label="内容保护" value={settings.shade} min={0} max={55} suffix="%" onChange={(shade) => patch({ shade })} /></SettingRow>
                <SettingRow title="柔化背景"><SliderControl label="柔化背景" value={settings.blur} min={0} max={12} suffix="px" onChange={(blur) => patch({ blur })} /></SettingRow>
                <div className="recent-grid">
                  {Object.values(WALLPAPERS).map((item) => (
                    <button key={item.id} className={`recent-item ${settings.wallpaper.id === item.id ? "active" : ""}`} style={{ backgroundImage: `url('${item.src}')` }} aria-label={item.name} onClick={() => patch({ wallpaper: item })}></button>
                  ))}
                  <button className="recent-item" aria-label="上传新壁纸" onClick={() => uploadRef.current?.click()} style={{ background: "rgba(255,255,255,.42)", color: "var(--tone)", fontSize: 20 }}>＋</button>
                </div>
              </div>
            </div>
          ) : (
            <div className="detail-panel">已切换为氛围配色。强调色和玻璃强度保持独立。</div>
          )}
        </Card>

        <Card icon="∿" title="动效与图标" subtitle="调整界面反馈的节奏与图标重量。">
          <div className="setting-list">
            <SettingRow title="图标动效" description="决定标签切换和操作反馈的弹性。"><Segmented label="图标动效" value={settings.motion} options={[{ value: "smooth", label: "柔和" }, { value: "snappy", label: "利落" }, { value: "bouncy", label: "弹性" }]} onChange={(motion) => patch({ motion })} /></SettingRow>
            <SettingRow title="图标线宽" description="同步应用主导视觉密度。"><Segmented label="图标线宽" value={settings.iconWeight} options={[{ value: "light", label: "纤细" }, { value: "regular", label: "标准" }, { value: "bold", label: "醒目" }]} onChange={(iconWeight) => patch({ iconWeight })} /></SettingRow>
          </div>
        </Card>
      </div>
    </div>
  );
}

function ConversationPage({ tab, settings, patch }) {
  const toggles = [
    ["showReasoning", "思考过程", "显示模型的思考摘要。"],
    ["showTools", "工具调用", "在回答时间线中显示执行步骤。"],
    ["showHooks", "Hooks", "显示运行前后的扩展事件。"],
    ["showTimestamps", "时间戳", "在每条消息上显示发送时间。"],
  ];
  return (
    <div className="page" data-screen-label="对话">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon="≡" title="回答编排" subtitle="选择长回答与工具过程的组织方式。">
          <div className="choice-grid">
            {[
              ["timeline", "时间线", "按执行顺序呈现思考、工具与回答。"],
              ["grouped", "分组", "将连续的工具活动收纳为一个阶段。"],
              ["answer", "答案优先", "默认收起运行细节，聚焦最终结果。"],
            ].map(([value, title, description]) => (
              <button key={value} className={`choice-card ${settings.answerLayout === value ? "active" : ""}`} onClick={() => patch({ answerLayout: value })}>
                <strong>{title}</strong><small>{description}</small>
              </button>
            ))}
          </div>
        </Card>
        <div className="two-column">
          <Card icon="▶" title="回答密度" subtitle="调整默认详细程度，不影响你在对话中的明确要求。">
            <Segmented label="回答密度" value={settings.verbosity} options={[{ value: "concise", label: "简洁" }, { value: "balanced", label: "平衡" }, { value: "detailed", label: "详细" }]} onChange={(verbosity) => patch({ verbosity })} />
          </Card>
          <Card icon="◉" title="即时预览" subtitle="当前组合会如何呈现在对话中。">
            <div className="detail-panel">
              <div className="inline-tags"><Tag tone="tone">{settings.answerLayout === "timeline" ? "时间线" : settings.answerLayout === "grouped" ? "分组" : "答案优先"}</Tag><Tag>{settings.verbosity === "balanced" ? "平衡" : settings.verbosity === "concise" ? "简洁" : "详细"}</Tag></div>
              <p style={{ marginTop: 9 }}>思考摘要 → 工具活动 → 最终回答，异常状态会自动展开。</p>
            </div>
          </Card>
        </div>
        <Card icon="▤" title="过程可见性" subtitle="只显示对你有帮助的运行信息。">
          <div className="setting-list">
            {toggles.map(([key, title, description]) => (
              <SettingRow key={key} title={title} description={description}><Toggle label={title} checked={settings[key]} onChange={(value) => patch({ [key]: value })} /></SettingRow>
            ))}
          </div>
        </Card>
      </div>
    </div>
  );
}

function TerminalPage({ tab, settings, patch, notify }) {
  return (
    <div className="page" data-screen-label="终端">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon=">_" title="执行模式" subtitle="对手动终端与 Agent 命令使用不同会话边界。">
          <div className="choice-grid">
            {[
              ["isolated", "独立会话", "用户终端与 AI 终端互不干扰，推荐。"],
              ["shared", "共享会话", "两端共享当前工作目录和 shell 状态。"],
              ["external", "外部终端", "将手动操作交给系统默认终端。"],
            ].map(([value, title, description]) => (
              <button key={value} className={`choice-card ${settings.terminalMode === value ? "active" : ""}`} onClick={() => patch({ terminalMode: value })}><strong>{title}</strong><small>{description}</small></button>
            ))}
          </div>
        </Card>
        <div className="two-column">
          <Card icon="Aa" title="显示" subtitle="使用 Nerd Font 时可完整显示终端图标。">
            <div className="setting-list">
              <SettingRow title="字体"><SelectControl label="终端字体" value={settings.terminalFont} options={[{ value: "JetBrains Mono", label: "JetBrains Mono" }, { value: "SF Mono", label: "SF Mono" }, { value: "Menlo", label: "Menlo" }]} onChange={(terminalFont) => patch({ terminalFont })} /></SettingRow>
              <SettingRow title="字号"><SliderControl label="终端字号" value={settings.terminalFontSize} min={11} max={20} suffix="px" onChange={(terminalFontSize) => patch({ terminalFontSize })} /></SettingRow>
            </div>
            <div className="code-field" style={{ marginTop: 10, fontFamily: settings.terminalFont, fontSize: settings.terminalFontSize }}>$ cargo check<br /><span style={{ color: "var(--success)" }}>✓ Finished dev profile</span></div>
          </Card>
          <Card icon="⇲" title="会话行为" subtitle="调整历史容量与关闭保护。">
            <div className="setting-list">
              <SettingRow title="回滚行数" description="更大的数值会占用更多内存。"><SelectControl label="回滚行数" value={String(settings.terminalScrollback)} options={[{ value: "2000", label: "2,000" }, { value: "5000", label: "5,000" }, { value: "10000", label: "10,000" }]} onChange={(value) => patch({ terminalScrollback: Number(value) })} /></SettingRow>
              <SettingRow title="关闭运行中会话时确认"><Toggle label="关闭确认" checked={settings.terminalConfirmClose} onChange={(terminalConfirmClose) => patch({ terminalConfirmClose })} /></SettingRow>
              <SettingRow title="清理已结束会话" description="仅清理终端输出，不删除 Agent 历史。"><Button onClick={() => notify("已清理 3 个已结束终端会话") }>立即清理</Button></SettingRow>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

function ContextPage({ tab, settings, patch }) {
  const presets = [
    ["quality", "质量优先", "更少压缩，保留更多原始工具结果。", 88],
    ["balanced", "平衡", "长任务与成本之间的推荐设置。", 78],
    ["economy", "精简", "更早压缩，适合高频、重复任务。", 64],
  ];
  return (
    <div className="page" data-screen-label="自动压缩">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon="◔" title="上下文策略" subtitle="压缩会保留原始历史，只改变下一次模型看到的视图。" action={<Tag tone="success">安全可恢复</Tag>}>
          <div className="choice-grid">
            {presets.map(([value, title, description, threshold]) => (
              <button key={value} className={`choice-card ${settings.contextMode === value ? "active" : ""}`} onClick={() => patch({ contextMode: value, contextThreshold: threshold })}><strong>{title}</strong><small>{description}</small></button>
            ))}
          </div>
        </Card>
        <div className="split-column">
          <Card icon="⊕" title="触发与预算" subtitle="在达到模型上下文的指定比例后开始压缩。">
            <div className="setting-list">
              <SettingRow title="软阈值" description="超过后优先剪枝大型工具结果。"><SliderControl label="压缩阈值" value={settings.contextThreshold} min={50} max={92} suffix="%" onChange={(contextThreshold) => patch({ contextThreshold, contextMode: "custom" })} /></SettingRow>
              <SettingRow title="压缩模型" description="自动会沿用主模型的辅助任务路由。"><SelectControl label="压缩模型" value={settings.compactionModel} options={[{ value: "auto", label: "自动" }, { value: "gpt-5.6-mini", label: "GPT-5.6 Mini" }, { value: "gemini-flash", label: "Gemini Flash" }]} onChange={(compactionModel) => patch({ compactionModel })} /></SettingRow>
            </div>
          </Card>
          <Card icon="∿" title="预估效果" subtitle="以 400K 上下文模型为例。">
            <div style={{ display: "grid", gap: 12 }}>
              <div className="stat"><div className="stat-label">首次压缩约在</div><div className="stat-value">{Math.round(400 * settings.contextThreshold / 100)}K</div><div className="stat-trend">可用 token</div></div>
              <div className="detail-panel"><p>执行顺序：工具结果剪枝 → 辅助模型摘要 → head/tail 回落。</p></div>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

function ProvidersPage({ tab, notify }) {
  const [selected, setSelected] = useStatePages("openai");
  const [connected, setConnected] = useStatePages(() => Object.fromEntries(PROVIDERS.map((item) => [item.id, item.status === "connected"])));
  const provider = PROVIDERS.find((item) => item.id === selected) ?? PROVIDERS[0];
  return (
    <div className="page" data-screen-label="模型服务">
      <PageHeader tab={tab} action={<Button variant="primary" onClick={() => notify("已打开新建 Provider 向导")}>＋ 添加服务</Button>} />
      <div className="split-column">
        <Card icon="◎" title="已配置服务" subtitle="选择一项查看连接和模型路由。">
          <div className="provider-list">
            {PROVIDERS.map((item) => (
              <button key={item.id} className={`provider-row ${selected === item.id ? "selected" : ""}`} onClick={() => setSelected(item.id)} style={{ cursor: "pointer", textAlign: "left", color: "inherit" }}>
                <span className="provider-logo">{item.mark}</span>
                <span className="row-copy"><span className="row-title">{item.name}{connected[item.id] ? <Tag tone="success">已连接</Tag> : <Tag tone="warning">待配置</Tag>}</span><span className="row-sub">{item.detail}</span></span>
                <span aria-hidden>›</span>
              </button>
            ))}
          </div>
        </Card>
        <Card icon={provider.mark} title={provider.name} subtitle="密钥仅保存在系统安全凭据库。">
          <div className="setting-list">
            <SettingRow title="API 状态" description={connected[provider.id] ? "认证有效，最近检查于刚刚。" : "添加凭据后启用此服务。"}><Tag tone={connected[provider.id] ? "success" : "warning"}>{connected[provider.id] ? "正常" : "未配置"}</Tag></SettingRow>
            <SettingRow title="Base URL"><span className="code-field">api.{provider.id}.com/v1</span></SettingRow>
            <SettingRow title="流式协议" description="Agent 路由始终使用原生 Responses。"><Tag tone="tone">Responses</Tag></SettingRow>
          </div>
          <div className="action-row" style={{ marginTop: 12 }}>
            <Button onClick={() => notify(`${provider.name} 连接测试成功`)}>测试连接</Button>
            <Button variant={connected[provider.id] ? "danger" : "primary"} onClick={() => setConnected((current) => ({ ...current, [provider.id]: !current[provider.id] }))}>{connected[provider.id] ? "移除凭据" : "添加凭据"}</Button>
          </div>
        </Card>
      </div>
    </div>
  );
}

function ToolsPage({ tab, notify }) {
  const [scope, setScope] = useStatePages("builtin");
  const [toolMode, setToolMode] = useStatePages("auto");
  const [query, setQuery] = useStatePages("");
  const [enabled, setEnabled] = useStatePages(() => Object.fromEntries(TOOLS.map((item) => [item.id, item.enabled])));
  const skillRows = [
    { id: "code-review", mark: "CR", name: "code-review", detail: "审查代码变更，优先发现缺陷与回归", scope: "项目", enabled: true },
    { id: "gemini-api", mark: "GI", name: "gemini-interactions-api", detail: "Gemini Interactions API 开发指引", scope: "项目", enabled: true },
    { id: "lark", mark: "LK", name: "lark-doc", detail: "飞书文档读取、创建与编辑", scope: "全局", enabled: false },
  ];
  const mcpRows = [
    { id: "filesystem", mark: "FS", name: "Workspace Files", detail: "stdio · 14 个工具", scope: "项目", enabled: true },
    { id: "browser-mcp", mark: "BR", name: "Browser Control", detail: "streamable HTTP · 9 个工具", scope: "全局", enabled: true },
  ];
  const rows = scope === "builtin" ? TOOLS : scope === "skills" ? skillRows : mcpRows;
  const filtered = rows.filter((item) => `${item.name} ${item.detail}`.toLowerCase().includes(query.toLowerCase()));
  return (
    <div className="page" data-screen-label="工具与技能">
      <PageHeader tab={tab} action={<Button variant="primary" onClick={() => notify("已打开扩展市场") }>添加扩展</Button>} />
      <Card icon="✧" title="能力目录" subtitle="禁用的能力不会暴露给模型。">
        <div className="toolbar">
          <Segmented label="能力类型" value={scope} options={[{ value: "builtin", label: "内置工具" }, { value: "skills", label: "Skills" }, { value: "mcp", label: "MCP" }]} onChange={setScope} />
          <span className="toolbar-spacer"></span>
          <input className="text-input" style={{ maxWidth: 220 }} placeholder="搜索能力" value={query} onChange={(event) => setQuery(event.target.value)} />
        </div>
        <div className="tool-list">
          {filtered.map((item) => (
            <div className="tool-row" key={item.id}>
              <span className="tool-logo">{item.mark}</span>
              <span className="row-copy"><span className="row-title">{item.name}<Tag>{item.scope}</Tag></span><span className="row-sub">{item.detail}</span></span>
              <Toggle label={`启用 ${item.name}`} checked={enabled[item.id] ?? item.enabled} onChange={(value) => setEnabled((current) => ({ ...current, [item.id]: value }))} />
            </div>
          ))}
          {filtered.length === 0 ? <div className="nav-empty">没有匹配的能力</div> : null}
        </div>
      </Card>
      <Card compact icon="◇" title="暴露策略" subtitle="Direct 直接暴露工具；Code Mode 只暴露 exec / wait；Auto 根据模型能力选择。">
        <Segmented label="工具模式" value={toolMode} options={[{ value: "auto", label: "Auto" }, { value: "direct", label: "Direct" }, { value: "code", label: "Code Mode" }]} onChange={(value) => { setToolMode(value); notify("工具模式会在下一轮对话生效"); }} />
      </Card>
    </div>
  );
}

function MemoryPage({ tab, settings, patch, notify }) {
  const [pending, setPending] = useStatePages(2);
  return (
    <div className="page" data-screen-label="记忆">
      <PageHeader tab={tab} action={<Button onClick={() => notify("记忆快照已刷新") }>刷新快照</Button>} />
      <div className="page-stack">
        <Card className="memory-hero" icon="◌" title="记忆状态" subtitle="当前工作区的长期记忆已加载，用户画像与每日摘要保持同步。">
          <div className="memory-score">78%</div>
        </Card>
        <div className="two-column">
          <Card icon="MD" title="记忆文件" subtitle="直接编辑被 Agent 在下一轮读取的内容。">
            <div className="memory-files">
              <div className="file-card"><div className="file-card-head"><span className="file-card-name">MEMORY.md</span><Tag tone="success">已同步</Tag></div><p>8.4 KB · 34 条可复用上下文</p><Button onClick={() => notify("已打开 MEMORY.md 编辑器") }>打开编辑</Button></div>
              <div className="file-card"><div className="file-card-head"><span className="file-card-name">USER.md</span><Tag tone="success">已同步</Tag></div><p>2.1 KB · 用户偏好与沟通方式</p><Button onClick={() => notify("已打开 USER.md 编辑器") }>打开编辑</Button></div>
            </div>
          </Card>
          <Card icon="✓" title="待审核记忆" subtitle="Agent 提议的长期记忆在写入前可由你确认。" action={<Tag tone={pending ? "warning" : "success"}>{pending ? `${pending} 条待处理` : "已清空"}</Tag>}>
            {pending ? (
              <div className="detail-panel"><h3>偏好使用简洁的中文结果</h3><p>来源：最近的前端格式化任务</p><div className="action-row" style={{ marginTop: 10 }}><Button variant="ghost" onClick={() => setPending((value) => Math.max(0, value - 1))}>忽略</Button><Button variant="primary" onClick={() => { setPending((value) => Math.max(0, value - 1)); notify("记忆已批准并写入"); }}>批准</Button></div></div>
            ) : <div className="nav-empty">暂无待审核记忆</div>}
          </Card>
        </div>
        <Card icon="⚙" title="自动化策略" subtitle="控制记忆何时加载、刷新与写入。">
          <div className="setting-list">
            <SettingRow title="启用长期记忆" description="在新对话中加载项目记忆与用户画像。"><Toggle label="启用长期记忆" checked={settings.memoryEnabled} onChange={(memoryEnabled) => patch({ memoryEnabled })} /></SettingRow>
            <SettingRow title="文件更新时刷新" description="检测到 MEMORY.md 变更后自动更新快照。"><Toggle label="自动刷新记忆" checked={settings.memoryAutoRefresh} onChange={(memoryAutoRefresh) => patch({ memoryAutoRefresh })} /></SettingRow>
            <SettingRow title="写入前需要批准" description="适合对长期记忆变更要求严格的工作区。"><Toggle label="记忆写入审批" checked={settings.memoryApproval} onChange={(memoryApproval) => patch({ memoryApproval })} /></SettingRow>
          </div>
        </Card>
      </div>
    </div>
  );
}

function BrowserPage({ tab, settings, patch, notify }) {
  const [sites, setSites] = useStatePages([
    { host: "github.com", access: "允许登录状态", tone: "success" },
    { host: "localhost", access: "本地调试", tone: "tone" },
  ]);
  return (
    <div className="page" data-screen-label="浏览器">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <div className="two-column">
          <Card icon="◎" title="运行方式" subtitle="可见浏览器适合协作，后台模式适合自动化。">
            <div className="setting-list">
              <SettingRow title="默认运行时"><Segmented label="浏览器运行时" value={settings.browserRuntime} options={[{ value: "visible", label: "可见" }, { value: "managed", label: "受管" }, { value: "system", label: "系统" }]} onChange={(browserRuntime) => patch({ browserRuntime })} /></SettingRow>
              <SettingRow title="视口预设"><SelectControl label="视口预设" value={settings.viewport} options={[{ value: "1280 × 800", label: "1280 × 800" }, { value: "1440 × 900", label: "1440 × 900" }, { value: "1920 × 1080", label: "1920 × 1080" }]} onChange={(viewport) => patch({ viewport })} /></SettingRow>
            </div>
          </Card>
          <Card icon="⇱" title="启动与下载" subtitle="为新建标签和下载文件指定默认行为。">
            <div className="setting-list">
              <SettingRow title="新标签页"><SelectControl label="新标签页" value={settings.browserStartup} options={[{ value: "blank", label: "空白页" }, { value: "last", label: "上次页面" }, { value: "workspace", label: "工作区主页" }]} onChange={(browserStartup) => patch({ browserStartup })} /></SettingRow>
              <SettingRow title="下载文件"><SelectControl label="下载文件" value={settings.browserDownloads} options={[{ value: "ask", label: "每次询问" }, { value: "workspace", label: "工作区 Downloads" }, { value: "system", label: "系统 Downloads" }]} onChange={(browserDownloads) => patch({ browserDownloads })} /></SettingRow>
            </div>
          </Card>
        </div>
        <Card icon="◈" title="站点权限" subtitle="按域名管理登录状态、剪贴板和下载权限。" action={<Button onClick={() => setSites((current) => [...current, { host: "example.com", access: "每次询问", tone: "warning" }])}>＋ 添加站点</Button>}>
          <div className="permission-list">
            {sites.map((site, index) => (
              <div className="permission-row" key={`${site.host}-${index}`}><span className="provider-logo">◎</span><span className="row-copy"><span className="row-title">{site.host}</span><span className="row-sub">{site.access}</span></span><Tag tone={site.tone}>{site.tone === "success" ? "已允许" : site.tone === "warning" ? "询问" : "本地"}</Tag><Button variant="ghost" onClick={() => setSites((current) => current.filter((_, itemIndex) => itemIndex !== index))}>移除</Button></div>
            ))}
          </div>
        </Card>
        <Card compact icon="▣" title="浏览数据" subtitle="Cookie、缓存和标签状态只保存在本机。" action={<Button variant="danger" onClick={() => notify("浏览器本地数据已清理") }>清理数据</Button>} />
      </div>
    </div>
  );
}

function ModelsPage({ tab, notify }) {
  const [query, setQuery] = useStatePages("");
  const [capability, setCapability] = useStatePages("all");
  const [selected, setSelected] = useStatePages("gpt-5.6");
  const filtered = MODELS.filter((model) => {
    const queryMatch = `${model.name} ${model.provider}`.toLowerCase().includes(query.toLowerCase());
    const capMatch = capability === "all" || model.caps.some((cap) => cap.toLowerCase().includes(capability));
    return queryMatch && capMatch;
  });
  const model = MODELS.find((item) => item.id === selected) ?? MODELS[0];
  return (
    <div className="page" data-screen-label="模型市场">
      <PageHeader tab={tab} action={<Button onClick={() => notify("模型目录已刷新") }>刷新目录</Button>} />
      <Card icon="⬡" title="可用模型" subtitle="能力来自 Provider 元数据，具体路由仍由对话或任务决定。">
        <div className="toolbar">
          <input className="text-input" placeholder="搜索模型或供应商" value={query} onChange={(event) => setQuery(event.target.value)} />
          <Segmented label="能力过滤" value={capability} options={[{ value: "all", label: "全部" }, { value: "tools", label: "Tools" }, { value: "vision", label: "Vision" }, { value: "reasoning", label: "Reasoning" }]} onChange={setCapability} />
        </div>
        <div className="model-grid">
          {filtered.map((item) => (
            <button key={item.id} className={`model-card ${selected === item.id ? "selected" : ""}`} onClick={() => setSelected(item.id)}>
              <span className="model-card-provider">{item.provider}</span><span className="model-card-title">{item.name}</span>
              <span className="model-card-meta">{item.caps.map((cap) => <Tag key={cap} tone={cap === "Tools" ? "tone" : ""}>{cap}</Tag>)}</span>
              <span className="row-sub">上下文 {item.context}</span>
            </button>
          ))}
        </div>
      </Card>
      <Card compact icon="◎" title={model.name} subtitle={`${model.provider} · ${model.caps.join(" · ")}`} action={<Button variant="primary" onClick={() => notify(`${model.name} 已设为当前会话模型`)}>用于当前对话</Button>}>
        <div className="inline-tags"><Tag>上下文 {model.context}</Tag><Tag tone="success">可用</Tag><Tag tone="tone">Responses</Tag></div>
      </Card>
    </div>
  );
}

function InsightsPage({ tab }) {
  const [period, setPeriod] = useStatePages("month");
  const datasets = {
    week: [28, 44, 39, 72, 56, 84, 68, 92, 75, 61, 88, 96],
    month: [42, 58, 47, 75, 64, 88, 72, 95, 82, 70, 91, 78],
    quarter: [34, 51, 67, 59, 73, 82, 76, 90, 84, 94, 88, 99],
  };
  return (
    <div className="page" data-screen-label="数据洞察">
      <PageHeader tab={tab} action={<Segmented label="统计周期" value={period} options={[{ value: "week", label: "7 天" }, { value: "month", label: "30 天" }, { value: "quarter", label: "90 天" }]} onChange={setPeriod} />} />
      <div className="page-stack">
        <div className="stats-grid"><Stat label="模型调用" value={period === "week" ? "286" : period === "month" ? "1,248" : "3,905"} trend="↑ 12.4%" /><Stat label="Token" value={period === "week" ? "4.2M" : "18.6M"} trend="↑ 8.1%" /><Stat label="估算成本" value={period === "week" ? "$18.42" : "$76.30"} trend="定价覆盖 94%" /><Stat label="工具成功率" value="97.8%" trend="↑ 1.2%" /></div>
        <div className="split-column">
          <Card icon="∿" title="用量趋势" subtitle="按日聚合的模型 token 用量。"><div className="chart">{datasets[period].map((height, index) => <div key={index} className="chart-bar" style={{ height: `${height}%` }} title={`${height}k tokens`}></div>)}</div></Card>
          <Card icon="≡" title="成本排行" subtitle="按已定价用量汇总。"><div className="rank-list">{[["GPT-5.6", 88, "$31.24"], ["Gemini 3.5 Pro", 61, "$19.80"], ["DeepSeek V4", 39, "$8.16"], ["GPT Image 2", 24, "$5.40"]].map(([name, width, value], index) => <div className="rank-row" key={name}><span className="rank-index">{index + 1}</span><span><span>{name}</span><span className="rank-meter"><i style={{ width: `${width}%` }}></i></span></span><strong>{value}</strong></div>)}</div></Card>
        </div>
        <Card icon="◈" title="能力使用" subtitle="工具、Skill、MCP 与定时任务的活跃度。">
          <div className="stats-grid"><Stat label="工具调用" value="864" trend="Terminal 38%" /><Stat label="Skills" value="142" trend="code-review 最常用" /><Stat label="MCP" value="76" trend="5 个服务活跃" /><Stat label="Cron" value="31" trend="30 次成功" /></div>
        </Card>
      </div>
    </div>
  );
}

function DiagnosticsPage({ tab, notify }) {
  const [level, setLevel] = useStatePages("all");
  const [query, setQuery] = useStatePages("");
  const filtered = LOG_ROWS.filter((row) => (level === "all" || row.level === level) && `${row.source} ${row.message}`.toLowerCase().includes(query.toLowerCase()));
  return (
    <div className="page" data-screen-label="诊断">
      <PageHeader tab={tab} action={<Button onClick={() => notify("诊断信息已刷新") }>刷新</Button>} />
      <div className="page-stack">
        <div className="stats-grid"><Stat label="Backend" value="正常" trend="127.0.0.1 · 内嵌" /><Stat label="Provider" value="4 / 4" trend="平均延迟 842ms" /><Stat label="MCP" value="4 / 5" trend="1 个正在重试" /><Stat label="数据库" value="WAL" trend="schema v22" /></div>
        <Card icon="△" title="运行日志" subtitle="日志按日滚动，默认只展示最近事件。" action={<Button onClick={() => notify("已复制诊断摘要") }>复制摘要</Button>}>
          <div className="toolbar"><Segmented label="日志级别" value={level} options={[{ value: "all", label: "全部" }, { value: "info", label: "Info" }, { value: "warn", label: "Warn" }, { value: "error", label: "Error" }]} onChange={setLevel} /><span className="toolbar-spacer"></span><input className="text-input" style={{ maxWidth: 240 }} placeholder="搜索日志" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
          <div className="log-list">{filtered.map((row, index) => <div className="log-row" key={`${row.time}-${index}`}><span className="log-time">{row.time}</span><span className={`log-level ${row.level}`}>{row.level.toUpperCase()}</span><span><strong>{row.source}</strong> · {row.message}</span></div>)}</div>
        </Card>
        <Card compact icon="⇱" title="导出诊断包" subtitle="包含近期日志、版本和脱敏后的运行配置，不包含 API 密钥。" action={<Button variant="primary" onClick={() => notify("诊断包已导出到下载目录") }>导出诊断包</Button>} />
      </div>
    </div>
  );
}

function AboutPage({ tab, notify }) {
  const [checking, setChecking] = useStatePages(false);
  function checkUpdates() {
    setChecking(true);
    window.setTimeout(() => { setChecking(false); notify("当前已是最新版本") }, 900);
  }
  return (
    <div className="page" data-screen-label="关于 Astro">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card className="about-hero">
          <div><img className="about-logo" src="assets/astro-app-icon.png" alt="Astro" /><h2>Astro</h2><p>本地 AI 桌面工作站</p><div className="about-meta"><Tag tone="tone">0.1.0</Tag><Tag>Universal</Tag><Tag tone="success">Stable</Tag></div></div>
        </Card>
        <div className="two-column">
          <Card icon="↑" title="更新" subtitle="当前使用稳定更新通道。"><div className="setting-list"><SettingRow title="版本 0.1.0" description="Build 2026.09.06 · aarch64-apple-darwin"><Button disabled={checking} onClick={checkUpdates}>{checking ? "正在检查…" : "检查更新"}</Button></SettingRow></div></Card>
          <Card icon="©" title="项目信息" subtitle="Astro 及其配置与数据均保存在本机。"><div className="setting-list"><SettingRow title="开源软件许可"><Button onClick={() => notify("已打开开源许可列表") }>查看</Button></SettingRow><SettingRow title="数据目录"><span className="code-field">~/.astro</span></SettingRow></div></Card>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, {
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
});
