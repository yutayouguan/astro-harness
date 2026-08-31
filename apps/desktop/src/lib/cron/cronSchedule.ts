/** 定时表达式解析与人类可读描述。 */
export type ScheduleMode =
  "interval" | "daily" | "weekly" | "weekdays" | "custom";
export type CustomFrequency =
  "hourly" | "daily" | "weekly" | "monthly" | "yearly";

export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6; // 0=Sun 与 cron 一致

export type ScheduleDraft = {
  mode: ScheduleMode;
  /** HH:mm 每天 */
  time?: string;
  weekdays: Weekday[]; // 空 = 每天
  intervalValue?: number;
  intervalUnit?: "m" | "h" | "d";
  customFrequency?: CustomFrequency;
  customInterval?: number;
  minute?: number;
  month?: number;
  monthDay?: number;
  /** 自定义周期的本地墙钟相位，格式 YYYY-MM-DDTHH:mm。 */
  start?: string;
};

const CUSTOM_INTERVAL_MAX = 999;

function boundedInteger(
  value: number | undefined,
  min: number,
  max: number,
  fallback: number,
): number {
  if (!Number.isFinite(value)) return fallback;
  return Math.min(max, Math.max(min, Math.floor(value as number)));
}

function normalizeClockTime(value: string | undefined, fallback = "09:00") {
  const match = /^(\d{1,2}):(\d{2})$/.exec(value?.trim() ?? "");
  if (!match) return fallback;
  const hour = Number(match[1]);
  const minute = Number(match[2]);
  if (hour > 23 || minute > 59) return fallback;
  return `${String(hour).padStart(2, "0")}:${String(minute).padStart(2, "0")}`;
}

