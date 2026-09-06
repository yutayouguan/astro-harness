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
    <div className="page" data-screen-label="基础设置">
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

          <Card icon="↻" title="系统集成" subtitle="管理 Astro 与 macOS 登录项和系统托盘的集成。">
            <div className="setting-list">
              <SettingRow title="登录时启动" description="登录系统后在后台启动 Astro。">
                <Toggle label="登录时启动" checked={settings.launchAtLogin} onChange={(launchAtLogin) => patch({ launchAtLogin })} />
              </SettingRow>
              <SettingRow title="关闭窗口" description="隐藏到系统托盘，后端与定时任务继续运行。">
                <Tag tone="tone">托盘常驻</Tag>
              </SettingRow>
            </div>
          </Card>
        </div>
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
                <Segmented label="玻璃强度" value={settings.glass} options={[{ value: "minimal", label: "最简" }, { value: "normal", label: "标准" }, { value: "rich", label: "丰富" }, { value: "liquid-soft", label: "柔液" }, { value: "liquid", label: "液态" }]} onChange={(glass) => patch({ glass })} />
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
              {settings.colorStyle === "unified" ? <SettingRow title="统一配色" description="为全局 Shell 选择一组稳定渐变。"><div className="accent-row">{accents.map((accent, index) => <button key={accent} className={`accent-dot ${settings.accent === accent ? "active" : ""}`} style={{ background: `linear-gradient(135deg, ${accent}, color-mix(in srgb, ${accent} 40%, white))` }} aria-label={`配色 ${index + 1}`} onClick={() => patch({ accent, gradientPreset: `preset-${index}` })}></button>)}</div></SettingRow> : null}
              {settings.colorStyle === "dynamic" ? <SettingRow title="灵动配色" description="每次重组会生成一组新的协调色彩。"><Button onClick={() => { const accent = accents[Math.floor(Math.random() * accents.length)]; patch({ accent }); notify("已重新生成灵动配色"); }}>⚄ 重新生成</Button></SettingRow> : null}
              {settings.colorStyle === "colorful" ? <SettingRow title="多彩策略" description="根据页面语义为不同能力分配独立强调色。"><Tag tone="tone">自动分配</Tag></SettingRow> : null}
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

          <div className="wallpaper-editor">
              <div className="wallpaper-preview" style={{ backgroundImage: settings.backgroundMode === "wallpaper" ? `url('${settings.wallpaper.src}')` : `radial-gradient(circle at 24% 18%, color-mix(in srgb, ${settings.accent} 42%, white), transparent 54%), linear-gradient(145deg, #bfd8e9, #ead2c8)`, backgroundSize: settings.backgroundMode === "wallpaper" && settings.fit === "stretch" ? "100% 100%" : settings.backgroundMode === "wallpaper" ? settings.fit : "cover", "--preview-shade": settings.shade / 100 }}>
                <div className="mini-shell"><div className="mini-side"></div><div className="mini-main"><div className="mini-line big"></div><div className="mini-line"></div><div className="mini-bubble"></div></div></div>
                <span className="preview-caption">当前：{settings.backgroundMode === "wallpaper" ? settings.wallpaper.name : "氛围配色"}</span>
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
        </Card>

        <Card icon="∿" title="动效与图标" subtitle="调整界面反馈的节奏与图标重量。">
          <div className="setting-list">
            <SettingRow title="图标动效" description="决定标签切换和操作反馈的弹性。"><Segmented label="图标动效" value={settings.motion} options={[{ value: "smooth", label: "柔和" }, { value: "snappy", label: "利落" }, { value: "bouncy", label: "弹性" }]} onChange={(motion) => patch({ motion })} /></SettingRow>
            <SettingRow title="图标线宽" description="同步应用主导视觉密度。"><Segmented label="图标线宽" value={settings.iconWeight} options={[{ value: "light", label: "纤细" }, { value: "regular", label: "标准" }, { value: "bold", label: "醒目" }]} onChange={(iconWeight) => patch({ iconWeight })} /></SettingRow>
          </div>
        </Card>
        <Card icon="A" title="应用图标" subtitle="切换 Dock、Finder 与系统托盘中的 Astro 图标。">
          <div className="app-icon-options" role="radiogroup" aria-label="应用图标">
            {[
              ["blue", "蓝色", "assets/app-icon-blue.png"], ["deep_blue", "深蓝", "assets/app-icon-deep-blue.png"], ["black", "黑色", "assets/app-icon-black.png"], ["white", "白色", "assets/app-icon-white.png"], ["white_logo", "白底", "assets/app-icon-white-logo.png"],
            ].map(([value, label, src]) => <button key={value} role="radio" aria-checked={settings.appIcon === value} className={`app-icon-choice ${settings.appIcon === value ? "active" : ""}`} onClick={() => patch({ appIcon: value })}><span className="app-icon-swatch"><img src={src} alt="" /></span><span>{label}</span></button>)}
          </div>
          <p className="card-sub" style={{ marginTop: 11 }}>Finder 和 Dock 可能在数秒后刷新图标缓存。</p>
        </Card>
      </div>
    </div>
  );
}

