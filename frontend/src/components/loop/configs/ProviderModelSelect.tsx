/** 供应商 + 模型选择器 —— 显示已启用供应商，支持按媒体类型选择专用模型 */

import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface ProviderDto {
  id: string;
  display_name: string;
  kind: string;
  model: string;
  enabled: boolean;
  has_api_key: boolean;
  image_model: string;
  video_model: string;
  tts_model: string;
  music_model: string;
  vision_model: string;
}

interface ProvidersState {
  providers: ProviderDto[];
  active_provider_id: string | null;
}

export type MediaType = "chat" | "image" | "video" | "tts" | "music" | "subtitle";

function getMediaModel(provider: ProviderDto, mediaType: MediaType): string {
  switch (mediaType) {
    case "image": return provider.image_model || provider.model;
    case "video": return provider.video_model || provider.model;
    case "tts": return provider.tts_model || provider.model;
    case "music": return provider.music_model || provider.model;
    case "subtitle": return provider.model;
    default: return provider.model;
  }
}

function mediaLabel(mediaType: MediaType): string {
  switch (mediaType) {
    case "image": return "图片生成";
    case "video": return "视频生成";
    case "tts": return "语音合成";
    case "music": return "音乐生成";
    case "subtitle": return "语音识别";
    default: return "聊天";
  }
}

interface Props {
  providerId: string;
  model: string;
  onProviderChange: (id: string) => void;
  onModelChange: (model: string) => void;
  mediaType?: MediaType;
}

export default function ProviderModelSelect({
  providerId,
  model,
  onProviderChange,
  onModelChange,
  mediaType = "chat",
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

        if (!providerId && state.active_provider_id) {
          const active = available.find((p) => p.id === state.active_provider_id);
          if (active) {
            onProviderChange(active.id);
            if (!model) {
              const defaultModel = getMediaModel(active, mediaType);
              if (defaultModel) onModelChange(defaultModel);
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
        if (provider) {
          const m = getMediaModel(provider, mediaType);
          if (m) setModels([m]);
        }
      }
    })();
  }, [effectiveProvider, providers, mediaType]);

  const activeProvider = providers.find((p) => p.id === activeId);
  const selectedProvider = providers.find((p) => p.id === providerId);
  const defaultMediaModel = selectedProvider
    ? getMediaModel(selectedProvider, mediaType)
    : activeProvider
      ? getMediaModel(activeProvider, mediaType)
      : "";
  const displayModel = model || defaultMediaModel;

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
            const p = providers.find((pp) => pp.id === newId);
            onModelChange(p ? getMediaModel(p, mediaType) : "");
          }}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.display_name}{p.id === activeId ? " (当前)" : ""}
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
        <span className="loop-config-label">{mediaType !== "chat" ? `${mediaLabel(mediaType)}模型` : "模型"}</span>
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
            placeholder={defaultMediaModel || "输入模型名称"}
          />
        )}
        {defaultMediaModel && !model && (
          <span className="loop-config-hint">
            使用已配置的{mediaLabel(mediaType)}模型: {defaultMediaModel}
          </span>
        )}
      </label>
    </>
  );
}
