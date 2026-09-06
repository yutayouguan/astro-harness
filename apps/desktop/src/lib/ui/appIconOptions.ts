import blueIcon from "../../assets/app-icons/blue.png";
import deepBlueIcon from "../../assets/app-icons/deep_blue.png";
import blackIcon from "../../assets/app-icons/black.png";
import whiteIcon from "../../assets/app-icons/white.png";
import whiteLogoIcon from "../../assets/app-icons/white_logo.png";
import type {
  AppIconId,
  AppIconOptionDto,
  AppIconSettingsDto,
} from "../../types";

export const APP_ICON_OPTIONS: AppIconOptionDto[] = [
  { id: "blue", dataUrl: blueIcon },
  { id: "deep_blue", dataUrl: deepBlueIcon },
  { id: "black", dataUrl: blackIcon },
  { id: "white", dataUrl: whiteIcon },
  { id: "white_logo", dataUrl: whiteLogoIcon },
];

function isAppIconId(value: unknown): value is AppIconId {
  return APP_ICON_OPTIONS.some((option) => option.id === value);
}

export function appIconSettingsWithFallback(
  value: unknown,
  preferredCurrent: AppIconId = "blue",
): AppIconSettingsDto {
  const candidate =
    value && typeof value === "object"
      ? (value as Partial<AppIconSettingsDto>)
      : null;
  const current = isAppIconId(candidate?.current)
    ? candidate.current
    : preferredCurrent;
  const remoteOptions = Array.isArray(candidate?.options)
    ? candidate.options.filter(
        (option): option is AppIconOptionDto =>
          isAppIconId(option?.id) &&
          typeof option.dataUrl === "string" &&
          option.dataUrl.length > 0,
      )
    : [];
  const remoteById = new Map(
    remoteOptions.map((option) => [option.id, option]),
  );

  return {
    current,
    options: APP_ICON_OPTIONS.map(
      (fallback) => remoteById.get(fallback.id) ?? fallback,
    ),
  };
}
