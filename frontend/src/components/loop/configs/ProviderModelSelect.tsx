/** 供应商 + 模型选择器 —— AI/多媒体节点共用 */

import { useCallback, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search } from "lucide-react";

interface ProviderDto {
  id: string;
  display_name: string;
  kind: string;
  model: string;
  models?: string[];
}

interface Props {
  providerId: string;
  model: string;
  onProviderChange: (id: string) => void;
  onModelChange: (model: string) => void;
}

export default function ProviderModelSelect({
  providerId,
  model,
  onProviderChange,
  onModelChange,
}: Props) {
  const [providers, setProviders] = useState<ProviderDto[]>([]);
  const [models, setModels] = useState<string[]>([]);
  const [modelSearch, setModelSearch] = useState("");
  const [modelDropdownOpen, setModelDropdownOpen] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const state = await invoke<{ providers: ProviderDto[] }>("get_providers_state");
        setProviders(state.providers ?? []);
      } catch {
        /* providers not available */
      }
    })();
  }, []);

  useEffect(() => {
    if (!providerId) {
      setModels([]);
      return;
    }
    (async () => {
      try {
        const cached = await invoke<string[]>("get_cached_provider_models", {
          providerId,
        });
        setModels(cached ?? []);
      } catch {
        const provider = providers.find((p) => p.id === providerId);
        if (provider?.model) {
          setModels([provider.model]);
        }
      }
    })();
  }, [providerId, providers]);

  const filteredModels = modelSearch
    ? models.filter((m) => m.toLowerCase().includes(modelSearch.toLowerCase()))
    : models;

  return (
    <>
      <label className="loop-config-field">
        <span className="loop-config-label">供应商</span>
        <select
          className="loop-config-select"
          value={providerId}
          onChange={(e) => {
            onProviderChange(e.target.value);
            onModelChange("");
          }}
        >
          <option value="">—</option>
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.display_name}
            </option>
          ))}
        </select>
      </label>

      <label className="loop-config-field">
        <span className="loop-config-label">模型</span>
        <div className="loop-model-select-wrapper">
          <select
            className="loop-config-select"
            value={model}
            onChange={(e) => onModelChange(e.target.value)}
            disabled={!providerId}
          >
            <option value="">{providerId ? "请选择模型" : "请先选择供应商"}</option>
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        </div>
        {!providerId && (
          <span className="loop-config-hint">
            未选择则使用当前的默认模型（和对话用的一致）。
          </span>
        )}
      </label>
    </>
  );
}
