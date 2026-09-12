import { MessageCircle, ArrowRight } from "lucide-react";
import { Button } from "../ui/Button";

export function FirstMeetingNotice({
  locale,
  busy,
  error,
  canStart,
  started,
  onStart,
  onDefer,
}: {
  locale: string;
  busy: boolean;
  error: boolean;
  canStart: boolean;
  started: boolean;
  onStart: () => void;
  onDefer: () => void;
}) {
  const zh = locale === "zh";
  return (
    <aside
      className="first-meeting-notice"
      aria-label={zh ? "初次见面" : "First meeting"}
    >
      <MessageCircle size={20} aria-hidden />
      <div className="first-meeting-notice-copy">
        <strong>
          {zh ? "你好，我们认识一下" : "Hello, let's get to know each other"}
        </strong>
        <p role={error ? "status" : undefined}>
          {error
            ? zh
              ? "暂时没有开始。可以重试，也可以直接做任务；不会自动反复发送。"
              : "We couldn't start yet. Retry or go straight to a task; nothing is resent automatically."
            : zh
              ? "在聊天里选择称呼和偏好，确认后才记住。也可以先告诉我想完成什么。"
              : "Choose a name and preferences in chat. I'll ask before remembering them. Or start with a task."}
        </p>
      </div>
      <div className="first-meeting-notice-actions">
        <Button size="sm" variant="ghost" disabled={busy} onClick={onDefer}>
          {zh ? "先做任务" : "Task first"}
        </Button>
        <Button size="sm" disabled={busy || !canStart} onClick={onStart}>
          {busy
            ? zh
              ? "正在准备"
              : "Preparing"
            : started
              ? zh
                ? "继续认识"
                : "Continue"
              : zh
                ? "认识一下"
                : "Let's meet"}
          <ArrowRight size={14} aria-hidden />
        </Button>
      </div>
    </aside>
  );
}
