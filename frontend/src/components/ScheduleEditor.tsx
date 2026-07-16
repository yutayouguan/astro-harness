/** 调度表达式编辑器。 */
import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ComponentType,
} from "react";
import { createPortal } from "react-dom";
import {
  CalendarClock,
  CalendarDays,
  ChevronLeft,
  ChevronRight,
  Clock,
  Minus,
  Plus,
  Timer,
  type LucideProps,
} from "lucide-react";
import type { ScheduleDraft, ScheduleMode, Weekday } from "../lib/cron/cronSchedule";
import { UI_WEEKDAYS } from "../lib/cron/cronSchedule";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";
import { useAnchoredMenu } from "../hooks/useAnchoredMenu";
import { SelectMenu } from "./SelectMenu";

/** 调度表达式编辑器入参 */
type Props = {
  value: ScheduleDraft;
  onChange: (next: ScheduleDraft) => void;
};

type ModeIcon = ComponentType<LucideProps>;

const MODES: { mode: ScheduleMode; labelKey: MessageKey; Icon: ModeIcon }[] = [
  { mode: "daily", labelKey: "cron.mode.daily", Icon: CalendarDays },
  { mode: "interval", labelKey: "cron.mode.interval", Icon: Timer },
  { mode: "once", labelKey: "cron.mode.once", Icon: CalendarClock },
];

function pad2(n: number) {
  return String(n).padStart(2, "0");
}

/** 解析 datetime-local / ISO 为本地年月日时分 */
function parseOnceParts(onceAt?: string): {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
} {
  const d = onceAt ? new Date(onceAt) : new Date();
  const base = Number.isNaN(d.getTime()) ? new Date() : d;
  return {
    year: base.getFullYear(),
    month: base.getMonth() + 1,
    day: base.getDate(),
    hour: base.getHours(),
    minute: base.getMinutes(),
  };
}

function toOnceLocal(parts: {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
}): string {
  return `${parts.year}-${pad2(parts.month)}-${pad2(parts.day)}T${pad2(parts.hour)}:${pad2(parts.minute)}`;
}

function daysInMonth(year: number, month: number) {
  return new Date(year, month, 0).getDate();
}

/** 周一为周首的格子（null = 空白） */
function buildMonthCells(year: number, month: number): (number | null)[] {
  const first = new Date(year, month - 1, 1);
  const startPad = (first.getDay() + 6) % 7;
  const total = daysInMonth(year, month);
  const cells: (number | null)[] = [];
  for (let i = 0; i < startPad; i++) cells.push(null);
  for (let d = 1; d <= total; d++) cells.push(d);
  while (cells.length % 7 !== 0) cells.push(null);
  return cells;
}

