import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { SelectField, cfgStr } from "./ConfigField";
import { KeyValueEditor, KvEntry, jsonToKvEntries, kvEntriesToObj } from "./StructuredEditors";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
}

export default function CustomLoopConfig({ config, onChange }: ConfigProps) {
  const [workflows, setWorkflows] = useState<{ id: string; name: string }[]>([]);

  useEffect(() => {
    invoke<{ id: string; name: string }[]>("list_loops")
      .then(setWorkflows)
      .catch(() => setWorkflows([]));
  }, []);

  return (
    <>
      <SelectField
        label="引用的 Loop ID"
        value={cfgStr(config, "workflow_id")}
        onChange={(v) => onChange({ ...config, workflow_id: v })}
        options={[
          { value: "", label: "请选择工作流…" },
          ...workflows.map((w) => ({ value: w.id, label: w.name })),
        ]}
        hint="此节点运行时会调用该 Loop 作为子工作流执行"
      />
      <KeyValueEditor
        label="输入映射"
        value={jsonToKvEntries(config.input_mapping)}
        onChange={(v: KvEntry[]) => onChange({ ...config, input_mapping: kvEntriesToObj(v) })}
        keyPlaceholder="子 Loop 输入名"
        valuePlaceholder="值 / {{var}}"
        hint="将当前上下文的变量映射到子 Loop 的输入"
      />
    </>
  );
}
