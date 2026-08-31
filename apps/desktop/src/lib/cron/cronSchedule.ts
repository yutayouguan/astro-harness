/** 定时表达式解析与人类可读描述。 */
export type ScheduleMode = "interval" | "daily" | "weekly" | "weekdays" | "custom" | "once";

export type Weekday = 0 | 1 | 2 | 3 | 4 | 5 | 6; // 0=Sun 与 cron 一致

export type ScheduleDraft = {
  mode: ScheduleMode;
  /** HH:mm 每天 */
  time?: string;
  weekdays: Weekday[]; // 空 = 每天
  intervalValue?: number;
  intervalUnit?: "m" | "h" | "d";
  /** 仅用于兼容已存在的单次任务；新建 UI 不再提供该模式。 */
  onceAt?: string;
};

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
    const [hh, mm] = (d.time ?? "09:00").split(":").map(Number);
    return `${mm} ${hh} * * *`;
  }
  if (d.mode === "interval") {
    const n = Math.max(1, d.intervalValue ?? 1);
    const unit = d.intervalUnit ?? "m";
    const weekdays = [...d.weekdays].sort((a, b) => a - b);
    return weekdays.length > 0 && weekdays.length < 7
      ? `every:${n}${unit};wd=${weekdays.join(",")}`
      : `every:${n}${unit}`;
  }
  if (d.mode === "once") {
    if (
      d.onceAt &&
      /^\d{4}-\d{2}-\d{2}T/.test(d.onceAt) &&
      /[+-]\d{2}:\d{2}$|Z$/.test(d.onceAt)
    ) {
      return `once:${d.onceAt}`;
    }
    return `once:${toRfc3339Local(d.onceAt)}`;
  }
  const [hh, mm] = (d.time ?? "09:00").split(":").map(Number);
  const weekdays =
    d.mode === "weekdays"
      ? ([1, 2, 3, 4, 5] as Weekday[])
      : d.mode === "weekly"
        ? [d.weekdays[0] ?? 1]
        : d.weekdays.length > 0
          ? d.weekdays
          : ([1, 2, 3, 4, 5] as Weekday[]);
  const wd = [...weekdays].sort((a, b) => a - b).join(",");
  return `${mm} ${hh} * * ${wd}`;
}

function toRfc3339Local(onceAt?: string): string {
  const date = onceAt ? new Date(onceAt) : new Date();
  if (Number.isNaN(date.getTime())) {
    return new Date().toISOString().replace(/\.\d{3}Z$/, "+00:00");
  }
  const pad = (value: number) => String(value).padStart(2, "0");
  const offsetMinutes = -date.getTimezoneOffset();
  const sign = offsetMinutes >= 0 ? "+" : "-";
  return [
    `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`,
    `T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`,
    `${sign}${pad(Math.floor(Math.abs(offsetMinutes) / 60))}:${pad(Math.abs(offsetMinutes) % 60)}`,
  ].join("");
}

export function formatScheduleLabel(schedule: string, locale: "zh" | "en"): string {
  const zh = locale === "zh";
  if (schedule.startsWith("once:")) {
    const raw = schedule.slice(5);
    const d = new Date(raw);
    if (!Number.isNaN(d.getTime())) {
      const text = d.toLocaleString(zh ? "zh-CN" : "en-US", {
        month: "2-digit",
        day: "2-digit",
        hour: "2-digit",
        minute: "2-digit",
      });
      return zh ? `单次 · ${text}` : `Once · ${text}`;
    }
    return zh ? `单次 ${raw}` : `Once ${raw}`;
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
  if (parts.length >= 5) {
    const mm = parts[0].padStart(2, "0");
    const hh = parts[1].padStart(2, "0");
    const wd = parts[4];
    if (wd === "*") {
      return zh ? `每天 ${hh}:${mm}` : `Daily ${hh}:${mm}`;
    }
    const days = wd.split(",");
    if (days.length === 5 && ["1", "2", "3", "4", "5"].every((d) => days.includes(d))) {
      return zh ? `工作日 ${hh}:${mm}` : `Weekdays ${hh}:${mm}`;
    }
    if (days.length === 6 && !days.includes("0")) {
      return zh ? `周一至周六 ${hh}:${mm}` : `Mon–Sat ${hh}:${mm}`;
    }
    return zh ? `${hh}:${mm} · 每周 ${days.length} 天` : `${hh}:${mm} · ${days.length} days/week`;
  }
  return schedule;
}

/** 把持久化 schedule 还原成编辑器草稿（尽量解析，失败则回退每天 09:00） */
export function decodeSchedule(schedule: string): ScheduleDraft {
  const raw = schedule.trim();
  if (raw.startsWith("once:")) {
    const iso = raw.slice(5);
    const d = new Date(iso);
    if (!Number.isNaN(d.getTime())) {
      const pad = (n: number) => String(n).padStart(2, "0");
      const onceAt = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}`;
      return { mode: "once", weekdays: [], onceAt };
    }
    return { mode: "once", weekdays: [], onceAt: iso.slice(0, 16) };
  }

  if (raw.startsWith("every:")) {
    const body = raw.slice(6);
    const [main, ...rest] = body.split(";");
    const m = main.match(/^(\d+)([mhd])$/i);
    const weekdays: Weekday[] = [];
    for (const part of rest) {
      if (part.startsWith("wd=")) {
        for (const segment of part.slice(3).split(",")) {
          const range = segment.match(/^(\d)-(\d)$/);
          if (range) {
            const lo = Number(range[1]);
            const hi = Number(range[2]);
            for (let value = lo; value <= hi; value += 1) {
              if (value >= 0 && value <= 6) weekdays.push(value as Weekday);
            }
            continue;
          }
          const value = Number(segment);
          if (value >= 0 && value <= 6) weekdays.push(value as Weekday);
        }
      }
    }
    return {
      mode: "interval",
      weekdays,
      intervalValue: m ? Math.max(1, Number(m[1])) : 1,
      intervalUnit:
        m && m[2].toLowerCase() === "d"
          ? "d"
          : m && m[2].toLowerCase() === "h"
            ? "h"
            : "m",
    };
  }

  const parts = raw.split(/\s+/);
  if (parts.length >= 5) {
    const mm = Number(parts[0]);
    const hh = Number(parts[1]);
    const wdRaw = parts[4];
    const weekdays: Weekday[] = [];
    if (wdRaw !== "*") {
      for (const seg of wdRaw.split(",")) {
        const range = seg.match(/^(\d)-(\d)$/);
        if (range) {
          const lo = Number(range[1]);
          const hi = Number(range[2]);
          for (let i = lo; i <= hi; i++) {
            if (i >= 0 && i <= 6) weekdays.push(i as Weekday);
          }
        } else {
          const v = Number(seg);
          if (v >= 0 && v <= 6) weekdays.push(v as Weekday);
        }
      }
    }
    const pad = (n: number) => String(n).padStart(2, "0");
    const allWeekdays =
      weekdays.length === 5 &&
      ([1, 2, 3, 4, 5] as Weekday[]).every((day) => weekdays.includes(day));
    const mode: ScheduleMode =
      wdRaw === "*"
        ? "daily"
        : allWeekdays
          ? "weekdays"
          : weekdays.length === 1
            ? "weekly"
            : "custom";
    return {
      mode,
      time: `${pad(Number.isFinite(hh) ? hh : 9)}:${pad(Number.isFinite(mm) ? mm : 0)}`,
      weekdays,
    };
  }

  return { mode: "daily", time: "09:00", weekdays: [] };
}
