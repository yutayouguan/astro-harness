/** 玻璃拟态日期选择（复用 cron-dt 月历样式，仅日期）。 */
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { CalendarDays, ChevronLeft, ChevronRight, X } from "lucide-react";
import { useI18n } from "../../i18n/LocaleContext";
import { useAnchoredMenu } from "../../hooks/ui/useAnchoredMenu";
import { toneStyleFromElement } from "../../lib/ui/toneFromElement";

function pad2(n: number) {
  return String(n).padStart(2, "0");
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

function parseYmd(value?: string): {
  year: number;
  month: number;
  day: number;
  hasValue: boolean;
} {
  const m = value?.trim().match(/^(\d{4})-(\d{2})-(\d{2})$/);
  if (m) {
    return {
      year: Number(m[1]),
      month: Number(m[2]),
      day: Number(m[3]),
      hasValue: true,
    };
  }
  const now = new Date();
  return {
    year: now.getFullYear(),
    month: now.getMonth() + 1,
    day: now.getDate(),
    hasValue: false,
  };
}

function toYmd(year: number, month: number, day: number) {
  return `${year}-${pad2(month)}-${pad2(day)}`;
}

type Props = {
  value: string;
  onChange: (next: string) => void;
  "aria-label"?: string;
  placeholder?: string;
  className?: string;
  /** 允许清空为未选（筛选场景） */
  allowClear?: boolean;
};

export function GlassDatePicker({
  value,
  onChange,
  "aria-label": ariaLabel,
  placeholder,
  className = "",
  allowClear = true,
}: Props) {
  const { t, locale } = useI18n();
  const parts = useMemo(() => parseYmd(value), [value]);
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
    maxHeightCap: 280,
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
    if (!parts.hasValue) {
      return placeholder || ariaLabel || t("cron.history.filterDate");
    }
    const d = new Date(parts.year, parts.month - 1, parts.day);
    return d.toLocaleDateString(locale === "zh" ? "zh-CN" : "en-US", {
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    });
  }, [parts, locale, placeholder, ariaLabel, t]);

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
    onChange(toYmd(viewYear, viewMonth, day));
    closePop();
  };

  const toneStyle = open ? toneStyleFromElement(triggerRef.current) : {};

  const pop = open
    ? createPortal(
        <div
          ref={popRef}
          id={listId}
          className="cron-dt-pop cron-dt-pop--date-only"
          role="dialog"
          aria-label={ariaLabel || t("cron.history.filterDate")}
          style={
            pos
              ? {
                  top: pos.top,
                  left: pos.left,
                  width: pos.width,
                  maxHeight: pos.maxHeight,
                  ...toneStyle,
                }
              : { visibility: "hidden", width: 248, ...toneStyle }
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
                    parts.hasValue &&
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
          {allowClear && parts.hasValue ? (
            <button
              type="button"
              className="cron-dt-clear"
              onClick={() => {
                onChange("");
                closePop();
              }}
            >
              <X size={13} strokeWidth={2.2} aria-hidden />
              {t("cron.history.clearDate")}
            </button>
          ) : null}
        </div>,
        document.body,
      )
    : null;

  return (
    <>
      <button
        ref={triggerRef}
        type="button"
        className={[
          "cron-sched-field-shell",
          "cron-history-date-btn",
          open ? "is-open" : "",
          parts.hasValue ? "" : "is-placeholder",
          className,
        ]
          .filter(Boolean)
          .join(" ")}
        aria-label={ariaLabel}
        title={ariaLabel}
        aria-haspopup="dialog"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((v) => !v)}
      >
        <CalendarDays size={14} strokeWidth={2.2} aria-hidden />
        <span className="cron-history-date-label">{label}</span>
      </button>
      {pop}
    </>
  );
}