function ConversationPage({ tab, settings, patch }) {
  const toggles = [
    ["showTools", "工具调用", "在回答时间线中显示执行步骤。"],
    ["showSkills", "Skills", "显示按需加载的 Skill 与工作流。"],
    ["showMcp", "MCP", "显示 MCP 工具调用和远程服务状态。"],
    ["showHooks", "Hooks", "显示运行前后的扩展事件。"],
    ["showMemory", "记忆更新", "显示记忆读取、刷新和写入活动。"],
    ["showStatus", "状态与阶段", "显示运行、重试和等待用户等阶段。"],
    ["showTimestamps", "时间戳", "在每条消息上显示发送时间。"],
  ];
  const setVerbosity = (verbosity) => {
    const preset = verbosity === "compact"
      ? { showTools: false, showSkills: false, showMcp: false, showHooks: false, showMemory: false, showStatus: false, showTimestamps: false }
      : verbosity === "detailed"
        ? { showTools: true, showSkills: true, showMcp: true, showHooks: true, showMemory: true, showStatus: true, showTimestamps: true }
        : { showTools: true, showSkills: true, showMcp: false, showHooks: true, showMemory: true, showStatus: true, showTimestamps: false };
    patch({ verbosity, ...preset });
  };
  return (
    <div className="page" data-screen-label="对话">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon="≡" title="回答编排" subtitle="选择长回答与工具过程的组织方式。">
          <div className="choice-grid" style={{ gridTemplateColumns: "repeat(2, minmax(0, 1fr))" }}>
            {[
              ["timeline", "时间线", "按执行顺序呈现思考、工具与回答。"],
              ["grouped", "分组", "将连续的工具活动收纳为一个阶段。"],
            ].map(([value, title, description]) => (
              <button key={value} className={`choice-card ${settings.answerLayout === value ? "active" : ""}`} onClick={() => patch({ answerLayout: value })}>
                <strong>{title}</strong><small>{description}</small>
              </button>
            ))}
          </div>
        </Card>
        <div className="two-column">
          <Card icon="▶" title="回答密度" subtitle="调整默认详细程度，不影响你在对话中的明确要求。">
            <Segmented label="回答密度" value={settings.verbosity} options={[{ value: "compact", label: "简洁" }, { value: "normal", label: "常规" }, { value: "detailed", label: "详细" }]} onChange={setVerbosity} />
          </Card>
          <Card icon="◉" title="即时预览" subtitle="当前组合会如何呈现在对话中。">
            <div className="detail-panel">
              <div className="inline-tags"><Tag tone="tone">{settings.answerLayout === "timeline" ? "时间线" : "分组"}</Tag><Tag>{settings.verbosity === "normal" ? "常规" : settings.verbosity === "compact" ? "简洁" : "详细"}</Tag></div>
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
  const resetTerminal = () => {
    patch({ terminalExecutionMode: "system", terminalFont: "MesloLGS NF", terminalFontSize: 13, terminalLineHeight: 1.25, terminalScrollback: 5000, terminalCursorStyle: "bar", terminalCursorBlink: true });
    notify("已恢复终端默认设置");
  };
  return (
    <div className="page" data-screen-label="终端">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon=">_" title="执行模式" subtitle="决定手动终端在系统环境还是当前项目上下文中启动。">
          <div className="choice-grid" style={{ gridTemplateColumns: "repeat(2, minmax(0, 1fr))" }}>
            {[
              ["system", "系统环境", "从用户主目录启动，使用完整的系统 Shell 环境。"],
              ["project", "项目环境", "从当前工作区启动，贴合项目命令与路径。"],
            ].map(([value, title, description]) => (
              <button key={value} className={`choice-card ${settings.terminalExecutionMode === value ? "active" : ""}`} onClick={() => patch({ terminalExecutionMode: value })}><strong>{title}</strong><small>{description}</small></button>
            ))}
          </div>
          <p className="card-sub" style={{ marginTop: 10 }}>{settings.terminalExecutionMode === "system" ? "手动终端不继承项目目录，Agent 执行仍保持自己的工作区。" : "新建手动终端会自动进入当前项目根目录。"}</p>
        </Card>
        <div className="two-column">
          <Card icon="Aa" title="显示" subtitle="使用 Nerd Font 时可完整显示终端图标。">
            <div className="setting-list">
              <SettingRow title="字体预设"><SelectControl label="终端字体预设" value={settings.terminalFont} options={[{ value: "MesloLGS NF", label: "MesloLGS NF" }, { value: "Hack Nerd Font Mono", label: "Hack Nerd Font Mono" }, { value: "JetBrainsMono Nerd Font", label: "JetBrainsMono Nerd Font" }, { value: "SF Mono", label: "macOS Mono" }]} onChange={(terminalFont) => patch({ terminalFont })} /></SettingRow>
              <SettingRow title="字体家族" description="可直接输入已安装字体的 CSS font-family。"><input className="text-input" value={settings.terminalFont} onChange={(event) => patch({ terminalFont: event.target.value })} /></SettingRow>
              <SettingRow title="字号"><SliderControl label="终端字号" value={settings.terminalFontSize} min={9} max={28} suffix="px" onChange={(terminalFontSize) => patch({ terminalFontSize })} /></SettingRow>
              <SettingRow title="行高"><SliderControl label="终端行高" value={settings.terminalLineHeight} min={1} max={2} step={0.05} onChange={(terminalLineHeight) => patch({ terminalLineHeight })} /></SettingRow>
            </div>
            <div className="code-field" style={{ marginTop: 10, fontFamily: settings.terminalFont, fontSize: settings.terminalFontSize, lineHeight: settings.terminalLineHeight }}>$ cargo check<br /><span style={{ color: "var(--success)" }}>✓ Finished dev profile</span></div>
          </Card>
          <Card icon="⇲" title="光标与历史" subtitle="调整光标外观和终端回滚容量。" action={<Button variant="ghost" onClick={resetTerminal}>恢复默认</Button>}>
            <div className="setting-list">
              <SettingRow title="光标样式"><Segmented label="光标样式" value={settings.terminalCursorStyle} options={[{ value: "bar", label: "细线" }, { value: "block", label: "方块" }, { value: "underline", label: "下划线" }]} onChange={(terminalCursorStyle) => patch({ terminalCursorStyle })} /></SettingRow>
              <SettingRow title="光标闪烁" description="让当前输入位置更容易被找到。"><Toggle label="光标闪烁" checked={settings.terminalCursorBlink} onChange={(terminalCursorBlink) => patch({ terminalCursorBlink })} /></SettingRow>
              <SettingRow title="回滚行数" description="可设置 500 至 50,000 行。"><SelectControl label="回滚行数" value={String(settings.terminalScrollback)} options={[{ value: "2000", label: "2,000" }, { value: "5000", label: "5,000" }, { value: "10000", label: "10,000" }, { value: "50000", label: "50,000" }]} onChange={(value) => patch({ terminalScrollback: Number(value) })} /></SettingRow>
            </div>
          </Card>
        </div>
      </div>
    </div>
  );
}

function ContextPage({ tab, settings, patch }) {
  const stages = [
    ["soft", "软阶段", "剪枝大型工具结果", "contextSoftRatio", 50, settings.contextMediumRatio - 3],
    ["medium", "中阶段", "使用辅助模型摘要", "contextMediumRatio", settings.contextSoftRatio + 3, settings.contextHardRatio - 3],
    ["hard", "硬阶段", "使用 head / tail 安全回落", "contextHardRatio", settings.contextMediumRatio + 3, 97],
  ];
  return (
    <div className="page" data-screen-label="自动压缩">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <Card icon="◔" title="上下文压缩" subtitle="压缩会保留原始历史，只改变下一次模型看到的视图。" action={<Toggle label="启用上下文压缩" checked={settings.contextEnabled} onChange={(contextEnabled) => patch({ contextEnabled })} />}>
          <div className={`context-stages ${settings.contextEnabled ? "" : "is-disabled"}`}>
            {stages.map(([id, title, description, key, min, max]) => <label className="context-stage" data-stage={id} key={id}><span className="context-stage-copy"><strong>{title}</strong><small>{description}</small></span><SliderControl label={title} value={settings[key]} min={min} max={max} suffix="%" onChange={(value) => patch({ [key]: value })} /></label>)}
          </div>
        </Card>
        <Card icon="⚙" title="保护与预算" subtitle="高级字符预算在此折叠，默认只暴露最常调整的保护参数。">
          <details className="advanced-disclosure"><summary>显示触发与预算</summary><div className="setting-list"><SettingRow title="工具结果上限" description="超出后优先压缩最早的工具结果。"><SliderControl label="工具结果上限" value={settings.contextToolResultsLimit} min={10} max={100} onChange={(contextToolResultsLimit) => patch({ contextToolResultsLimit })} /></SettingRow><SettingRow title="保护最近消息" description="压缩时不改写最近的对话消息。"><SliderControl label="保护最近消息" value={settings.contextProtectLastN} min={2} max={20} onChange={(contextProtectLastN) => patch({ contextProtectLastN })} /></SettingRow><SettingRow title="保留尾部气泡" description="保留最近完整工具来回。"><SliderControl label="保留尾部气泡" value={settings.contextKeepTailBubbles} min={1} max={12} onChange={(contextKeepTailBubbles) => patch({ contextKeepTailBubbles })} /></SettingRow></div></details>
        </Card>
      </div>
    </div>
  );
}

function ProvidersPage({ tab, notify }) {
  const [surface, setSurface] = useStatePages("providers");
  const [detailTab, setDetailTab] = useStatePages("chat");
  const [selected, setSelected] = useStatePages("openai");
  const [connected, setConnected] = useStatePages(() => Object.fromEntries(PROVIDERS.map((item) => [item.id, item.status === "connected"])));
  const provider = PROVIDERS.find((item) => item.id === selected) ?? PROVIDERS[0];
  return (
    <div className="page" data-screen-label="模型服务">
      <PageHeader tab={tab} action={<div className="toolbar" style={{ margin: 0 }}><Segmented label="模型服务主页签" value={surface} options={[{ value: "providers", label: "服务商" }, { value: "auxiliary", label: "辅助模型" }]} onChange={setSurface} />{surface === "providers" ? <Button variant="primary" onClick={() => notify("已打开新建 Provider 向导")}>＋ 添加服务</Button> : null}</div>} />
      {surface === "providers" ? <div className="split-column">
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
          <div className="toolbar provider-detail-tabs"><Segmented label="Provider 详情" value={detailTab} options={[{ value: "chat", label: "对话" }, { value: "models", label: "模型" }, { value: "media", label: "媒体" }, { value: "voice", label: "语音" }, { value: "embedding", label: "Embedding" }]} onChange={setDetailTab} /></div>
          {detailTab === "chat" ? <div className="setting-list"><SettingRow title="API 状态" description={connected[provider.id] ? "认证有效，最近检查于刚刚。" : "添加凭据后启用此服务。"}><Tag tone={connected[provider.id] ? "success" : "warning"}>{connected[provider.id] ? "正常" : "未配置"}</Tag></SettingRow><SettingRow title="Base URL"><span className="code-field">api.{provider.id}.com/v1</span></SettingRow><SettingRow title="流式协议" description="Agent 路由始终使用原生 Responses。"><Tag tone="tone">Responses</Tag></SettingRow></div> : null}
          {detailTab === "models" ? <div className="setting-list"><SettingRow title="当前模型" description="用于普通对话和 Agent 任务。"><SelectControl label="当前模型" value="gpt-5.6" options={[{ value: "gpt-5.6", label: "GPT-5.6" }, { value: "gpt-5.6-mini", label: "GPT-5.6 Mini" }]} onChange={() => notify("对话模型已更新")} /></SettingRow><SettingRow title="上下文窗口"><Tag>400K</Tag></SettingRow><SettingRow title="备用模型" description="仅在首个流式 chunk 前失败时切换。"><Button>＋ 添加 fallback</Button></SettingRow></div> : null}
          {detailTab === "media" ? <div className="setting-list"><SettingRow title="图像模型"><SelectControl label="图像模型" value="gpt-image-2" options={[{ value: "gpt-image-2", label: "GPT Image 2" }, { value: "none", label: "未配置" }]} onChange={() => notify("默认图像模型已更新")} /></SettingRow><SettingRow title="视频模型"><SelectControl label="视频模型" value="none" options={[{ value: "none", label: "未配置" }, { value: "veo", label: "Veo 3" }]} onChange={() => notify("默认视频模型已更新")} /></SettingRow><SettingRow title="音乐模型"><Tag>未配置</Tag></SettingRow></div> : null}
          {detailTab === "voice" ? <div className="setting-list"><SettingRow title="TTS 模型"><SelectControl label="TTS 模型" value="gpt-4o-mini-tts" options={[{ value: "gpt-4o-mini-tts", label: "GPT-4o Mini TTS" }, { value: "none", label: "未配置" }]} onChange={() => notify("TTS 模型已更新")} /></SettingRow><SettingRow title="ASR 模型"><SelectControl label="ASR 模型" value="whisper-1" options={[{ value: "whisper-1", label: "Whisper 1" }, { value: "none", label: "未配置" }]} onChange={() => notify("ASR 模型已更新")} /></SettingRow></div> : null}
          {detailTab === "embedding" ? <div className="setting-list"><SettingRow title="Embedding 模型" description="用于知识库索引和语义召回。"><SelectControl label="Embedding 模型" value="text-embedding-3" options={[{ value: "text-embedding-3", label: "Text Embedding 3" }, { value: "none", label: "未配置" }]} onChange={() => notify("Embedding 模型已更新")} /></SettingRow></div> : null}
          <div className="action-row" style={{ marginTop: 12 }}>
            <Button onClick={() => notify(`${provider.name} 连接测试成功`)}>测试连接</Button>
            <Button variant={connected[provider.id] ? "danger" : "primary"} onClick={() => setConnected((current) => ({ ...current, [provider.id]: !current[provider.id] }))}>{connected[provider.id] ? "移除凭据" : "添加凭据"}</Button>
          </div>
        </Card>
      </div> : <div className="page-stack"><Card icon="✧" title="辅助任务路由" subtitle="为压缩、标题、智能审批和记忆审查选择独立模型链。" action={<Tag tone="tone">未配置时跟随主模型</Tag>}><div className="setting-list">{[["上下文压缩", "长对话摘要与工具结果压缩"], ["标题生成", "会话与任务标题"], ["智能审批", "命令风险评估"], ["记忆审查", "记忆写入与待审提案"]].map(([title, description], index) => <SettingRow key={title} title={title} description={description}><SelectControl label={title} value={index < 2 ? "auto" : "gpt-5.6-mini"} options={[{ value: "auto", label: "跟随主模型" }, { value: "gpt-5.6-mini", label: "GPT-5.6 Mini" }, { value: "gemini-flash", label: "Gemini Flash" }]} onChange={() => notify(`${title}路由已更新`)} /></SettingRow>)}</div></Card></div>}
    </div>
  );
}

function ToolsPage({ tab, notify }) {
  const [mainTab, setMainTab] = useStatePages("builtin");
  const [query, setQuery] = useStatePages("");
  const [enabled, setEnabled] = useStatePages(() => Object.fromEntries(TOOLS.map((item) => [item.id, item.enabled])));
  const [approvalMode, setApprovalMode] = useStatePages("ask_for_approval");
  const [browserRules, setBrowserRules] = useStatePages([{ origin: "https://github.com", action: "state changing" }]);
  const [commandRules, setCommandRules] = useStatePages(["git status", "cargo test"]);
  const [commandDraft, setCommandDraft] = useStatePages("");
  const filtered = TOOLS.filter((item) => `${item.name} ${item.detail}`.toLowerCase().includes(query.toLowerCase()));
  function changeApprovalMode(value) {
    setApprovalMode(value);
    notify(value === "full_access" ? "完全访问仍不会越过硬性安全边界" : "审批策略已更新");
  }
  return (
    <div className="page" data-screen-label="工具与技能">
      <PageHeader tab={tab} action={<Segmented label="工具主页签" value={mainTab} options={[{ value: "builtin", label: "内置工具" }, { value: "approvals", label: "审批与权限" }]} onChange={setMainTab} />} />
      {mainTab === "builtin" ? <Card icon="✧" title="内置工具" subtitle="禁用的工具不会注册到当前 Agent。">
          <div className="toolbar"><div className="inline-tags"><Tag tone="tone">网格视图</Tag><Tag>{TOOLS.length} 个工具集</Tag></div><span className="toolbar-spacer"></span><input className="text-input" style={{ maxWidth: 240 }} placeholder="搜索工具" value={query} onChange={(event) => setQuery(event.target.value)} /></div>
          <div className="tool-list">{filtered.map((item) => <div className="tool-row" key={item.id}><span className="tool-logo">{item.mark}</span><span className="row-copy"><span className="row-title">{item.name}<Tag>{item.scope}</Tag></span><span className="row-sub">{item.detail}</span></span><Toggle label={`启用 ${item.name}`} checked={enabled[item.id] ?? item.enabled} onChange={(value) => setEnabled((current) => ({ ...current, [item.id]: value }))} /></div>)}{filtered.length === 0 ? <div className="nav-empty">没有匹配的工具</div> : null}</div>
        </Card> : <div className="page-stack">
          <Card icon="◈" title="命令审批策略" subtitle="默认使用工作区用户审批；更宽松的模式仍受硬性安全边界约束。">
            <div className="choice-grid">{[["ask_for_approval", "询问批准", "在需要扩大权限时由你决定。"], ["approve_for_me", "自动审阅", "根据命令风险和项目信任自动决定。"], ["full_access", "完全访问", "允许普通升级，不跳过硬性拒绝。"]].map(([value, title, description]) => <button key={value} className={`choice-card ${approvalMode === value ? "active" : ""}`} onClick={() => changeApprovalMode(value)}><strong>{title}</strong><small>{description}</small></button>)}</div>
          </Card>
          <div className="two-column">
            <Card icon="◎" title="浏览器授权" subtitle="已批准的站点状态变更操作。"><div className="permission-list">{browserRules.length ? browserRules.map((rule, index) => <div className="permission-row" key={rule.origin}><span className="row-copy"><span className="row-title">{rule.origin}</span><span className="row-sub">{rule.action}</span></span><Button variant="ghost" onClick={() => setBrowserRules((rows) => rows.filter((_, i) => i !== index))}>移除</Button></div>) : <div className="nav-empty">暂无站点授权</div>}</div></Card>
            <Card icon=">_" title="命令允许列表" subtitle="完全匹配的命令可在询问模式下直接执行。"><div className="toolbar"><input className="text-input" placeholder="例如 cargo test" value={commandDraft} onChange={(event) => setCommandDraft(event.target.value)} /><Button disabled={!commandDraft.trim()} onClick={() => { setCommandRules((rows) => [...rows, commandDraft.trim()]); setCommandDraft(""); }}>添加</Button></div><div className="inline-tags">{commandRules.map((command) => <button className="tag" key={command} onClick={() => setCommandRules((rows) => rows.filter((item) => item !== command))}>{command} ×</button>)}</div></Card>
          </div>
          <Card compact icon="!" title="硬性安全边界" subtitle="即使选择完全访问，凭据窃取、破坏性系统命令和未授权的外部操作仍会被拒绝。" />
        </div>}
    </div>
  );
}

function MemoryPage({ tab, settings, patch, notify }) {
  const [view, setView] = useStatePages("diary");
  const [pending, setPending] = useStatePages(2);
  const [dreamEnabled, setDreamEnabled] = useStatePages(true);
  const [dreamRunning, setDreamRunning] = useStatePages(false);
  function runDreaming() {
    setDreamRunning(true);
    window.setTimeout(() => { setDreamRunning(false); notify("入梦完成：生成 3 条新记忆和 1 份摘要"); }, 1100);
  }
  return (
    <div className="page" data-screen-label="记忆">
      <PageHeader tab={tab} action={<Segmented label="记忆视图" value={view} options={[{ value: "diary", label: "日记" }, { value: "dream", label: "入梦" }, { value: "longterm", label: "长期记忆" }, { value: "pending", label: pending ? `待审 ${pending}` : "待审" }]} onChange={setView} />} />
      <div className="page-stack">
        {view === "diary" ? <><Card icon="◷" title="每日日记" subtitle="从会话中沉淀的日度摘要，可按日期和 Agent 查看。" action={<Button onClick={() => notify("已刷新日记索引") }>刷新</Button>}><div className="diary-calendar">{["01", "02", "03", "04", "05", "06", "07", "08", "09", "10", "11", "12", "13", "14"].map((day) => <button key={day} className={day === "06" ? "active" : ["02", "04", "05"].includes(day) ? "has-entry" : ""}>{day}</button>)}</div></Card><Card icon="06" title="9 月 6 日" subtitle="2 条日记 · 1 个 Agent"><div className="memory-timeline"><div><Tag tone="tone">root</Tag><strong>完成设置中心交互原型</strong><p>统一了前后端格式化命令，并完成 GitFlow 分支整理。</p></div><div><Tag>design</Tag><strong>壁纸设置扩展为完整设置中心</strong><p>保留了原有玻璃与 tone 视觉体系。</p></div></div></Card></> : null}
        {view === "dream" ? <><Card className="memory-hero" icon="☾" title="入梦管道" subtitle={dreamRunning ? "正在整理日记、提取事实并生成长期记忆。" : "将近期日记聚合为可复用的长期记忆。"}><Toggle label="启用入梦" checked={dreamEnabled} onChange={setDreamEnabled} /></Card><div className="stats-grid"><Stat label="待处理日记" value="4" trend="2 个 Agent" /><Stat label="记忆点" value="128" trend="本月 +19" /><Stat label="摘要" value="12" trend="最近于昨天" /><Stat label="运行状态" value={dreamRunning ? "运行中" : "空闲"} trend={dreamEnabled ? "已启用" : "已停用"} /></div><Card icon="✧" title="立即入梦" subtitle="在后台运行一次记忆整理，不会中断当前对话。" action={<Button variant="primary" disabled={!dreamEnabled || dreamRunning} onClick={runDreaming}>{dreamRunning ? "正在入梦…" : "立即运行"}</Button>} /></> : null}
        {view === "longterm" ? <><Card className="memory-hero" icon="◌" title="长期记忆状态" subtitle="当前工作区的记忆已加载，用户画像与项目事实保持同步。"><div className="memory-score">78%</div></Card><Card icon="MD" title="记忆文件" subtitle="直接编辑被 Agent 在下一轮读取的内容。"><div className="memory-files"><div className="file-card"><div className="file-card-head"><span className="file-card-name">MEMORY.md</span><Tag tone="success">已同步</Tag></div><p>8.4 KB · 34 条可复用上下文</p><Button onClick={() => notify("已打开 MEMORY.md 编辑器") }>打开编辑</Button></div><div className="file-card"><div className="file-card-head"><span className="file-card-name">USER.md</span><Tag tone="success">已同步</Tag></div><p>2.1 KB · 用户偏好与沟通方式</p><Button onClick={() => notify("已打开 USER.md 编辑器") }>打开编辑</Button></div></div></Card></> : null}
        {view === "pending" ? <><Card icon="✓" title="待审核记忆" subtitle="Agent 提议的长期记忆在写入前可由你确认。" action={<Tag tone={pending ? "warning" : "success"}>{pending ? `${pending} 条待处理` : "已清空"}</Tag>}>{pending ? <div className="detail-panel"><div className="inline-tags"><Tag tone="tone">append</Tag><Tag>MEMORY.md</Tag><Tag>root</Tag></div><h3 style={{ marginTop: 9 }}>偏好使用简洁的中文结果</h3><p>来源：最近的前端格式化任务</p><div className="action-row" style={{ marginTop: 10 }}><Button variant="ghost" onClick={() => setPending((value) => Math.max(0, value - 1))}>拒绝</Button><Button variant="primary" onClick={() => { setPending((value) => Math.max(0, value - 1)); notify("记忆已批准并写入"); }}>批准</Button></div></div> : <div className="nav-empty">暂无待审核记忆</div>}</Card><Card icon="⚙" title="记忆设置" subtitle="控制快照刷新、后台审查和写入批准。"><div className="setting-list"><SettingRow title="文件更新时刷新" description="检测到 MEMORY.md 变更后自动更新快照。"><Toggle label="自动刷新记忆" checked={settings.memoryAutoRefresh} onChange={(memoryAutoRefresh) => patch({ memoryAutoRefresh })} /></SettingRow><SettingRow title="后台审查" description="在空闲时检查待审记忆。"><Toggle label="后台审查" checked={settings.memoryBackgroundReview} onChange={(memoryBackgroundReview) => patch({ memoryBackgroundReview })} /></SettingRow><SettingRow title="写入前需要批准" description="新记忆必须在此确认后才会落盘。"><Toggle label="记忆写入审批" checked={settings.memoryApproval} onChange={(memoryApproval) => patch({ memoryApproval })} /></SettingRow></div></Card></> : null}
      </div>
    </div>
  );
}

function BrowserPage({ tab, settings, patch, notify }) {
  const [sites, setSites] = useStatePages([
    { host: "https://github.com", access: "允许会改变站点状态的操作", tone: "success" },
    { host: "http://localhost:4311", access: "本地调试页面", tone: "tone" },
  ]);
  return (
    <div className="page" data-screen-label="浏览器">
      <PageHeader tab={tab} />
      <div className="page-stack">
        <div className="two-column">
          <Card icon="◎" title="浏览器运行时" subtitle="本地浏览器运行时已就绪。" action={<Tag tone="success">可用</Tag>}>
            <SettingRow title="数据目录" description="Cookie、缓存和标签状态只保存在本机。"><span className="code-field">~/.astro/browser</span></SettingRow>
          </Card>
          <Card icon="▣" title="启动页与视口" subtitle="用统一的初始页和视口尺寸启动新标签。">
            <div className="setting-list">
              <SettingRow title="主页" description="未带 URL 打开新标签时使用。"><input className="text-input" style={{ width: 220 }} aria-label="浏览器主页" value={settings.browserHomePage} onChange={(event) => patch({ browserHomePage: event.target.value })} /></SettingRow>
              <SettingRow title="视口预设"><Segmented label="浏览器视口" value={settings.viewport} options={[{ value: "1280 × 800", label: "1280 × 800" }, { value: "1440 × 900", label: "1440 × 900" }, { value: "390 × 844", label: "390 × 844" }]} onChange={(viewport) => patch({ viewport })} /></SettingRow>
            </div>
          </Card>
        </div>
        <Card icon="◈" title="运行权限" subtitle="只开启工作需要的浏览器能力。">
          <div className="setting-list">
            <SettingRow title="允许本机回环地址" description="访问 localhost 和精确端口，用于预览本地开发项目。"><Toggle label="允许本机回环地址" checked={settings.browserLoopback} onChange={(browserLoopback) => patch({ browserLoopback })} /></SettingRow>
            <SettingRow title="允许下载" description="下载仍会遵循工作区路径与权限策略。"><Toggle label="允许下载" checked={settings.browserDownloadsEnabled} onChange={(browserDownloadsEnabled) => patch({ browserDownloadsEnabled })} /></SettingRow>
          </div>
        </Card>
        <Card icon="◇" title="已批准站点" subtitle="站点权限在操作发生时申请，只能在此撤销。">
          <div className="permission-list">
            {sites.map((site, index) => (
              <div className="permission-row" key={`${site.host}-${index}`}><span className="provider-logo">◎</span><span className="row-copy"><span className="row-title">{site.host}</span><span className="row-sub">{site.access}</span></span><Tag tone={site.tone}>{site.tone === "success" ? "state changing" : "local"}</Tag><Button variant="ghost" onClick={() => { setSites((current) => current.filter((_, itemIndex) => itemIndex !== index)); notify(`已撤销 ${site.host} 的浏览器授权`); }}>撤销</Button></div>
            ))}
          </div>
          <p className="card-sub" style={{ marginTop: 11 }}>密码、付款与其他敏感操作不会因站点授权而自动执行。</p>
        </Card>
      </div>
    </div>
  );
}

function ModelsPage({ tab, notify }) {
  const [surface, setSurface] = useStatePages("catalog");
  const [query, setQuery] = useStatePages("");
  const [capability, setCapability] = useStatePages("all");
  const [modelType, setModelType] = useStatePages("all");
  const [viewMode, setViewMode] = useStatePages("gallery");
  const [selected, setSelected] = useStatePages("gpt-5.6");
  const filtered = MODELS.filter((model) => {
    const queryMatch = `${model.name} ${model.provider}`.toLowerCase().includes(query.toLowerCase());
    const capMatch = capability === "all" || model.caps.some((cap) => cap.toLowerCase().includes(capability));
    const typeMatch = modelType === "all" || model.type === modelType;
    return queryMatch && capMatch && typeMatch;
  });
  const model = MODELS.find((item) => item.id === selected) ?? MODELS[0];
  return (
    <div className="page" data-screen-label="模型市场">
      <PageHeader tab={tab} action={<div className="toolbar" style={{ margin: 0 }}><Segmented label="模型市场视图" value={surface} options={[{ value: "catalog", label: "模型目录" }, { value: "rankings", label: "使用排行" }]} onChange={setSurface} /><Button className="icon-button" aria-label="刷新模型目录" onClick={() => notify("模型目录已刷新") }>↻</Button></div>} />
      {surface === "catalog" ? <div className="page-stack"><Card icon="⬡" title="可用模型" subtitle="能力来自 Provider 元数据，并标记可配置的运行时用途。">
          <div className="toolbar"><input className="text-input" placeholder="搜索模型或供应商" value={query} onChange={(event) => setQuery(event.target.value)} /><Segmented label="布局" value={viewMode} options={[{ value: "gallery", label: "网格" }, { value: "list", label: "列表" }, { value: "detail", label: "详情" }]} onChange={setViewMode} /></div>
          <div className="model-type-tabs" role="tablist" aria-label="模型类型">{[["all", "全部"], ["generation", "生成"], ["image", "图像"], ["video", "视频"], ["speech", "语音"], ["transcription", "转录"], ["music", "音乐"], ["embedding", "Embedding"], ["rerank", "Rerank"]].map(([value, label]) => { const candidates = value === "all" ? MODELS : MODELS.filter((item) => item.type === value); return <button role="tab" aria-selected={modelType === value} className={modelType === value ? "active" : ""} key={value} onClick={() => { setModelType(value); setCapability("all"); if (candidates[0]) setSelected(candidates[0].id); }}>{label}<span>{candidates.length}</span></button>; })}</div>
          <div className="toolbar model-filter-row">{modelType === "all" || modelType === "generation" ? <Segmented label="能力过滤" value={capability} options={[{ value: "all", label: "所有能力" }, { value: "tools", label: "Tools" }, { value: "vision", label: "Vision" }, { value: "reasoning", label: "Reasoning" }]} onChange={setCapability} /> : <Tag tone="tone">{modelType}</Tag>}<span className="toolbar-spacer"></span><Tag>{filtered.length} 个模型</Tag></div>
          {viewMode === "gallery" ? <div className="model-grid">{filtered.map((item) => <button key={item.id} className={`model-card ${selected === item.id ? "selected" : ""}`} onClick={() => setSelected(item.id)}><span className="model-card-provider">{item.provider}</span><span className="model-card-title">{item.name}</span><span className="model-card-meta">{item.caps.map((cap) => <Tag key={cap} tone={cap === "Tools" ? "tone" : ""}>{cap}</Tag>)}</span><span className="row-sub">上下文 {item.context}</span></button>)}</div> : null}
          {viewMode === "list" ? <div className="tool-list">{filtered.map((item) => <button key={item.id} className={`tool-row ${selected === item.id ? "selected" : ""}`} onClick={() => setSelected(item.id)}><span className="provider-logo">{item.provider.slice(0, 2)}</span><span className="row-copy"><span className="row-title">{item.name}<Tag>{item.type}</Tag></span><span className="row-sub">{item.provider} · {item.caps.join(" · ")}</span></span><strong>{item.context}</strong></button>)}</div> : null}
          {viewMode === "detail" ? <div className="model-detail-split"><div className="model-detail-list">{filtered.map((item) => <button key={item.id} className={selected === item.id ? "selected" : ""} onClick={() => setSelected(item.id)}><span><small>{item.provider}</small><strong>{item.name}</strong></span><b>{item.context}</b></button>)}</div><div className="detail-panel"><div className="detail-panel-head"><div><h3>{model.name}</h3><p>{model.provider} · {model.type}</p></div><Tag tone="success">可用</Tag></div><div className="setting-list"><SettingRow title="上下文">{model.context}</SettingRow><SettingRow title="能力"><div className="inline-tags">{model.caps.map((cap) => <Tag key={cap}>{cap}</Tag>)}</div></SettingRow><SettingRow title="运行时"><Button variant="primary" onClick={() => notify(`${model.name} 已配置到当前任务`)}>配置模型</Button></SettingRow></div></div></div> : null}
          {filtered.length === 0 ? <div className="nav-empty">没有匹配的模型</div> : null}
        </Card>{viewMode !== "detail" ? <Card compact icon="◎" title={model.name} subtitle={`${model.provider} · ${model.caps.join(" · ")}`} action={<Button variant="primary" onClick={() => notify(`${model.name} 已配置到当前任务`)}>配置模型</Button>}><div className="inline-tags"><Tag>上下文 {model.context}</Tag><Tag tone="success">可用</Tag><Tag tone="tone">Responses</Tag></div></Card> : null}</div> : <div className="page-stack"><div className="stats-grid"><Stat label="请求量最高" value="GPT-5.6" trend="32.4% 份额" /><Stat label="批处理" value="DeepSeek V4" trend="1.8M tokens" /><Stat label="工具使用" value="Gemini 3.5" trend="96.8% 成功" /><Stat label="性价比" value="Flash" trend="$0.42 / M" /></div><Card icon="↗" title="OpenRouter 公开排行" subtitle="数据优先来自官方 Data API，必要时使用 frontend 缓存补位。"><div className="rank-list">{[["GPT-5.6", 94, "1.42B"], ["Gemini 3.5 Pro", 78, "1.10B"], ["DeepSeek V4", 61, "842M"], ["Claude Opus 5", 46, "633M"]].map(([name, width, value], index) => <div className="rank-row" key={name}><span className="rank-index">{index + 1}</span><span><span>{name}</span><span className="rank-meter"><i style={{ width: `${width}%` }}></i></span></span><strong>{value}</strong></div>)}</div></Card></div>}
    </div>
  );
}

function InsightsPage({ tab }) {
  const [view, setView] = useStatePages("overview");
  const [period, setPeriod] = useStatePages("month");
  const [metric, setMetric] = useStatePages("tokens");
  const [trace, setTrace] = useStatePages("settings-prototype");
  const datasets = {
    month: [42, 58, 47, 75, 64, 88, 72, 95, 82, 70, 91, 78],
    quarter: [34, 51, 67, 59, 73, 82, 76, 90, 84, 94, 88, 99],
    year: [28, 36, 44, 52, 61, 58, 72, 79, 76, 86, 92, 97],
  };
  return (
    <div className="page" data-screen-label="数据洞察">
      <PageHeader tab={tab} action={<Segmented label="统计周期" value={period} options={[{ value: "month", label: "月" }, { value: "quarter", label: "季度" }, { value: "year", label: "年" }]} onChange={setPeriod} />} />
      <div className="page-stack">
        <div className="toolbar insights-toolbar"><Segmented label="洞察视图" value={view} options={[{ value: "overview", label: "概览" }, { value: "models", label: "模型" }, { value: "tools", label: "工具" }, { value: "tracing", label: "Tracing" }]} onChange={setView} /><span className="toolbar-spacer"></span>{view !== "tracing" ? <Segmented label="统计指标" value={metric} options={[{ value: "calls", label: "调用" }, { value: "tokens", label: "Tokens" }, { value: "cost", label: "成本" }]} onChange={setMetric} /> : null}</div>
        {view === "overview" ? <><div className="stats-grid"><Stat label="估算成本" value={period === "month" ? "$76.30" : period === "quarter" ? "$212.84" : "$804.16"} trend="定价覆盖 94%" /><Stat label="Token" value={period === "month" ? "18.6M" : period === "quarter" ? "54.2M" : "218M"} trend="↑ 8.1%" /><Stat label="所有调用" value={period === "month" ? "1,248" : "3,905"} trend="↑ 12.4%" /><Stat label="未定价事件" value="7" trend="需要补全模型定价" /></div><div className="split-column"><Card icon="∿" title="用量趋势" subtitle={`按日聚合的 ${metric === "tokens" ? "token" : metric === "calls" ? "调用次数" : "成本"}。`}><div className="chart">{datasets[period].map((height, index) => <div key={index} className="chart-bar" style={{ height: `${height}%` }} title={`${height}${metric === "cost" ? "$" : metric === "calls" ? " calls" : "k tokens"}`}></div>)}</div></Card><Card icon="≡" title="Provider 排行" subtitle={`按${metric === "cost" ? "成本" : metric === "calls" ? "调用次数" : "token"}汇总。`}><div className="rank-list">{[["OpenAI", 88, metric === "cost" ? "$31.24" : "8.4M"], ["Google", 61, metric === "cost" ? "$19.80" : "5.9M"], ["DeepSeek", 39, metric === "cost" ? "$8.16" : "3.2M"], ["OpenRouter", 24, metric === "cost" ? "$5.40" : "1.8M"]].map(([name, width, value], index) => <div className="rank-row" key={name}><span className="rank-index">{index + 1}</span><span><span>{name}</span><span className="rank-meter"><i style={{ width: `${width}%` }}></i></span></span><strong>{value}</strong></div>)}</div></Card></div></> : null}
        {view === "models" ? <><div className="stats-grid"><Stat label="模型数" value="7" trend="4 个 Provider" /><Stat label="Agent 数" value="5" trend="3 个活跃" /><Stat label="输入 Tokens" value="12.8M" trend="68.8%" /><Stat label="推理 Tokens" value="1.7M" trend="9.1%" /></div><Card icon="⬡" title="模型用量" subtitle="比较模型的 token、调用和成本。"><div className="rank-list">{[["GPT-5.6", 96, "7.2M"], ["Gemini 3.5 Pro", 73, "5.4M"], ["DeepSeek V4", 51, "3.8M"], ["Gemini Flash", 29, "2.2M"]].map(([name, width, value], index) => <div className="rank-row" key={name}><span className="rank-index">{index + 1}</span><span><span>{name}</span><span className="rank-meter"><i style={{ width: `${width}%` }}></i></span></span><strong>{value}</strong></div>)}</div></Card></> : null}
        {view === "tools" ? <><div className="stats-grid"><Stat label="工具调用" value="864" trend="Terminal 38%" /><Stat label="Skills" value="142" trend="code-review 最常用" /><Stat label="MCP" value="76" trend="5 个服务活跃" /><Stat label="Cron" value="31" trend="30 次成功" /></div><Card icon="◈" title="能力调用排行" subtitle="将工具、Skill、MCP 和 Cron 放在同一尺度下比较。"><div className="rank-list">{[["terminal", 94, "328"], ["code-review", 66, "142"], ["mcp__browser", 48, "76"], ["daily-alignment", 22, "31"]].map(([name, width, value], index) => <div className="rank-row" key={name}><span className="rank-index">{index + 1}</span><span><span>{name}</span><span className="rank-meter"><i style={{ width: `${width}%` }}></i></span></span><strong>{value}</strong></div>)}</div></Card></> : null}
        {view === "tracing" ? <div className="trace-split"><div className="trace-list">{[["settings-prototype", "设置原型精修", "12 events"], ["daily-alignment", "Codex 每日对齐", "28 events"], ["browser-check", "浏览器回归验证", "9 events"]].map(([id, title, detail]) => <button className={trace === id ? "selected" : ""} key={id} onClick={() => setTrace(id)}><span><strong>{title}</strong><small>{detail}</small></span><Tag tone={id === "settings-prototype" ? "success" : ""}>{id === "settings-prototype" ? "done" : "trace"}</Tag></button>)}</div><Card compact icon="◎" title={trace === "settings-prototype" ? "设置原型精修" : trace === "daily-alignment" ? "Codex 每日对齐" : "浏览器回归验证"} subtitle="按 Turn 组织的事件链与 token / 成本明细。"><div className="memory-timeline"><div><Tag tone="tone">user</Tag><strong>收到任务</strong><p>0 tokens · 0ms</p></div><div><Tag>llm</Tag><strong>生成与工具编排</strong><p>18,402 tokens · 7.4s · $0.18</p></div><div><Tag tone="success">tool</Tag><strong>验证并提交</strong><p>4 calls · done</p></div></div></Card></div> : null}
      </div>
    </div>
  );
}

function DiagnosticsPage({ tab, notify }) {
  const [level, setLevel] = useStatePages("all");
  const [source, setSource] = useStatePages("both");
  const [scope, setScope] = useStatePages("current");
  const [lines, setLines] = useStatePages("100");
  const [advanced, setAdvanced] = useStatePages(false);
  const [query, setQuery] = useStatePages("");
  const filtered = LOG_ROWS.filter((row) => {
    const levelMatch = level === "all" || (level === "issues" ? row.level !== "info" : row.level === level);
    const sourceMatch = source === "both" || (source === "errors" ? row.level === "error" : row.level !== "error");
    return levelMatch && sourceMatch && `${row.source} ${row.message}`.toLowerCase().includes(query.toLowerCase());
  });
  return (
    <div className="page" data-screen-label="诊断">
      <PageHeader tab={tab} action={<Button onClick={() => notify("诊断信息已刷新") }>刷新</Button>} />
      <div className="page-stack">
        <div className="stats-grid"><Stat label="Backend" value="正常" trend="127.0.0.1 · 内嵌" /><Stat label="Provider" value="4 / 4" trend="平均延迟 842ms" /><Stat label="MCP" value="4 / 5" trend="1 个正在重试" /><Stat label="数据库" value="WAL" trend="schema v22" /></div>
        <Card icon="△" title="运行日志" subtitle="日志按日滚动，默认只展示最近事件。" action={<Button onClick={() => notify("已复制诊断摘要") }>复制摘要</Button>}>
          <div className="diagnostic-filters"><label><span>范围</span><SelectControl label="日志范围" value={scope} options={[{ value: "current", label: "当前会话" }, { value: "all", label: "全部会话" }]} onChange={setScope} /></label><label><span>来源</span><SelectControl label="日志来源" value={source} options={[{ value: "both", label: "Agent + Errors" }, { value: "agent", label: "Agent" }, { value: "errors", label: "Errors" }]} onChange={setSource} /></label><label><span>级别</span><SelectControl label="日志级别" value={level} options={[{ value: "all", label: "全部" }, { value: "issues", label: "仅问题" }, { value: "warn", label: "Warn" }, { value: "error", label: "Error" }]} onChange={setLevel} /></label><label><span>行数</span><SelectControl label="日志行数" value={lines} options={[{ value: "50", label: "50" }, { value: "100", label: "100" }, { value: "500", label: "500" }]} onChange={setLines} /></label></div>
          <div className="toolbar"><input className="text-input" placeholder="搜索日志内容" value={query} onChange={(event) => setQuery(event.target.value)} /><Button variant="ghost" onClick={() => setAdvanced((value) => !value)}>{advanced ? "收起高级过滤" : "高级过滤"}</Button></div>
          {advanced ? <div className="diagnostic-advanced"><label><span>Session ID</span><input className="text-input" placeholder="可选" /></label><label><span>Turn ID</span><input className="text-input" placeholder="可选" /></label><Tag tone="tone">最多返回 500 行</Tag></div> : null}
          <div className="log-list">{filtered.slice(0, Number(lines)).map((row, index) => <div className="log-row" key={`${row.time}-${index}`}><span className="log-time">{row.time}</span><span className={`log-level ${row.level}`}>{row.level.toUpperCase()}</span><span><strong>{row.source}</strong> · {row.message}</span></div>)}</div>
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
