/** 统一短暂 Toast：进度条倒计时 + 可点关闭；支持 tone / 图标。 */
import {
  createElement,
  useCallback,
  useMemo,
  useState,
  type ReactElement,
} from "react";
import {
  Toast,
  TOAST_DURATION_MS,
  TOAST_ERROR_DURATION_MS,
  type ToastTone,
} from "../components/ui/Toast";

export type ShowToastOptions = {
  sticky?: boolean;
  durationMs?: number;
  /** 语义色 + 图标；未指定时由 `error` 推断 */
  tone?: ToastTone;
  /** 错误类：更长展示时间；等价于 tone: "error"（若未显式传 tone） */
  error?: boolean;
};

function resolveTone(opts?: ShowToastOptions): ToastTone {
  if (opts?.tone) return opts.tone;
  if (opts?.error) return "error";
  return "info";
}

export function useTransientToast(): {
  showToast: (msg: string, opts?: ShowToastOptions) => void;
  dismissToast: () => void;
  toastHost: ReactElement;
} {
  const [message, setMessage] = useState("");
  const [visible, setVisible] = useState(false);
  const [epoch, setEpoch] = useState(0);
  const [sticky, setSticky] = useState(false);
  const [tone, setTone] = useState<ToastTone>("info");
  const [durationMs, setDurationMs] = useState(TOAST_DURATION_MS);

  const dismissToast = useCallback(() => {
    setVisible(false);
    setSticky(false);
  }, []);

  const showToast = useCallback((msg: string, opts?: ShowToastOptions) => {
    const text = msg.trim();
    if (!text) return;
    const isSticky = Boolean(opts?.sticky);
    const nextTone = resolveTone(opts);
    const isErrorLike = nextTone === "error" || nextTone === "warning";
    setMessage(text);
    setSticky(isSticky);
    setTone(nextTone);
    setDurationMs(
      opts?.durationMs ??
        (isSticky || isErrorLike || opts?.error
          ? TOAST_ERROR_DURATION_MS
          : TOAST_DURATION_MS),
    );
    setEpoch((n) => n + 1);
    setVisible(true);
  }, []);

  const toastHost = useMemo(
    () =>
      createElement(Toast, {
        key: epoch,
        message,
        visible,
        sticky,
        tone,
        durationMs: sticky ? undefined : durationMs,
        onDismiss: dismissToast,
      }),
    [epoch, message, visible, sticky, tone, durationMs, dismissToast],
  );

  return { showToast, dismissToast, toastHost };
}
