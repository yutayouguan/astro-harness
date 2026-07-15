/** 统一短暂 Toast：进度条倒计时 + 可点关闭。 */
import { useCallback, useMemo, useState, type ReactElement } from "react";
import {
  Toast,
  TOAST_DURATION_MS,
  TOAST_ERROR_DURATION_MS,
} from "../components/Toast";

export type ShowToastOptions = {
  sticky?: boolean;
  durationMs?: number;
  /** 错误类：更长展示时间 */
  error?: boolean;
};

export function useTransientToast(): {
  showToast: (msg: string, opts?: ShowToastOptions) => void;
  dismissToast: () => void;
  toastHost: ReactElement;
} {
  const [message, setMessage] = useState("");
  const [visible, setVisible] = useState(false);
  const [epoch, setEpoch] = useState(0);
  const [sticky, setSticky] = useState(false);
  const [durationMs, setDurationMs] = useState(TOAST_DURATION_MS);

  const dismissToast = useCallback(() => {
    setVisible(false);
    setSticky(false);
  }, []);

  const showToast = useCallback((msg: string, opts?: ShowToastOptions) => {
    const text = msg.trim();
    if (!text) return;
    const isSticky = Boolean(opts?.sticky);
    setMessage(text);
    setSticky(isSticky);
    setDurationMs(
      opts?.durationMs ??
        (isSticky || opts?.error
          ? TOAST_ERROR_DURATION_MS
          : TOAST_DURATION_MS),
    );
    setEpoch((n) => n + 1);
    setVisible(true);
  }, []);

  const toastHost = useMemo(
    () => (
      <Toast
        key={epoch}
        message={message}
        visible={visible}
        sticky={sticky}
        durationMs={sticky ? undefined : durationMs}
        onDismiss={dismissToast}
      />
    ),
    [epoch, message, visible, sticky, durationMs, dismissToast],
  );

  return { showToast, dismissToast, toastHost };
}
