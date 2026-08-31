/** 定时表达式编辑器。 */
import { type ComponentType, type ReactNode } from "react";
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
import type {
  CustomFrequency,
  ScheduleDraft,
  ScheduleMode,
  Weekday,
} from "../../lib/cron/cronSchedule";
import { UI_WEEKDAYS } from "../../lib/cron/cronSchedule";
import { useI18n } from "../../i18n/LocaleContext";
import type { MessageKey } from "../../i18n/messages";
import { SelectMenu } from "../ui/SelectMenu";
import { GlassTimePicker } from "./GlassTimePicker";

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
const CUSTOM_FREQUENCIES: {
  value: CustomFrequency;
  labelKey: MessageKey;
}[] = [
  { value: "hourly", labelKey: "cron.custom.hourly" },
  { value: "daily", labelKey: "cron.custom.daily" },
  { value: "weekly", labelKey: "cron.custom.weekly" },
  { value: "monthly", labelKey: "cron.custom.monthly" },
  { value: "yearly", labelKey: "cron.custom.yearly" },
];

function normalizedDraft(
  value: ScheduleDraft,
  mode: ScheduleMode,
): ScheduleDraft {
  if (mode === "daily") return { ...value, mode, weekdays: [] };
  if (mode === "weekdays") return { ...value, mode, weekdays: WORKDAYS };
  if (mode === "weekly") {
    return { ...value, mode, weekdays: [value.weekdays[0] ?? 1] };
  }
  if (mode === "custom") {
    return {
      ...value,
      mode,
      customFrequency: value.customFrequency ?? "weekly",
      customInterval: value.customInterval ?? 1,
      month: value.month ?? 1,
      monthDay: value.monthDay ?? 1,
      minute: value.minute ?? 0,
      weekdays: value.weekdays.length > 0 ? value.weekdays : [1],
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

  const chooseWeeklyDay = (day: Weekday) => {
    onChange({ ...value, weekdays: [day] });
  };

  const showTime = value.mode !== "interval" && value.mode !== "custom";

  return (
    <div className="cron-sched">
      <div className="cron-sched-label">
        <CalendarClock size={13} strokeWidth={2.2} aria-hidden />
        <span>{t("cron.field.schedule")}</span>
      </div>

      <div
        className="cron-sched-modes"
        role="tablist"
        aria-label={t("cron.field.schedule")}
      >
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
            <span className="cron-sched-interval-label">
              {t("cron.interval.every")}
            </span>
            <div
              className="cron-sched-stepper"
              role="group"
              aria-label={t("cron.interval.every")}
            >
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label={t("cron.interval.decrease")}
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
                onChange={(event) => {
                  const next = Number(event.target.value);
                  onChange({
                    ...value,
                    intervalValue:
                      Number.isFinite(next) && next >= 1 ? Math.floor(next) : 1,
                  });
                }}
              />
              <button
                type="button"
                className="cron-sched-stepper-btn"
                aria-label={t("cron.interval.increase")}
                onClick={() =>
                  onChange({ ...value, intervalValue: intervalValue + 1 })
                }
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
                {
                  value: "m",
                  label: t("cron.interval.m"),
                  icon: <Timer size={13} aria-hidden />,
                },
                {
                  value: "h",
                  label: t("cron.interval.h"),
                  icon: <Clock size={13} aria-hidden />,
                },
                {
                  value: "d",
                  label: t("cron.interval.d"),
                  icon: <CalendarDays size={13} aria-hidden />,
                },
              ]}
            />
          </div>
        </div>
      )}

      {value.mode === "custom" && (
        <CustomScheduleFields value={value} onChange={onChange} />
      )}

      {showTime && (
        <div className="cron-sched-body">
          {value.mode === "weekdays" && (
            <p className="cron-sched-summary">{t("cron.weekdays.summary")}</p>
          )}
          {value.mode === "weekly" && (
            <WeekdayChips
              selected={value.weekdays}
              onToggle={chooseWeeklyDay}
              t={t}
            />
          )}
          <GlassTimePicker
            value={value.time ?? "09:00"}
            onChange={(time) => onChange({ ...value, time })}
            aria-label={t("cron.time")}
          />
        </div>
      )}
    </div>
  );
}

