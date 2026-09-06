function Card({ icon, title, subtitle, action, children, compact = false, className = "" }) {
  return (
    <section className={`card ${compact ? "compact" : ""} ${className}`}>
      {(icon || title) && (
        <div className="card-head">
          {icon ? <div className="card-icon" aria-hidden>{icon}</div> : null}
          <div className="card-head-copy">
            <h2 className="card-title">{title}</h2>
            {subtitle ? <p className="card-sub">{subtitle}</p> : null}
          </div>
          {action ? <div className="card-action">{action}</div> : null}
        </div>
      )}
      {children}
    </section>
  );
}

function PageHeader({ tab, badge = "可交互设计稿", action }) {
  return (
    <header className="page-head">
      <div>
        <p className="eyebrow">{tab.eyebrow}</p>
        <h1>{tab.label}</h1>
        <p className="page-intro">{tab.description}</p>
      </div>
      {action ?? <span className="page-badge">{badge}</span>}
    </header>
  );
}

function Toggle({ checked, onChange, label }) {
  return (
    <button
      type="button"
      role="switch"
      aria-label={label}
      aria-checked={checked}
      className={`toggle ${checked ? "on" : ""}`}
      onClick={() => onChange(!checked)}
    >
      <span></span>
    </button>
  );
}

function Segmented({ value, options, onChange, label }) {
  return (
    <div className="segmented" role="radiogroup" aria-label={label}>
      {options.map((option) => (
        <button
          type="button"
          role="radio"
          aria-checked={value === option.value}
          className={`segment ${value === option.value ? "active" : ""}`}
          key={option.value}
          onClick={() => onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
    </div>
  );
}

function SettingRow({ title, description, children }) {
  return (
    <div className="setting-row">
      <div className="setting-copy">
        <strong>{title}</strong>
        {description ? <small>{description}</small> : null}
      </div>
      <div className="setting-control">{children}</div>
    </div>
  );
}

function SelectControl({ value, options, onChange, label }) {
  return (
    <select className="select" value={value} aria-label={label} onChange={(event) => onChange(event.target.value)}>
      {options.map((option) => (
        <option key={option.value} value={option.value}>{option.label}</option>
      ))}
    </select>
  );
}

function SliderControl({ value, min, max, step = 1, suffix = "", onChange, label }) {
  return (
    <div className="slider-wrap">
      <input className="slider" type="range" aria-label={label} min={min} max={max} step={step} value={value} onChange={(event) => onChange(Number(event.target.value))} />
      <span className="slider-value">{value}{suffix}</span>
    </div>
  );
}

function Tag({ children, tone = "" }) {
  return <span className={`tag ${tone}`}>{children}</span>;
}

function Stat({ label, value, trend }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {trend ? <div className="stat-trend">{trend}</div> : null}
    </div>
  );
}

function Button({ children, variant = "", className = "", ...props }) {
  return <button type="button" className={`button ${variant} ${className}`} {...props}>{children}</button>;
}

Object.assign(window, {
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
});
