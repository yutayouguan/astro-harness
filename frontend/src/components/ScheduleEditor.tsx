/** 调度表达式编辑器。 */
import type { ScheduleDraft, ScheduleMode, Weekday } from "../lib/cronSchedule";
import { UI_WEEKDAYS } from "../lib/cronSchedule";
import { useI18n } from "../i18n/LocaleContext";
import type { MessageKey } from "../i18n/messages";

/** 调度表达式编辑器入参 */
type Props = {
  value: ScheduleDraft;
  onChange: (next: ScheduleDraft) => void;
};

const MODES: { mode: ScheduleMode; labelKey: MessageKey }[] = [
  { mode: "daily", labelKey: "cron.mode.daily" },
  { mode: "interval", labelKey: "cron.mode.interval" },
  { mode: "once", labelKey: "cron.mode.once" },
];

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

  return (
    <div className="cron-sched">
      <div className="cron-sched-label">{t("cron.field.schedule")}</div>

      <div className="cron-sched-modes" role="tablist" aria-label={t("cron.field.schedule")}>
        {MODES.map(({ mode, labelKey }) => (
          <button
            key={mode}
            type="button"
            role="tab"
            aria-selected={value.mode === mode}
            className={`cron-sched-mode${value.mode === mode ? " active" : ""}`}
            onClick={() => setMode(mode)}
          >
            {t(labelKey)}
          </button>
        ))}
      </div>

      {value.mode === "daily" && (
        <div className="cron-sched-body">
          <div className="cron-sched-row">
            <input
              type="time"
              className="cron-sched-input"
              value={value.time ?? "09:00"}
              onChange={(e) => onChange({ ...value, time: e.target.value })}
            />
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
            <input
              type="number"
              min={1}
              className="cron-sched-input cron-sched-input--num"
              value={value.intervalValue ?? 1}
              onChange={(e) => {
                const n = Number(e.target.value);
                onChange({
                  ...value,
                  intervalValue: Number.isFinite(n) && n >= 1 ? Math.floor(n) : 1,
                });
              }}
            />
            <select
              className="cron-sched-select"
              value={value.intervalUnit ?? "m"}
              onChange={(e) =>
                onChange({
                  ...value,
                  intervalUnit: e.target.value === "h" ? "h" : "m",
                })
              }
            >
              <option value="m">{t("cron.interval.m")}</option>
              <option value="h">{t("cron.interval.h")}</option>
            </select>
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
          <div className="cron-sched-row">
            <input
              type="datetime-local"
              className="cron-sched-input cron-sched-input--datetime"
              value={value.onceAt ?? ""}
              onChange={(e) => onChange({ ...value, onceAt: e.target.value })}
            />
          </div>
        </div>
      )}
    </div>
  );
}

/** 星期多选芯片 */
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
