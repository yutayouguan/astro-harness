import type { OnboardingStateDto } from "./onboarding.ts";

export type FirstMeetingState = NonNullable<
  OnboardingStateDto["first_meeting"]
>;
export const FIRST_MEETING_SKILL = "first-meeting";

export function firstMeetingPrompt(locale: string): string {
  return `/${FIRST_MEETING_SKILL} ${
    locale === "zh"
      ? "你好，我们第一次见面。先介绍一下你自己，再用几个容易选择的问题认识我；我也可以随时直接开始任务。需要长期记住的内容，请先让我确认。"
      : "Hello, this is our first meeting. Introduce yourself, then get to know me with a few easy choices. I may start a task at any time. Ask me to confirm anything you want to remember."
  }`;
}

export function canAutoStartFirstMeeting({
  meeting,
  ready,
  providerReady,
  empty,
  input,
  busy,
}: {
  meeting: FirstMeetingState | null;
  ready: boolean;
  providerReady: boolean;
  empty: boolean;
  input: string;
  busy: boolean;
}): boolean {
  return (
    meeting?.status === "pending" &&
    ready &&
    providerReady &&
    empty &&
    !input.trim() &&
    !busy
  );
}