function CustomScheduleFields({ value, onChange }: Props) {
  const { t, locale } = useI18n();
  const frequency = value.customFrequency ?? "weekly";
  const interval = Math.max(1, value.customInterval ?? 1);
  const month = Math.min(12, Math.max(1, value.month ?? 1));
  const maxDay = daysInMonth(month);
  const monthDay = Math.min(maxDay, Math.max(1, value.monthDay ?? 1));
  const unitKey = `cron.custom.unit.${frequency}` as MessageKey;

  const updateFrequency = (next: string) => {
    const customFrequency = CUSTOM_FREQUENCIES.some(
      ({ value }) => value === next,
    )
      ? (next as CustomFrequency)
      : "weekly";
    onChange({
      ...value,
      customFrequency,
      customInterval: interval,
      month,
      monthDay,
      minute: value.minute ?? 0,
      weekdays: value.weekdays.length > 0 ? value.weekdays : [1],
      time: value.time ?? "09:00",
    });
  };

  return (
    <div className="cron-sched-body cron-custom-fields">
      <ScheduleFieldRow label={t("cron.custom.repeat")}>
        <SelectMenu
          className="cron-custom-select"
          value={frequency}
          onChange={updateFrequency}
          aria-label={t("cron.custom.repeat")}
          options={CUSTOM_FREQUENCIES.map((option) => ({
            value: option.value,
            label: t(option.labelKey),
          }))}
        />
      </ScheduleFieldRow>

      <ScheduleFieldRow label={t("cron.custom.every")}>
        <div className="cron-custom-interval-control">
          <NumberStepper
            value={interval}
            label={t("cron.custom.every")}
            onChange={(customInterval) =>
              onChange({ ...value, customInterval })
            }
          />
          <span className="cron-custom-unit">{t(unitKey)}</span>
        </div>
      </ScheduleFieldRow>

      {frequency === "hourly" && (
        <ScheduleFieldRow label={t("cron.custom.atMinute")}>
          <SelectMenu
            className="cron-custom-select cron-custom-select--compact"
            value={String(value.minute ?? 0)}
            onChange={(minute) =>
              onChange({ ...value, minute: Number(minute) })
            }
            aria-label={t("cron.custom.atMinute")}
            options={Array.from({ length: 60 }, (_, minute) => ({
              value: String(minute),
              label: String(minute).padStart(2, "0"),
            }))}
            menuMaxHeight={280}
          />
        </ScheduleFieldRow>
      )}

      {frequency === "yearly" && (
        <ScheduleFieldRow label={t("cron.custom.inMonth")}>
          <SelectMenu
            className="cron-custom-select"
            value={String(month)}
            onChange={(rawMonth) => {
              const nextMonth = Number(rawMonth);
              onChange({
                ...value,
                month: nextMonth,
                monthDay: Math.min(value.monthDay ?? 1, daysInMonth(nextMonth)),
              });
            }}
            aria-label={t("cron.custom.inMonth")}
            options={Array.from({ length: 12 }, (_, index) => {
              const value = index + 1;
              return {
                value: String(value),
                label: new Intl.DateTimeFormat(
                  locale === "zh" ? "zh-CN" : "en-US",
                  {
                    month: "long",
                  },
                ).format(new Date(2024, index, 1)),
              };
            })}
          />
        </ScheduleFieldRow>
      )}

      {(frequency === "monthly" || frequency === "yearly") && (
        <ScheduleFieldRow label={t("cron.custom.onDay")}>
          <SelectMenu
            className="cron-custom-select cron-custom-select--compact"
            value={String(monthDay)}
            onChange={(day) => onChange({ ...value, monthDay: Number(day) })}
            aria-label={t("cron.custom.onDay")}
            options={Array.from(
              { length: frequency === "yearly" ? maxDay : 31 },
              (_, index) => ({
                value: String(index + 1),
                label: String(index + 1),
              }),
            )}
            menuMaxHeight={280}
          />
        </ScheduleFieldRow>
      )}

      {frequency === "weekly" && (
        <div className="cron-custom-weekday-field">
          <span className="cron-custom-row-label">
            {t("cron.custom.onWeekdays")}
          </span>
          <WeekdayChips
            selected={value.weekdays.length > 0 ? value.weekdays : [1]}
            onToggle={(day) => {
              const selected: Weekday[] =
                value.weekdays.length > 0 ? value.weekdays : [1];
              const has = selected.includes(day);
              if (has && selected.length === 1) return;
              onChange({
                ...value,
                weekdays: has
                  ? selected.filter((candidate) => candidate !== day)
                  : [...selected, day],
              });
            }}
            t={t}
          />
        </div>
      )}

      {frequency !== "hourly" && (
        <ScheduleFieldRow label={t("cron.time")}>
          <GlassTimePicker
            value={value.time ?? "09:00"}
            onChange={(time) => onChange({ ...value, time })}
            aria-label={t("cron.time")}
          />
        </ScheduleFieldRow>
      )}
    </div>
  );
}

function ScheduleFieldRow({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) {
  return (
    <div className="cron-custom-row">
      <span className="cron-custom-row-label">{label}</span>
      <div className="cron-custom-row-control">{children}</div>
    </div>
  );
}

function NumberStepper({
  value,
  label,
  onChange,
}: {
  value: number;
  label: string;
  onChange: (next: number) => void;
}) {
  return (
    <div className="cron-sched-stepper" role="group" aria-label={label}>
      <button
        type="button"
        className="cron-sched-stepper-btn"
        aria-label={`${label} -`}
        onClick={() => onChange(Math.max(1, value - 1))}
      >
        <Minus size={14} strokeWidth={2.4} aria-hidden />
      </button>
      <input
        type="number"
        min={1}
        max={999}
        className="cron-sched-input cron-sched-input--num cron-sched-input--bare"
        value={value}
        aria-label={label}
        onChange={(event) => {
          const next = Number(event.target.value);
          onChange(
            Number.isFinite(next) && next >= 1
              ? Math.min(999, Math.floor(next))
              : 1,
          );
        }}
      />
      <button
        type="button"
        className="cron-sched-stepper-btn"
        aria-label={`${label} +`}
        onClick={() => onChange(Math.min(999, value + 1))}
      >
        <Plus size={14} strokeWidth={2.4} aria-hidden />
      </button>
    </div>
  );
}

function daysInMonth(month: number): number {
  return new Date(2024, month, 0).getDate();
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
    <div
      className="cron-sched-weekdays"
      aria-label={t("cron.custom.chooseDays")}
    >
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
