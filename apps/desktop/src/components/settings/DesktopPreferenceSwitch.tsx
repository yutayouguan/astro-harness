import { useId } from "react";
import { useI18n } from "../../i18n/LocaleContext";
import {
  useDesktopPreference,
  type DesktopPreference,
} from "../../hooks/settings/useDesktopPreference";

export function DesktopPreferenceSwitch({
  kind,
  label,
  preview = false,
}: {
  kind: DesktopPreference;
  label: string;
  preview?: boolean;
}) {
  const { locale } = useI18n();
  const state = useDesktopPreference(kind, preview);
  const errorId = useId();
  return (
    <span className="desktop-preference-control">
      <button
        type="button"
        role="switch"
        className="prefs-switch"
        aria-label={label}
        aria-checked={state.value ?? false}
        aria-busy={state.busy || undefined}
        aria-describedby={state.error ? errorId : undefined}
        disabled={state.busy || state.value === null}
        onClick={() => void state.change(!state.value)}
      >
        <span className="prefs-switch-thumb" />
      </button>
      {state.error && (
        <span id={errorId} className="desktop-preference-error" role="alert">
          {locale === "zh"
            ? kind === "notifications"
              ? "通知未生效，请检查系统通知权限。"
              : "无法确认登录启动状态，未假定更改成功。"
            : kind === "notifications"
              ? "Check system notification permissions."
              : "Could not confirm the login setting."}
          <button
            type="button"
            onClick={() => void state.reload()}
            disabled={state.busy}
          >
            {locale === "zh" ? "重新读取" : "Reload"}
          </button>
        </span>
      )}
    </span>
  );
}
