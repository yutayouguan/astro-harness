import { useId } from "react";

/** Reuses the app's switch material, with a full label target and local help. */
export default function PetSettingSwitch({
  label,
  description,
  checked,
  disabled,
  onChange,
  controls,
}: {
  label: string;
  description: string;
  checked: boolean;
  disabled: boolean;
  onChange: (checked: boolean) => void;
  controls?: string;
}) {
  const id = useId();
  return (
    <div className="pet-setting-row">
      <label className="pet-setting-copy" htmlFor={id}>
        <strong>{label}</strong>
        <small id={`${id}-hint`}>{description}</small>
      </label>
      <button
        id={id}
        type="button"
        role="switch"
        className="prefs-switch pet-setting-switch"
        aria-label={label}
        aria-describedby={`${id}-hint`}
        aria-checked={checked}
        aria-controls={controls}
        disabled={disabled}
        onClick={() => onChange(!checked)}
      >
        <span className="prefs-switch-thumb" />
      </button>
    </div>
  );
}
