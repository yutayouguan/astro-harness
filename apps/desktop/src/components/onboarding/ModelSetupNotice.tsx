import { useI18n } from "../../i18n/LocaleContext";
export function ModelSetupNotice({ onConfigure }: { onConfigure: () => void }) {
  const { t } = useI18n();
  return (
    <aside className="model-setup-notice" role="status">
      <span>{t("chat.modelSetupRequired")}</span>
      <button type="button" onClick={onConfigure}>
        {t("chat.configureModel")}
      </button>
    </aside>
  );
}
