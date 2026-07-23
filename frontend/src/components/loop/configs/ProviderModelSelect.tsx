/** 供应商 + 模型选择器 —— 按媒体能力过滤，自动选配默认模型 */

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
  supports_image: boolean;
  supports_video: boolean;
  supports_tts: boolean;
  supports_music: boolean;
}

interface ProvidersState {
  providers: ProviderDto[];
  active_provider_id: string | null;
}

export type MediaType = "chat" | "image" | "video" | "tts" | "music" | "subtitle";

function supportsMedia(provider: ProviderDto, mediaType: MediaType): boolean {
  switch (mediaType) {
    case "image": return provider.supports_image;
    case "video": return provider.supports_video;
    case "tts": return provider.supports_tts;
    case "music": return provider.supports_music;
    case "subtitle": return true;
    default: return true;
  }
}

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
  const [allProviders, setAllProviders] = useState<ProviderDto[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [models, setModels] = useState<string[]>([]);
  const [initialized, setInitialized] = useState(false);

  // 按能力过滤的可用供应商
  const providers = allProviders.filter((p) => supportsMedia(p, mediaType));

  useEffect(() => {
    (async () => {
      try {
        const state = await invoke<ProvidersState>("get_providers_state");
        const available = (state.providers ?? []).filter(
          (p) => p.enabled && p.has_api_key,
        );
        setAllProviders(available);
        setActiveId(state.active_provider_id ?? null);

        // 自动选中第一个有能力的供应商 + 其默认模型
        if (!providerId) {
          const capable = available.filter((p) => supportsMedia(p, mediaType));
          const best = capable.find((p) => p.id === state.active_provider_id) ?? capable[0];
          if (best) {
            onProviderChange(best.id);
            if (!model) {
              const defaultModel = getMediaModel(best, mediaType);
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

  const effectiveProvider = providerId || "";
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
        const provider = allProviders.find((p) => p.id === effectiveProvider);
        if (provider) {
          const m = getMediaModel(provider, mediaType);
          if (m) setModels([m]);
        }
      }
    })();
  }, [effectiveProvider, allProviders, mediaType]);

  const selectedProvider = providers.find((p) => p.id === providerId);
  const defaultMediaModel = selectedProvider ? getMediaModel(selectedProvider, mediaType) : "";
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
          {providers.length === 0 && <option value="">无可用供应商</option>}
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
            暂无支持{mediaLabel(mediaType)}的供应商，请先在「模型服务」中配置。
          </span>
        )}
      </label>

      <label className="loop-config-field">
        <span className="loop-config-label">
          {mediaType !== "chat" ? `${mediaLabel(mediaType)}模型` : "模型"}
        </span>
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
              <option key={m} value={m}>{m}</option>
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
            默认: {defaultMediaModel}
          </span>
        )}
      </label>
    </>
  );
}
