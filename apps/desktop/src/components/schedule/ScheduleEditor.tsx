/** 定时表达式编辑器。 */
import { type ComponentType } from "react";
import {
  BriefcaseBusiness,
  CalendarClock,
  CalendarDays,
  CalendarRange,
  Clock,
  Minus,
  Plus,
  SlidersHorizontal,
  Timer,
  type LucideProps,
} from "lucide-react";
import type { ScheduleDraft, ScheduleMode, Weekday } from "../../lib/cron/cronSchedule";
import { UI_WEEKDAYS } from "../../lib/cron/cronSchedule";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { SelectMenu } from "../ui/SelectMenu";

type Props = {
  value: ScheduleDraft;
  onChange: (next: ScheduleDraft) => void;
};

type ModeIcon = ComponentType<LucideProps>;

const MODES: { mode: ScheduleMode; labelKey: MessageKey; Icon: ModeIcon }[] = [
  { mode: "interval", labelKey: "cron.mode.interval", Icon: Timer },
  { mode: "daily", labelKey: "cron.mode.daily", Icon: CalendarDays },
  { mode: "weekly", labelKey: "cron.mode.weekly", Icon: CalendarRange },
  { mode: "weekdays", labelKey: "cron.mode.weekdays", Icon: BriefcaseBusiness },
  { mode: "custom", labelKey: "cron.mode.custom", Icon: SlidersHorizontal },
];

const WORKDAYS: Weekday[] = [1, 2, 3, 4, 5];

function normalizedDraft(value: ScheduleDraft, mode: ScheduleMode): ScheduleDraft {
  if (mode === "daily") return { ...value, mode, weekdays: [] };
  if (mode === "weekdays") return { ...value, mode, weekdays: WORKDAYS };
  if (mode === "weekly") {
    return { ...value, mode, weekdays: [value.weekdays[0] ?? 1] };
  }
  if (mode === "custom") {
    return {
      ...value,
      mode,
      weekdays: value.weekdays.length > 0 ? value.weekdays : WORKDAYS,
    };
  }
  return {
    ...value,
    mode,
    weekdays: [],
    intervalValue: value.intervalValue ?? 1,
    intervalUnit: value.intervalUnit ?? "h",
  };
}

export function ScheduleEditor({ value, onChange }: Props) {
  const { t } = useI18n();
  const intervalValue = value.intervalValue ?? 1;

  const setMode = (mode: ScheduleMode) => {
    if (mode !== value.mode) onChange(normalizedDraft(value, mode));
  };

  const toggleCustomWeekday = (day: Weekday) => {
    const has = value.weekdays.includes(day);
    if (has && value.weekdays.length === 1) return;
    onChange({
      ...value,
      weekdays: has
        ? value.weekdays.filter((candidate) => candidate !== day)
        : [...value.weekdays, day],
    });
  };

  const chooseWeeklyDay = (day: Weekday) => {
    onChange({ ...value, weekdays: [day] });
  };

  const showTime = value.mode !== "interval";

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

      {value.mode === "interval" && (
        <div className="cron-sched-body">
          <div className="cron-sched-row cron-sched-interval">
            <span className="cron-sched-interval-label">{t("cron.interval.every")}</span>
            <div className="cron-sched-stepper" role="group" aria-label={t("cron.interval.every")}>
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label={t("cron.interval.decrease")}
                onClick={() => onChange({ ...value, intervalValue: Math.max(1, intervalValue - 1) })}
              >
                <Minus size={14} strokeWidth={2.4} aria-hidden />
              </button>
              <input
                type="number"
                min={1}
                className="cron-sched-input cron-sched-input--num cron-sched-input--bare"
                value={intervalValue}
                onChange={(event) => {
                  const next = Number(event.target.value);
                  onChange({
                    ...value,
                    intervalValue: Number.isFinite(next) && next >= 1 ? Math.floor(next) : 1,
                  });
                }}
              />
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label={t("cron.interval.increase")}
                onClick={() => onChange({ ...value, intervalValue: intervalValue + 1 })}
              >
                <Plus size={14} strokeWidth={2.4} aria-hidden />
              </button>
            </div>
            <SelectMenu
              className="cron-sched-unit-menu"
              size="sm"
              value={value.intervalUnit ?? "h"}
              onChange={(unit) =>
                onChange({
                  ...value,
                  intervalUnit: unit === "d" ? "d" : unit === "h" ? "h" : "m",
                })
              }
              aria-label={t("cron.interval.unit")}
              options={[
                { value: "m", label: t("cron.interval.m"), icon: <Timer size={13} aria-hidden /> },
                { value: "h", label: t("cron.interval.h"), icon: <Clock size={13} aria-hidden /> },
                { value: "d", label: t("cron.interval.d"), icon: <CalendarDays size={13} aria-hidden /> },
              ]}
            />
          </div>
        </div>
      )}

      {showTime && (
        <div className="cron-sched-body">
          {value.mode === "weekdays" && (
            <p className="cron-sched-summary">{t("cron.weekdays.summary")}</p>
          )}
          {value.mode === "weekly" && (
            <WeekdayChips selected={value.weekdays} onToggle={chooseWeeklyDay} t={t} />
          )}
          {value.mode === "custom" && (
            <>
              <p className="cron-sched-summary">{t("cron.custom.chooseDays")}</p>
              <WeekdayChips selected={value.weekdays} onToggle={toggleCustomWeekday} t={t} />
            </>
          )}
          <label className="cron-sched-field-shell cron-sched-field-shell--time">
            <Clock size={14} strokeWidth={2.2} aria-hidden />
            <span className="sr-only">{t("cron.time")}</span>
            <input
              type="time"
              className="cron-sched-input cron-sched-input--bare"
              value={value.time ?? "09:00"}
              onChange={(event) => onChange({ ...value, time: event.target.value })}
            />
          </label>
        </div>
      )}
    </div>
  );
}

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
    <div className="cron-sched-weekdays" aria-label={t("cron.custom.chooseDays")}>
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
