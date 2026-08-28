/** A2UI surface 渲染器：解析 operations 并挂接 action。 */

import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import { CatalogTree } from "./CatalogAdapter";
import { mergeInitialFieldValues } from "./initialFieldValues";
import { collectComponents, parseOperations } from "./validate";

type Props = {
  operations: unknown[];
  disabled?: boolean;
  initialFieldValues?: Record<string, unknown>;
  mediaBaseDir?: string | null;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

function surfaceKey(operations: unknown[]): string {
  const ops = parseOperations(operations);
  for (const op of ops) {
    const id = op.createSurface?.surfaceId;
    if (typeof id === "string" && id) return id;
  }
  // Fallback: stable-ish fingerprint when createSurface is missing.
  try {
    return JSON.stringify(operations);
  } catch {
    return String(operations.length);
  }
}

export default function A2UIRenderer({
  operations,
  disabled = false,
  initialFieldValues,
  mediaBaseDir = null,
  onAction,
}: Props) {
  const { t } = useI18n();
  const [fieldValues, setFieldValues] = useState<Record<string, unknown>>({});
  const key = useMemo(() => surfaceKey(operations), [operations]);
  const ops = parseOperations(operations);
  const components = collectComponents(ops);

  useEffect(() => {
    setFieldValues({});
  }, [key]);

  useEffect(() => {
    if (!initialFieldValues || Object.keys(initialFieldValues).length === 0) return;
    setFieldValues((current) =>
      mergeInitialFieldValues(current, initialFieldValues),
    );
  }, [initialFieldValues, key]);

  if (!components.length) return null;

  return (
    <div className={`a2ui-surface ${disabled ? "is-disabled" : ""}`}>
      <CatalogTree
        components={components}
        disabled={disabled}
        onAction={onAction}
        unknownLabel={t("chat.a2ui.unknown")}
        fieldValues={fieldValues}
        setFieldValue={(id, value) =>
          setFieldValues((prev) => ({ ...prev, [id]: value }))
        }
        mediaBaseDir={mediaBaseDir}
      />
    </div>
  );
}
