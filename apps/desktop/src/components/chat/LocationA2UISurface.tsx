import { useMemo } from "react";
import A2UIRenderer from "../../a2ui/A2UIRenderer";
import { useI18n } from "../../i18n/LocaleContext";
import { useIpCitySuggestion } from "../../hooks/ui/useIpCitySuggestion";

type Props = {
  operations: unknown[];
  disabled?: boolean;
  mediaBaseDir?: string | null;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

export default function LocationA2UISurface({
  operations,
  disabled = false,
  mediaBaseDir,
  onAction,
}: Props) {
  const { t } = useI18n();
  const suggestion = useIpCitySuggestion(!disabled);
  const initialFieldValues = useMemo(
    () =>
      suggestion.status === "success" ? { city: suggestion.city } : undefined,
    [suggestion.status, suggestion.city],
  );

  return (
    <div className="location-a2ui-surface">
      <p className="a2ui-caption">
        {suggestion.status === "loading"
          ? t("chat.location.ipLoading")
          : suggestion.status === "success"
            ? t("chat.location.ipSuggested")
            : t("chat.location.ipFailed")}
      </p>
      <A2UIRenderer
        operations={operations}
        disabled={disabled}
        initialFieldValues={initialFieldValues}
        mediaBaseDir={mediaBaseDir}
        onAction={onAction}
      />
    </div>
  );
}
