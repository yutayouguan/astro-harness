/** A2UI surface 渲染器：解析 operations 并挂接 action。 */

import { useState } from "react";
import { useI18n } from "../i18n/LocaleContext";
import { renderCatalogTree } from "./CatalogAdapter";
import { collectComponents, parseOperations } from "./validate";

type Props = {
  operations: unknown[];
  disabled?: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
};

export default function A2UIRenderer({
  operations,
  disabled = false,
  onAction,
}: Props) {
  const { t } = useI18n();
  const [fieldValues, setFieldValues] = useState<Record<string, unknown>>({});
  const ops = parseOperations(operations);
  const components = collectComponents(ops);
  if (!components.length) return null;

  return (
    <div className={`a2ui-surface ${disabled ? "is-disabled" : ""}`}>
      {renderCatalogTree(components, {
        disabled,
        onAction,
        unknownLabel: t("chat.a2ui.unknown"),
        fieldValues,
        setFieldValue: (id, value) =>
          setFieldValues((prev) => ({ ...prev, [id]: value })),
      })}
    </div>
  );
}