/** 可视化编辑 daily / interval / once 调度 */
export function ScheduleEditor({ value, onChange }: Props) {
  const { t } = useI18n();

  const setMode = (mode: ScheduleMode) => {
    if (mode === value.mode) return;
    onChange({ ...value, mode });
  };

  const toggleWeekday = (day: Weekday) => {
    const has = value.weekdays.includes(day);
    const weekdays = has
      ? value.weekdays.filter((d) => d !== day)
      : [...value.weekdays, day];
    onChange({ ...value, weekdays });
  };

  const intervalValue = value.intervalValue ?? 1;

  return (
    <div className="cron-sched">
      <div className="cron-sched-label">
        <CalendarClock size={13} strokeWidth={2.2} aria-hidden />
        <span>{t("cron.field.schedule")}</span>
      </div>

      <div className="cron-sched-modes" role="tablist" aria-label={t("cron.field.schedule")}>
        {MODES.map(({ mode, labelKey, Icon }) => (
          <button
            key={mode}
            type="button"
            role="tab"
            aria-selected={value.mode === mode}
            className={`cron-sched-mode${value.mode === mode ? " active" : ""}`}
            onClick={() => setMode(mode)}
          >
            <Icon size={14} strokeWidth={2.2} aria-hidden />
            <span>{t(labelKey)}</span>
          </button>
        ))}
      </div>

      {value.mode === "daily" && (
        <div className="cron-sched-body">
          <div className="cron-sched-row">
            <label className="cron-sched-field-shell cron-sched-field-shell--time">
              <input
                type="time"
                className="cron-sched-input cron-sched-input--bare"
                value={value.time ?? "09:00"}
                onChange={(e) => onChange({ ...value, time: e.target.value })}
              />
            </label>
          </div>
          <WeekdayChips
            selected={value.weekdays}
            onToggle={toggleWeekday}
            t={t}
          />
        </div>
      )}

      {value.mode === "interval" && (
        <div className="cron-sched-body">
          <div className="cron-sched-row cron-sched-interval">
            <span className="cron-sched-interval-label">{t("cron.interval.every")}</span>
            <div className="cron-sched-stepper" role="group" aria-label={t("cron.interval.every")}>
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label="-"
                onClick={() =>
                  onChange({
                    ...value,
                    intervalValue: Math.max(1, intervalValue - 1),
                  })
                }
              >
                <Minus size={14} strokeWidth={2.4} aria-hidden />
              </button>
              <input
                type="number"
                min={1}
                className="cron-sched-input cron-sched-input--num cron-sched-input--bare"
                value={intervalValue}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  onChange({
                    ...value,
                    intervalValue: Number.isFinite(n) && n >= 1 ? Math.floor(n) : 1,
                  });
                }}
              />
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label="+"
                onClick={() =>
                  onChange({
                    ...value,
                    intervalValue: intervalValue + 1,
                  })
                }
              >
                <Plus size={14} strokeWidth={2.4} aria-hidden />
              </button>
            </div>
            <SelectMenu
              className="cron-sched-unit-menu"
              size="sm"
              value={value.intervalUnit ?? "m"}
              onChange={(unit) =>
                onChange({
                  ...value,
                  intervalUnit: unit === "h" ? "h" : "m",
                })
              }
              aria-label={t("cron.field.schedule")}
              options={[
                {
                  value: "m",
                  label: t("cron.interval.m"),
                  icon: <Timer size={13} strokeWidth={2.2} aria-hidden />,
                },
                {
                  value: "h",
                  label: t("cron.interval.h"),
                  icon: <Clock size={13} strokeWidth={2.2} aria-hidden />,
                },
              ]}
            />
          </div>
          <WeekdayChips
            selected={value.weekdays}
            onToggle={toggleWeekday}
            t={t}
          />
        </div>
      )}

      {value.mode === "once" && (
        <div className="cron-sched-body">
          <OnceDateTimePicker
            value={value.onceAt}
            onChange={(onceAt) => onChange({ ...value, onceAt })}
          />
        </div>
      )}
    </div>
  );
}

