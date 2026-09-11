import { useTheme } from "../../hooks/app/useTheme";
import { useI18n } from "../../i18n/LocaleContext";

/** Native radios provide arrow-key navigation without another focus manager. */
export default function InterfaceMaterialPicker() {
  const { material, setMaterial } = useTheme();
  const { t } = useI18n();
  return (
    <fieldset className="material-picker">
      <legend>{t("prefs.appearance.surface.title")}</legend>
      <div className="material-picker-options">
        {(["glass", "soft"] as const).map((id) => (
          <label className="material-option" key={id}>
            <input
              type="radio"
              name="interface-material"
              value={id}
              checked={material === id}
              onChange={() => setMaterial(id)}
            />
            <span className="material-option-body">
              <span
                className={`material-swatch material-swatch--${id}`}
                aria-hidden="true"
              >
                <i />
                <i />
                <i />
              </span>
              <strong>{t(`prefs.appearance.surface.${id}`)}</strong>
              <span>{t(`prefs.appearance.surface.${id}Desc`)}</span>
            </span>
          </label>
        ))}
      </div>
    </fieldset>
  );
}
