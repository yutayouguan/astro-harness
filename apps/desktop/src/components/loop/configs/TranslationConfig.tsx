import { TextField, SelectField, cfgStr } from "./ConfigField";
import ProviderModelSelect from "./ProviderModelSelect";
import {
  KeyValueEditor,
  KvEntry,
  jsonToKvEntries,
  kvEntriesToObj,
} from "./StructuredEditors";
import type { UpstreamOutput } from "./upstreamOutputs";

interface ConfigProps {
  config: Record<string, unknown>;
  onChange: (config: Record<string, unknown>) => void;
  upstreamOutputs?: UpstreamOutput[];
}

const LANGUAGE_OPTIONS = [
  { value: "zh", label: "中文" },
  { value: "en", label: "English" },
  { value: "ja", label: "日本語" },
  { value: "ko", label: "한국어" },
  { value: "fr", label: "Français" },
  { value: "de", label: "Deutsch" },
  { value: "es", label: "Español" },
  { value: "pt", label: "Português" },
  { value: "ru", label: "Русский" },
  { value: "ar", label: "العربية" },
];

export default function TranslationConfig({
  config,
  onChange,
  upstreamOutputs,
}: ConfigProps) {
  const up = upstreamOutputs ?? [];
  return (
    <>
      <TextField
        label="输入文本"
        value={cfgStr(config, "text_template")}
        onChange={(v) => onChange({ ...config, text_template: v })}
        placeholder="输入要翻译的文本…"
        multiline
        hint="支持 {{var}} 引用上游变量"
        upstream={up}
      />
      <SelectField
        label="源语言"
        value={cfgStr(config, "source_lang", "auto")}
        onChange={(v) => onChange({ ...config, source_lang: v })}
        options={[{ value: "auto", label: "自动检测" }, ...LANGUAGE_OPTIONS]}
      />
      <SelectField
        label="目标语言"
        value={cfgStr(config, "target_lang", "en")}
        onChange={(v) => onChange({ ...config, target_lang: v })}
        options={LANGUAGE_OPTIONS}
      />
      <ProviderModelSelect
        providerId={cfgStr(config, "provider_id")}
        model={cfgStr(config, "model")}
        onProviderChange={(v) => onChange({ ...config, provider_id: v })}
        onModelChange={(v) => onChange({ ...config, model: v })}
      />
      <KeyValueEditor
        label="术语表"
        value={jsonToKvEntries(config.glossary)}
        onChange={(v: KvEntry[]) =>
          onChange({ ...config, glossary: kvEntriesToObj(v) })
        }
        keyPlaceholder="原文术语"
        valuePlaceholder="翻译"
        hint="指定专有名词的固定翻译"
      />
    </>
  );
}
