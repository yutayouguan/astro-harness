/** 供应商 + 模型选择器 —— 显示已启用供应商，默认继承主模型 */

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

interface ProvidersState {
  providers: ProviderDto[];
  active_provider_id: string | null;
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
  const [activeId, setActiveId] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [initialized, setInitialized] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const state = await invoke<ProvidersState>("get_providers_state");
        const available = (state.providers ?? []).filter(
          (p) => p.enabled && p.has_api_key,
        );
        setProviders(available);
        setActiveId(state.active_provider_id ?? null);

        // 如果父组件没有设置 providerId，自动继承主模型的提供商和模型
        if (!providerId && state.active_provider_id) {
          const active = available.find((p) => p.id === state.active_provider_id);
          if (active) {
            onProviderChange(active.id);
            if (!model && active.model) {
              onModelChange(active.model);
            }
          }
        }
        setInitialized(true);
      } catch {
        setInitialized(true);
      }
    })();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 当选中的供应商变化时，加载该供应商的模型列表
  const effectiveProvider = providerId || activeId || "";
  useEffect(() => {
    if (!effectiveProvider) {
      setModels([]);
      return;
    }
    (async () => {
      try {
        const cached = await invoke<string[]>("get_cached_provider_models", {
          providerId: effectiveProvider,
        });
        setModels(cached ?? []);
      } catch {
        const provider = providers.find((p) => p.id === effectiveProvider);
        if (provider?.model) {
          setModels([provider.model]);
        }
      }
    })();
  }, [effectiveProvider, providers]);

  const activeProvider = providers.find((p) => p.id === activeId);
  const selectedProvider = providers.find((p) => p.id === providerId);
  const displayModel = model || selectedProvider?.model || activeProvider?.model || "";

  return (
    <>
      <label className="loop-config-field">
        <span className="loop-config-label">供应商</span>
        <select
          className="loop-config-select"
          value={providerId}
          onChange={(e) => {
            const newId = e.target.value;
            onProviderChange(newId);
            // 切换供应商时自动设置该供应商的默认模型
            const p = providers.find((pp) => pp.id === newId);
            onModelChange(p?.model || "");
          }}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.display_name}{p.id === activeId ? " (当前主模型)" : ""}
            </option>
          ))}
        </select>
        {!initialized && (
          <span className="loop-config-hint">加载供应商列表…</span>
        )}
        {initialized && providers.length === 0 && (
          <span className="loop-config-hint">
            暂无可用供应商，请先在「模型服务」中配置并启用。
          </span>
        )}
      </label>

      <label className="loop-config-field">
        <span className="loop-config-label">模型</span>
        {models.length > 0 ? (
          <select
            className="loop-config-select"
            value={displayModel}
            onChange={(e) => onModelChange(e.target.value)}
          >
            {!models.includes(displayModel) && displayModel && (
              <option value={displayModel}>{displayModel}</option>
            )}
            {models.map((m) => (
              <option key={m} value={m}>
                {m}
              </option>
            ))}
          </select>
        ) : (
          <input
            className="loop-config-input"
            value={model}
            onChange={(e) => onModelChange(e.target.value)}
            placeholder={activeProvider?.model || "输入模型名称"}
          />
        )}
        {activeProvider && !model && (
          <span className="loop-config-hint">
            当前继承主模型: {activeProvider.display_name} / {activeProvider.model}
          </span>
        )}
      </label>
    </>
  );
}