function localStartStamp(date = new Date()): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}T${String(date.getHours()).padStart(2, "0")}:${String(date.getMinutes()).padStart(2, "0")}`;
}

function normalizeStart(value: string | undefined): string | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/.exec(
    value?.trim() ?? "",
  );
  if (!match) return null;
  const [year, month, day, hour, minute] = match.slice(1).map(Number);
  const candidate = new Date(year, month - 1, day, hour, minute);
  if (
    candidate.getFullYear() !== year ||
    candidate.getMonth() !== month - 1 ||
    candidate.getDate() !== day ||
    candidate.getHours() !== hour ||
    candidate.getMinutes() !== minute
  ) {
    return null;
  }
  return match[0];
}

function maxDayForYearlyMonth(month: number): number {
  return new Date(2024, month, 0).getDate();
}

function normalizedWeekdays(
  days: Weekday[],
  fallback: Weekday[] = [],
): Weekday[] {
  const unique = [...new Set(days.filter((day) => day >= 0 && day <= 6))];
  return unique.length > 0 ? unique.sort((a, b) => a - b) : fallback;
}

function parseWeekdayExpression(raw: string): Weekday[] | null {
  if (!raw.trim()) return null;
  const weekdays: Weekday[] = [];
  for (const rawSegment of raw.split(",")) {
    const segment = rawSegment.trim();
    const range = segment.match(/^([0-6])-([0-6])$/);
    if (range) {
      const lo = Number(range[1]);
      const hi = Number(range[2]);
      if (lo > hi) return null;
      for (let value = lo; value <= hi; value += 1) {
        if (!weekdays.includes(value as Weekday)) {
          weekdays.push(value as Weekday);
        }
      }
      continue;
    }
    if (!/^[0-6]$/.test(segment)) return null;
    const value = Number(segment) as Weekday;
    if (!weekdays.includes(value)) weekdays.push(value);
  }
  return weekdays.length > 0 ? weekdays : null;
}

function safeDefaultDraft(): ScheduleDraft {
  return { mode: "daily", time: "09:00", weekdays: [] };
}

/** 周一=1 … 周日=0 的 UI 顺序映射到 Weekday */
export const UI_WEEKDAYS: { labelKey: string; value: Weekday }[] = [
  { labelKey: "cron.wd.mon", value: 1 },
  { labelKey: "cron.wd.tue", value: 2 },
  { labelKey: "cron.wd.wed", value: 3 },
  { labelKey: "cron.wd.thu", value: 4 },
  { labelKey: "cron.wd.fri", value: 5 },
  { labelKey: "cron.wd.sat", value: 6 },
  { labelKey: "cron.wd.sun", value: 0 },
];

export function encodeSchedule(d: ScheduleDraft): string {
  if (d.mode === "daily") {
    const [hh, mm] = normalizeClockTime(d.time).split(":").map(Number);
    return `${mm} ${hh} * * *`;
  }
  if (d.mode === "interval") {
    const n = boundedInteger(d.intervalValue, 1, Number.MAX_SAFE_INTEGER, 1);
    const unit = d.intervalUnit ?? "m";
    const weekdays = normalizedWeekdays(d.weekdays);
    return weekdays.length > 0 && weekdays.length < 7
      ? `every:${n}${unit};wd=${weekdays.join(",")}`
      : `every:${n}${unit}`;
  }
  if (d.mode === "custom") {
    const frequency = d.customFrequency ?? "weekly";
    const every = boundedInteger(d.customInterval, 1, CUSTOM_INTERVAL_MAX, 1);
    const start = normalizeStart(d.start) ?? localStartStamp();
    if (frequency === "hourly") {
      const minute = boundedInteger(d.minute, 0, 59, 0);
      return `custom:hourly;every=${every};minute=${minute};start=${start}`;
    }
    const time = normalizeClockTime(d.time);
    if (frequency === "daily")
      return `custom:daily;every=${every};time=${time};start=${start}`;
    if (frequency === "weekly") {
      const weekdays = normalizedWeekdays(d.weekdays, [1]);
      return `custom:weekly;every=${every};wd=${weekdays.join(",")};time=${time};start=${start}`;
    }
    const day = boundedInteger(d.monthDay, 1, 31, 1);
    if (frequency === "monthly") {
      return `custom:monthly;every=${every};day=${day};time=${time};start=${start}`;
    }
    const month = boundedInteger(d.month, 1, 12, 1);
    const yearlyDay = Math.min(day, maxDayForYearlyMonth(month));
    return `custom:yearly;every=${every};month=${month};day=${yearlyDay};time=${time};start=${start}`;
  }
  const [hh, mm] = normalizeClockTime(d.time).split(":").map(Number);
  const weekdays =
    d.mode === "weekdays"
      ? ([1, 2, 3, 4, 5] as Weekday[])
      : d.mode === "weekly"
        ? [d.weekdays[0] ?? 1]
        : d.weekdays.length > 0
          ? d.weekdays
          : ([1, 2, 3, 4, 5] as Weekday[]);
  const wd = normalizedWeekdays(weekdays, [1]).join(",");
  return `${mm} ${hh} * * ${wd}`;
}

export function formatScheduleLabel(
  schedule: string,
  locale: "zh" | "en",
): string {
  const zh = locale === "zh";
  if (schedule.startsWith("custom:")) {
    const custom = decodeCustomSchedule(schedule);
    if (custom) {
      const every = custom.customInterval ?? 1;
      const unit = custom.customFrequency ?? "weekly";
      const names = zh
        ? {
            hourly: "小时",
            daily: "天",
            weekly: "周",
            monthly: "月",
            yearly: "年",
          }
        : {
            hourly: "hour",
            daily: "day",
            weekly: "week",
            monthly: "month",
            yearly: "year",
          };
      const prefix = zh
        ? every === 1
          ? `每${names[unit]}`
          : `每 ${every} ${names[unit]}`
        : `Every ${every} ${names[unit]}${every === 1 ? "" : "s"}`;
      if (unit === "hourly") {
        return `${prefix} · ${zh ? "第" : "at minute"} ${String(custom.minute ?? 0).padStart(2, "0")} ${zh ? "分" : ""}`.trim();
      }
      if (unit === "yearly") {
        const date = zh
          ? `${custom.month ?? 1}月${custom.monthDay ?? 1}日`
          : `${custom.month ?? 1}/${custom.monthDay ?? 1}`;
        return `${prefix} · ${date} ${custom.time ?? "09:00"}`;
      }
      if (unit === "monthly") {
        return `${prefix} · ${zh ? `${custom.monthDay ?? 1}日` : `day ${custom.monthDay ?? 1}`} ${custom.time ?? "09:00"}`;
      }
      return `${prefix} · ${custom.time ?? "09:00"}`;
    }
  }
  if (schedule.startsWith("every:")) {
    const body = schedule.slice(6);
    const [main, ...rest] = body.split(";");
    const m = main.match(/^(\d+)([mhd])$/i);
    let base = zh ? `间隔 ${body}` : `Every ${body}`;
    if (m) {
      const n = m[1];
      const unit =
        m[2].toLowerCase() === "d"
          ? zh
            ? "天"
            : "d"
          : m[2].toLowerCase() === "h"
            ? zh
              ? "小时"
              : "h"
            : zh
              ? "分钟"
              : "m";
      base = zh ? `每 ${n} ${unit}` : `Every ${n}${unit}`;
    }
    const wd = rest.find((p) => p.startsWith("wd="));
    if (wd) {
      base += zh ? " · 部分工作日" : " · weekdays";
    }
    return base;
  }

  const parts = schedule.trim().split(/\s+/);
  if (parts.length === 5) {
    const minute = Number(parts[0]);
    const hour = Number(parts[1]);
    const validMinute = Number.isInteger(minute) && minute >= 0 && minute <= 59;
    const validHour = Number.isInteger(hour) && hour >= 0 && hour <= 23;
    const mm = validMinute ? String(minute).padStart(2, "0") : parts[0];
    const hh = validHour ? String(hour).padStart(2, "0") : parts[1];
    const day = parts[2];
    const month = parts[3];
    const wd = parts[4];
    const dayValue = Number(day);
    const monthValue = Number(month);
    const validDay = /^\d+$/.test(day) && dayValue >= 1 && dayValue <= 31;
    const validMonth =
      /^\d+$/.test(month) && monthValue >= 1 && monthValue <= 12;
    if (
      validMinute &&
      parts[1] === "*" &&
      day === "*" &&
      month === "*" &&
      wd === "*"
    ) {
      return zh ? `每小时第 ${mm} 分` : `Hourly at minute ${mm}`;
    }
    if (
      validMinute &&
      validHour &&
      validDay &&
      validMonth &&
      dayValue <= maxDayForYearlyMonth(monthValue) &&
      wd === "*"
    ) {
      return zh
        ? `每年 ${monthValue}月${dayValue}日 ${hh}:${mm}`
        : `Yearly ${monthValue}/${dayValue} ${hh}:${mm}`;
    }
    if (validMinute && validHour && validDay && month === "*" && wd === "*") {
      return zh
        ? `每月 ${dayValue}日 ${hh}:${mm}`
        : `Monthly on day ${dayValue} at ${hh}:${mm}`;
    }
    if (
      validMinute &&
      validHour &&
      day === "*" &&
      month === "*" &&
      wd === "*"
    ) {
      return zh ? `每天 ${hh}:${mm}` : `Daily ${hh}:${mm}`;
    }
    if (!validMinute || !validHour || day !== "*" || month !== "*")
      return schedule;
    const parsedDays = parseWeekdayExpression(wd);
    if (!parsedDays) return schedule;
    const days = parsedDays.map(String);
    if (
      days.length === 5 &&
      ["1", "2", "3", "4", "5"].every((d) => days.includes(d))
    ) {
      return zh ? `工作日 ${hh}:${mm}` : `Weekdays ${hh}:${mm}`;
    }
    if (days.length === 6 && !days.includes("0")) {
      return zh ? `周一至周六 ${hh}:${mm}` : `Mon–Sat ${hh}:${mm}`;
    }
    return zh
      ? `${hh}:${mm} · 每周 ${days.length} 天`
      : `${hh}:${mm} · ${days.length} days/week`;
  }
  return schedule;
}

/** 把持久化 schedule 还原成编辑器草稿（尽量解析，失败则回退每天 09:00） */
export function decodeSchedule(schedule: string): ScheduleDraft {
  const raw = schedule.trim();
  const custom = decodeCustomSchedule(raw);
  if (custom) return custom;
  if (raw.startsWith("every:")) {
    const body = raw.slice(6);
    const [main, ...rest] = body.split(";");
    const m = main.match(/^(\d+)([mhd])$/i);
    if (!m) return safeDefaultDraft();
    const intervalValue = Number(m[1]);
    if (!Number.isSafeInteger(intervalValue) || intervalValue < 1) {
      return safeDefaultDraft();
    }
    const weekdays: Weekday[] = [];
    let sawWeekdays = false;
    for (const part of rest) {
      if (!part.startsWith("wd=") || sawWeekdays) return safeDefaultDraft();
      sawWeekdays = true;
      const parsed = parseWeekdayExpression(part.slice(3));
      if (!parsed) return safeDefaultDraft();
      weekdays.push(...parsed);
    }
    return {
      mode: "interval",
      weekdays,
      intervalValue,
      intervalUnit:
        m[2].toLowerCase() === "d"
          ? "d"
          : m[2].toLowerCase() === "h"
            ? "h"
            : "m",
    };
  }

  const parts = raw.split(/\s+/);
  if (parts.length === 5) {
    const minuteRaw = parts[0];
    const hourRaw = parts[1];
    const dayRaw = parts[2];
    const monthRaw = parts[3];
    const mm = Number(minuteRaw);
    const hh = Number(hourRaw);
    const wdRaw = parts[4];
    const weekdays = wdRaw === "*" ? [] : parseWeekdayExpression(wdRaw);
    if (weekdays === null) return safeDefaultDraft();
    const pad = (n: number) => String(n).padStart(2, "0");
    const validMinute = Number.isInteger(mm) && mm >= 0 && mm <= 59;
    const validHour = Number.isInteger(hh) && hh >= 0 && hh <= 23;
    if (
      hourRaw === "*" &&
      validMinute &&
      dayRaw === "*" &&
      monthRaw === "*" &&
      wdRaw === "*"
    ) {
      return {
        mode: "custom",
        customFrequency: "hourly",
        customInterval: 1,
        minute: mm,
        weekdays: [],
      };
    }
    if (!validMinute || !validHour) return safeDefaultDraft();
    const time = `${pad(hh)}:${pad(mm)}`;
    if (dayRaw !== "*" && monthRaw !== "*") {
      const day = Number(dayRaw);
      const month = Number(monthRaw);
      if (
        !/^\d+$/.test(dayRaw) ||
        !/^\d+$/.test(monthRaw) ||
        day < 1 ||
        month < 1 ||
        month > 12 ||
        day > maxDayForYearlyMonth(month)
      ) {
        return safeDefaultDraft();
      }
      return {
        mode: "custom",
        customFrequency: "yearly",
        customInterval: 1,
        month,
        monthDay: day,
        time,
        weekdays: [],
      };
    }
    if (dayRaw !== "*") {
      const day = Number(dayRaw);
      if (monthRaw !== "*" || !/^\d+$/.test(dayRaw) || day < 1 || day > 31) {
        return safeDefaultDraft();
      }
      return {
        mode: "custom",
        customFrequency: "monthly",
        customInterval: 1,
        monthDay: day,
        time,
        weekdays: [],
      };
    }
    if (monthRaw !== "*") return safeDefaultDraft();
    const allWorkdays =
      weekdays.length === 5 &&
      ([1, 2, 3, 4, 5] as Weekday[]).every((day) => weekdays.includes(day));
    const allWeekdays = weekdays.length === 7;
    const mode: ScheduleMode =
      wdRaw === "*" || allWeekdays
        ? "daily"
        : allWorkdays
          ? "weekdays"
          : weekdays.length === 1
            ? "weekly"
            : "custom";
    return {
      mode,
      time,
      weekdays,
      ...(mode === "custom"
        ? { customFrequency: "weekly" as const, customInterval: 1 }
        : {}),
    };
  }

  return safeDefaultDraft();
}

function decodeCustomSchedule(raw: string): ScheduleDraft | null {
  if (!raw.startsWith("custom:")) return null;
  const [head, ...segments] = raw.split(";");
  const frequency = head.slice(7).trim() as CustomFrequency;
  if (!["hourly", "daily", "weekly", "monthly", "yearly"].includes(frequency)) {
    return null;
  }
  const fields = new Map<string, string>();
  const allowedFields = new Set([
    "every",
    "minute",
    "time",
    "wd",
    "day",
    "month",
    "start",
  ]);
  for (const segment of segments) {
    const [rawKey, rawValue] = segment.split("=", 2);
    const key = rawKey?.trim();
    const value = rawValue?.trim();
    if (!key || !value || !allowedFields.has(key) || fields.has(key))
      return null;
    fields.set(key, value);
  }
  const requiredFields: Record<CustomFrequency, string[]> = {
    hourly: ["every", "minute"],
    daily: ["every", "time"],
    weekly: ["every", "wd", "time"],
    monthly: ["every", "day", "time"],
    yearly: ["every", "month", "day", "time"],
  };
  if (requiredFields[frequency].some((field) => !fields.has(field)))
    return null;
  if (
    [...fields.keys()].some(
      (field) =>
        field !== "start" && !requiredFields[frequency].includes(field),
    )
  ) {
    return null;
  }
  const everyRaw = fields.get("every") ?? "";
  if (!/^\d+$/.test(everyRaw)) return null;
  const every = Number(everyRaw);
  if (!Number.isInteger(every) || every < 1 || every > CUSTOM_INTERVAL_MAX)
    return null;
  const draft: ScheduleDraft = {
    mode: "custom",
    customFrequency: frequency,
    customInterval: every,
    weekdays: [],
  };
  const startRaw = fields.get("start");
  if (startRaw) {
    const start = normalizeStart(startRaw);
    if (!start) return null;
    draft.start = start;
  }
  if (frequency === "hourly") {
    const minuteRaw = fields.get("minute") ?? "";
    if (!/^\d+$/.test(minuteRaw)) return null;
    const minute = Number(minuteRaw);
    if (minute < 0 || minute > 59) return null;
    draft.minute = minute;
    return draft;
  }
  const timeRaw = fields.get("time");
  const time = normalizeClockTime(timeRaw, "");
  if (!time) return null;
  draft.time = time;
  if (frequency === "weekly") {
    const weekdays = parseWeekdayExpression(fields.get("wd") ?? "");
    if (!weekdays) return null;
    draft.weekdays = normalizedWeekdays(weekdays);
  }
  if (frequency === "monthly" || frequency === "yearly") {
    const dayRaw = fields.get("day") ?? "";
    if (!/^\d+$/.test(dayRaw)) return null;
    const day = Number(dayRaw);
    if (day < 1 || day > 31) return null;
    draft.monthDay = day;
  }
  if (frequency === "yearly") {
    const monthRaw = fields.get("month") ?? "";
    if (!/^\d+$/.test(monthRaw)) return null;
    const month = Number(monthRaw);
    if (
      month < 1 ||
      month > 12 ||
      (draft.monthDay ?? 1) > maxDayForYearlyMonth(month)
    ) {
      return null;
    }
    draft.month = month;
  }
  return draft;
}
