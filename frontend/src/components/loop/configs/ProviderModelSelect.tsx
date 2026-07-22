/** 供应商 + 模型选择器 —— 只显示已启用且已配置 API Key 的供应商 */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ProviderDto {
  id: string;
  display_name: string;
  kind: string;
  model: string;
  enabled: boolean;
  has_api_key: boolean;
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

  useEffect(() => {
    (async () => {
      try {
        const state = await invoke<{ providers: ProviderDto[] }>("get_providers_state");
        const available = (state.providers ?? []).filter(
          (p) => p.enabled && p.has_api_key,
        );
        setProviders(available);
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
        {providers.length === 0 && (
          <span className="loop-config-hint">
            暂无可用供应商，请先在「模型服务」中配置并启用。
          </span>
        )}
      </label>

      <label className="loop-config-field">
        <span className="loop-config-label">模型</span>
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
        {!providerId && (
          <span className="loop-config-hint">
            未选择则使用当前的默认模型（和对话用的一致）。
          </span>
        )}
      </label>
    </>
  );
}
