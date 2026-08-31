/** 供应商 + 模型选择器 —— 按媒体能力过滤，自动选配默认模型 */

import { useEffect, useState, useRef } from "react";
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

export type MediaType =
  "chat" | "image" | "video" | "tts" | "music" | "subtitle";

function supportsMedia(provider: ProviderDto, mediaType: MediaType): boolean {
  switch (mediaType) {
    case "image":
      return provider.supports_image;
    case "video":
      return provider.supports_video;
    case "tts":
      return provider.supports_tts;
    case "music":
      return provider.supports_music;
    case "subtitle":
      return true;
    default:
      return true;
  }
}

function getMediaModel(provider: ProviderDto, mediaType: MediaType): string {
  switch (mediaType) {
    case "image":
      return provider.image_model || provider.model;
    case "video":
      return provider.video_model || provider.model;
    case "tts":
      return provider.tts_model || provider.model;
    case "music":
      return provider.music_model || provider.model;
    case "subtitle":
      return provider.model;
    default:
      return provider.model;
  }
}

function mediaLabel(mediaType: MediaType): string {
  switch (mediaType) {
    case "image":
      return "图片生成";
    case "video":
      return "视频生成";
    case "tts":
      return "语音合成";
    case "music":
      return "音乐生成";
    case "subtitle":
      return "语音识别";
    default:
      return "聊天";
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

  const providers = allProviders.filter((p) => supportsMedia(p, mediaType));

  // 标记供应商刚被用户（或初始化）切换，下一轮 model effect 应强制重选模型
  const providerJustChanged = useRef(false);

  // ── 初始化：加载供应商列表，若当前 provider 无效则自动选最佳 ──
  useEffect(() => {
    (async () => {
      try {
        const state = await invoke<ProvidersState>("get_providers_state");
        const available = (state.providers ?? []).filter(
          (p) => p.enabled && p.has_api_key,
        );
        setAllProviders(available);
        setActiveId(state.active_provider_id ?? null);

        const capable = available.filter((p) => supportsMedia(p, mediaType));
        const matchesCurrent = capable.find((p) => p.id === providerId);

        if (!providerId || !matchesCurrent) {
          const best =
            capable.find((p) => p.id === state.active_provider_id) ??
            capable[0];
          if (best) {
            providerJustChanged.current = true;
            onProviderChange(best.id);
            // 不在这里调 onModelChange —— 由 effectiveProvider useEffect 在下一轮渲染处理
          }
        }
        setInitialized(true);
      } catch {
        setInitialized(true);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ── provider 变化时：加载模型列表并自动选配默认模型 ──
  const effectiveProvider = providerId || "";
  useEffect(() => {
    if (!effectiveProvider) {
      setModels([]);
      return;
    }
    const shouldForceModel = providerJustChanged.current;
    providerJustChanged.current = false;

    (async () => {
      try {
        const cached = await invoke<string[]>("get_cached_provider_models", {
          providerId: effectiveProvider,
        });
        const list = cached ?? [];
        setModels(list);

        // 需要重选模型的条件：供应商刚切换 / model 为空 / model 不在新供应商的列表中
        if (
          shouldForceModel ||
          !model ||
          (list.length > 0 && !list.includes(model))
        ) {
          const provider = allProviders.find((p) => p.id === effectiveProvider);
          const def = provider ? getMediaModel(provider, mediaType) : list[0];
          onModelChange(def || list[0] || "");
        }
      } catch {
        const provider = allProviders.find((p) => p.id === effectiveProvider);
        if (provider) {
          const m = getMediaModel(provider, mediaType);
          if (m) {
            setModels([m]);
            if (shouldForceModel || !model) {
              onModelChange(m);
            }
          }
        }
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [effectiveProvider, allProviders, mediaType]);

  const selectedProvider = providers.find((p) => p.id === providerId);
  const defaultMediaModel = selectedProvider
    ? getMediaModel(selectedProvider, mediaType)
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
            providerJustChanged.current = true;
            onProviderChange(e.target.value);
            // 不调 onModelChange —— useEffect 在下一轮渲染中自动处理
          }}
        >
          {providers.length === 0 && <option value="">无可用供应商</option>}
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.display_name}
              {p.id === activeId ? " (当前)" : ""}
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
          <span className="loop-config-hint">默认: {defaultMediaModel}</span>
        )}
      </label>
    </>
  );
}
