import { useEffect, useId, useState } from "react";
import { invoke, convertFileSrc } from "@tauri-apps/api/core";
import { Bell, ImageOff, PawPrint, Power } from "lucide-react";
import { DesktopPreferenceSwitch } from "../settings/DesktopPreferenceSwitch";
import { Button } from "../ui";
import companion from "../../assets/onboarding-companion.svg";

type PetInfo = {
  enabled: boolean;
  hasAsset: boolean;
  previewPath: string | null;
  spriteVersionNumber: number | null;
  displayName: string | null;
};
export function OnboardingPreferences({
  locale,
  preview,
  petChoice,
  onPetChoice,
}: {
  locale: "zh" | "en";
  preview: boolean;
  petChoice: boolean | null;
  onPetChoice: (value: boolean | null) => void;
}) {
  const zh = locale === "zh";
  const [pet, setPet] = useState<PetInfo | null>(
    preview
      ? {
          enabled: false,
          hasAsset: false,
          previewPath: null,
          spriteVersionNumber: null,
          displayName: null,
        }
      : null,
  );
  const [error, setError] = useState(false);
  const [retry, setRetry] = useState(0);
  const petHintId = useId();
  useEffect(() => {
    if (preview) return;
    let disposed = false;
    setError(false);
    void invoke<PetInfo>("get_onboarding_pet")
      .then((value) => {
        if (!disposed) setPet(value);
      })
      .catch(() => {
        if (!disposed) setError(true);
      });
    return () => {
      disposed = true;
    };
  }, [preview, retry]);
  const enabled = petChoice ?? pet?.enabled ?? false;
  const missingAsset = Boolean(pet?.previewPath && !pet.hasAsset);
  const previewSrc =
    pet?.hasAsset && pet.previewPath
      ? convertFileSrc(pet.previewPath)
      : companion;
  return (
    <section
      className="onboarding-preferences"
      aria-label={zh ? "使用偏好（可选）" : "Usage preferences (optional)"}
    >
      <h2>
        {zh ? "使用偏好" : "Usage preferences"}{" "}
        <small>
          {zh
            ? "可选，之后可在设置中修改"
            : "Optional · change later in Settings"}
        </small>
      </h2>
      <p className="onboarding-preference-save-hint">
        {zh
          ? "通知和登录启动即时保存；桌宠在完成设置后显示。"
          : "Notification and login preferences save immediately; your companion appears after setup."}
      </p>
      <div className="onboarding-preference-row onboarding-pet-preference">
        {missingAsset ? (
          <span
            className="onboarding-pet-preview is-missing"
            role="img"
            aria-label={zh ? "桌宠资源不可用" : "Pet asset unavailable"}
          >
            <ImageOff size={22} aria-hidden />
          </span>
        ) : pet?.hasAsset && pet.spriteVersionNumber === 2 ? (
          <div
            className="onboarding-pet-preview is-atlas"
            style={{ backgroundImage: `url("${previewSrc}")` }}
            role="img"
            aria-label={
              pet.displayName || (zh ? "已有桌宠预览" : "Current pet preview")
            }
          />
        ) : (
          <img
            className="onboarding-pet-preview"
            src={previewSrc}
            alt={zh ? "桌面小伙伴预览" : "Desktop companion preview"}
          />
        )}
        <div className="onboarding-preference-copy">
          <strong>
            <PawPrint size={15} aria-hidden />
            {zh ? "桌面宠物" : "Desktop companion"}
          </strong>
          <p id={petHintId}>
            {missingAsset
              ? zh
                ? "原有资源不可用，请在桌宠设置中重新选择。"
                : "Your saved pet is unavailable. Reselect it in Settings."
              : pet?.hasAsset
                ? zh
                  ? "保留你的现有宠物，完成设置后显示。"
                  : "Keep your current companion; show it after setup."
                : zh
                  ? "内置小伙伴，无需模型生成；完成设置后显示。"
                  : "Built-in companion, no AI generation. Shown after setup."}
          </p>
        </div>
        <button
          type="button"
          role="switch"
          className="prefs-switch"
          aria-label={zh ? "桌面宠物" : "Desktop companion"}
          aria-describedby={petHintId}
          aria-checked={enabled}
          disabled={!pet || (missingAsset && !enabled)}
          onClick={() => onPetChoice(!enabled)}
        >
          <span className="prefs-switch-thumb" />
        </button>
      </div>
      {error && (
        <div className="onboarding-preference-error" role="alert">
          {zh
            ? "暂时无法读取桌宠设置，不会覆盖现有偏好。"
            : "Pet settings unavailable; existing preferences are preserved."}
          <Button size="sm" onClick={() => setRetry((value) => value + 1)}>
            {zh ? "重新读取" : "Reload"}
          </Button>
          {petChoice !== null && (
            <Button size="sm" onClick={() => onPetChoice(null)}>
              {zh ? "保留原设置" : "Keep current setting"}
            </Button>
          )}
        </div>
      )}
      <div className="onboarding-preference-row">
        <span className="onboarding-preference-icon">
          <Bell size={18} aria-hidden />
        </span>
        <div className="onboarding-preference-copy">
          <strong>{zh ? "任务通知" : "Task notifications"}</strong>
          <p>
            {zh
              ? "仅提醒完成、失败和需要确认，不显示任务正文；开启时申请系统权限。"
              : "Completion, failure and action-required alerts only. No task content; permission requested when enabled."}
          </p>
        </div>
        <DesktopPreferenceSwitch
          kind="notifications"
          preview={preview}
          label={zh ? "任务通知" : "Task notifications"}
        />
      </div>
      <div className="onboarding-preference-row">
        <span className="onboarding-preference-icon">
          <Power size={18} aria-hidden />
        </span>
        <div className="onboarding-preference-copy">
          <strong>{zh ? "登录时启动" : "Launch at login"}</strong>
          <p>
            {zh
              ? "登录系统后自动启动 Astro，默认关闭。"
              : "Start Astro when you log in. Off by default."}
          </p>
        </div>
        <DesktopPreferenceSwitch
          kind="autostart"
          preview={preview}
          label={zh ? "登录时启动" : "Launch at login"}
        />
      </div>
    </section>
  );
}
