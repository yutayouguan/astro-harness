export type MemoryTimelineItem = {
  date: string;
  hasDiary: boolean;
  hasDream: boolean;
  diaryAgentCount: number;
};

/** 统计一个月份中的记录数；同一天不同专家的记录分别计数。 */
export function countMonthEntries(
  dateGroups: Iterable<readonly string[]>,
  monthPrefix: string,
): number {
  let total = 0;
  for (const dates of dateGroups) {
    total += dates.filter((date) => date.startsWith(monthPrefix)).length;
  }
  return total;
}

/** 合并当月的日记与做梦日期，生成由近到远的展示轨迹。 */
export function buildMonthTimeline({
  monthPrefix,
  diaryDates,
  dreamDates,
  diaryDatesByAgent,
  includeAgentCount,
}: {
  monthPrefix: string;
  diaryDates: ReadonlySet<string>;
  dreamDates: ReadonlySet<string>;
  diaryDatesByAgent: Readonly<Record<string, readonly string[]>>;
  includeAgentCount: boolean;
}): MemoryTimelineItem[] {
  const dates = new Set<string>();
  for (const date of diaryDates) {
    if (date.startsWith(monthPrefix)) dates.add(date);
  }
  for (const date of dreamDates) {
    if (date.startsWith(monthPrefix)) dates.add(date);
  }

  return Array.from(dates)
    .sort((a, b) => b.localeCompare(a))
    .map((date) => ({
      date,
      hasDiary: diaryDates.has(date),
      hasDream: dreamDates.has(date),
      diaryAgentCount: includeAgentCount
        ? Object.values(diaryDatesByAgent).filter((agentDates) =>
            agentDates.includes(date),
          ).length
        : diaryDates.has(date)
          ? 1
          : 0,
    }));
}