/** 玻璃拟态单次日期时间选择 */
function OnceDateTimePicker({
  value,
  onChange,
}: {
  value?: string;
  onChange: (next: string) => void;
}) {
  const { t, locale } = useI18n();
  const parts = useMemo(() => parseOnceParts(value), [value]);
  const [open, setOpen] = useState(false);
  const [viewYear, setViewYear] = useState(parts.year);
  const [viewMonth, setViewMonth] = useState(parts.month);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const popRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const pos = useAnchoredMenu({
    open,
    anchorRef: triggerRef,
    menuRef: popRef,
    fixedWidth: 248,
    preferAlign: "start",
    placement: "auto",
    gap: 8,
    maxHeightCap: 292,
    maxHeightRatio: 1,
    minMaxHeight: 120,
  });

  const closePop = (restoreFocus = false) => {
    setOpen(false);
    if (restoreFocus) triggerRef.current?.focus();
  };

  useEffect(() => {
    if (!open) return;
    setViewYear(parts.year);
    setViewMonth(parts.month);
  }, [open, parts.year, parts.month]);

  useEffect(() => {
    if (!open) return;
    const onDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (triggerRef.current?.contains(target)) return;
      if (popRef.current?.contains(target)) return;
      closePop();
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closePop(true);
      }
    };
    document.addEventListener("mousedown", onDown);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDown);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const label = useMemo(() => {
    const d = new Date(toOnceLocal(parts));
    if (Number.isNaN(d.getTime())) return t("cron.field.schedule");
    return d.toLocaleString(locale === "zh" ? "zh-CN" : "en-US", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      hour12: false,
    });
  }, [parts, locale, t]);

  const cells = useMemo(
    () => buildMonthCells(viewYear, viewMonth),
    [viewYear, viewMonth],
  );

  const weekdays =
    locale === "zh"
      ? ["一", "二", "三", "四", "五", "六", "日"]
      : ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];

  const monthTitle =
    locale === "zh"
      ? `${viewYear}年${viewMonth}月`
      : new Date(viewYear, viewMonth - 1, 1).toLocaleDateString("en-US", {
          month: "long",
          year: "numeric",
        });

  const shiftMonth = (delta: number) => {
    let m = viewMonth + delta;
    let y = viewYear;
    if (m < 1) {
      m = 12;
      y -= 1;
    } else if (m > 12) {
      m = 1;
      y += 1;
    }
    setViewYear(y);
    setViewMonth(m);
  };

  const pickDay = (day: number) => {
    onChange(
      toOnceLocal({
        ...parts,
        year: viewYear,
        month: viewMonth,
        day,
      }),
    );
  };

  const pop =
    open
      ? createPortal(
          <div
            ref={popRef}
            id={listId}
            className="cron-dt-pop"
            role="dialog"
            aria-label={t("cron.mode.once")}
            style={
              pos
                ? {
                    top: pos.top,
                    left: pos.left,
                    width: pos.width,
                    maxHeight: pos.maxHeight,
                  }
                : { visibility: "hidden", width: 248 }
            }
          >
            <div className="cron-dt-pop-head">
              <button
                type="button"
                className="cron-dt-nav"
                onClick={() => shiftMonth(-1)}
                aria-label="prev"
              >
                <ChevronLeft size={16} strokeWidth={2.2} aria-hidden />
              </button>
              <div className="cron-dt-month">{monthTitle}</div>
              <button
                type="button"
                className="cron-dt-nav"
                onClick={() => shiftMonth(1)}
                aria-label="next"
              >
                <ChevronRight size={16} strokeWidth={2.2} aria-hidden />
              </button>
            </div>
            <div className="cron-dt-weekdays">
              {weekdays.map((w) => (
                <span key={w}>{w}</span>
              ))}
            </div>
            <div className="cron-dt-grid">
              {cells.map((day, i) =>
                day == null ? (
                  <span key={`e-${i}`} className="cron-dt-day is-empty" />
                ) : (
                  <button
                    key={`${viewYear}-${viewMonth}-${day}`}
                    type="button"
                    className={[
                      "cron-dt-day",
                      day === parts.day &&
                      viewMonth === parts.month &&
                      viewYear === parts.year
                        ? "is-selected"
                        : "",
                      day === new Date().getDate() &&
                      viewMonth === new Date().getMonth() + 1 &&
                      viewYear === new Date().getFullYear()
                        ? "is-today"
                        : "",
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onClick={() => pickDay(day)}
                  >
                    {day}
                  </button>
                ),
              )}
            </div>
            <div className="cron-dt-time">
              <Clock size={14} strokeWidth={2.1} aria-hidden />
              <input
                type="number"
                min={0}
                max={23}
                className="cron-dt-time-input"
                value={parts.hour}
                onChange={(e) => {
                  const hour = Math.min(23, Math.max(0, Number(e.target.value) || 0));
                  onChange(toOnceLocal({ ...parts, hour }));
                }}
                aria-label="hour"
              />
              <span className="cron-dt-time-sep">:</span>
              <input
                type="number"
                min={0}
                max={59}
                className="cron-dt-time-input"
                value={parts.minute}
                onChange={(e) => {
                  const minute = Math.min(59, Math.max(0, Number(e.target.value) || 0));
                  onChange(toOnceLocal({ ...parts, minute }));
                }}
                aria-label="minute"
              />
            </div>
          </div>,
          document.body,
        )
      : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={`cron-sched-field-shell cron-sched-field-shell--datetime${open ? " is-open" : ""}`}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((v) => !v)}
      >
        <span className="cron-sched-datetime-label">{label}</span>
      </button>
      {pop}
    </>
  );
}

/** 星期多选芯片 */
function WeekdayChips({
  selected,
  onToggle,
  t,
}: {
  selected: Weekday[];
  onToggle: (day: Weekday) => void;
  t: (key: MessageKey) => string;
}) {
  return (
    <div className="cron-sched-weekdays">
      {UI_WEEKDAYS.map(({ labelKey, value }) => {
        const active = selected.includes(value);
        return (
          <button
            key={value}
            type="button"
            className={`cron-sched-wd${active ? " active" : ""}`}
            aria-pressed={active}
            onClick={() => onToggle(value)}
          >
            {t(labelKey as MessageKey)}
          </button>
        );
      })}
    </div>
  );
}
