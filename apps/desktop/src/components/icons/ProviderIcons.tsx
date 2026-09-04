/** 供应商品牌图标。 */
import type { CSSProperties, ComponentType, SVGProps } from "react";
import Anthropic from "@lobehub/icons/es/Anthropic/components/Mono";
import Azure from "@lobehub/icons/es/Azure/components/Mono";
import Bailian from "@lobehub/icons/es/Bailian/components/Mono";
import Cohere from "@lobehub/icons/es/Cohere/components/Mono";
import DeepSeek from "@lobehub/icons/es/DeepSeek/components/Mono";
import ByteDance from "@lobehub/icons/es/ByteDance/components/Mono";
import ClaudeCode from "@lobehub/icons/es/ClaudeCode/components/Mono";
import Cline from "@lobehub/icons/es/Cline/components/Mono";
import Doubao from "@lobehub/icons/es/Doubao/components/Mono";
import Fireworks from "@lobehub/icons/es/Fireworks/components/Mono";
import Gemini from "@lobehub/icons/es/Gemini/components/Mono";
import Google from "@lobehub/icons/es/Google/components/Mono";
import Groq from "@lobehub/icons/es/Groq/components/Mono";
import HuggingFace from "@lobehub/icons/es/HuggingFace/components/Mono";
import Hunyuan from "@lobehub/icons/es/Hunyuan/components/Mono";
import HermesAgent from "@lobehub/icons/es/HermesAgent/components/Mono";
import InternLM from "@lobehub/icons/es/InternLM/components/Mono";
import KiloCode from "@lobehub/icons/es/KiloCode/components/Mono";
import Kimi from "@lobehub/icons/es/Kimi/components/Mono";
import Meta from "@lobehub/icons/es/Meta/components/Mono";
import Minimax from "@lobehub/icons/es/Minimax/components/Mono";
import Mistral from "@lobehub/icons/es/Mistral/components/Mono";
import Moonshot from "@lobehub/icons/es/Moonshot/components/Mono";
import Nvidia from "@lobehub/icons/es/Nvidia/components/Mono";
import Ollama from "@lobehub/icons/es/Ollama/components/Mono";
import OpenClaw from "@lobehub/icons/es/OpenClaw/components/Mono";
import OpenCode from "@lobehub/icons/es/OpenCode/components/Mono";
import OpenHands from "@lobehub/icons/es/OpenHands/components/Mono";
import OpenAI from "@lobehub/icons/es/OpenAI/components/Mono";
import OpenRouter from "@lobehub/icons/es/OpenRouter/components/Mono";
import Perplexity from "@lobehub/icons/es/Perplexity/components/Mono";
import Qwen from "@lobehub/icons/es/Qwen/components/Mono";
import RooCode from "@lobehub/icons/es/RooCode/components/Mono";
import Stepfun from "@lobehub/icons/es/Stepfun/components/Mono";
import Together from "@lobehub/icons/es/Together/components/Mono";
import Volcengine from "@lobehub/icons/es/Volcengine/components/Mono";
import XAI from "@lobehub/icons/es/XAI/components/Mono";
import XiaomiMiMo from "@lobehub/icons/es/XiaomiMiMo/components/Mono";
import Yi from "@lobehub/icons/es/Yi/components/Mono";
import Zhipu from "@lobehub/icons/es/Zhipu/components/Mono";
import Codex from "@lobehub/icons/es/Codex/components/Mono";
import Cursor from "@lobehub/icons/es/Cursor/components/Mono";
import Lovable from "@lobehub/icons/es/Lovable/components/Mono";
import Trae from "@lobehub/icons/es/Trae/components/Mono";
import V0 from "@lobehub/icons/es/V0/components/Mono";
import Windsurf from "@lobehub/icons/es/Windsurf/components/Mono";

type IconProps = SVGProps<SVGSVGElement> & {
  size?: number | string;
};

type LobeMonoIcon = ComponentType<{
  size?: number | string;
  className?: string;
  style?: CSSProperties;
}>;

type BrandKey =
  | "anthropic"
  | "openai"
  | "google"
  | "deepseek"
  | "ollama"
  | "azure"
  | "zhipu"
  | "openrouter"
  | "bailian"
  | "qwen"
  | "nvidia"
  | "moonshot"
  | "kimi"
  | "volcengine"
  | "doubao"
  | "minimax"
  | "meta"
  | "xai"
  | "mistral"
  | "cohere"
  | "groq"
  | "perplexity"
  | "together"
  | "fireworks"
  | "huggingface"
  | "stepfun"
  | "internlm"
  | "yi"
  | "hunyuan"
  | "bytedance"
  | "xiaomi";

const BRAND_ICONS: Record<BrandKey, LobeMonoIcon> = {
  anthropic: Anthropic,
  openai: OpenAI,
  google: Google,
  deepseek: DeepSeek,
  ollama: Ollama,
  azure: Azure,
  zhipu: Zhipu,
  openrouter: OpenRouter,
  bailian: Bailian,
  qwen: Qwen,
  nvidia: Nvidia,
  moonshot: Moonshot,
  kimi: Kimi,
  volcengine: Volcengine,
  doubao: Doubao,
  minimax: Minimax,
  meta: Meta,
  xai: XAI,
  mistral: Mistral,
  cohere: Cohere,
  groq: Groq,
  perplexity: Perplexity,
  together: Together,
  fireworks: Fireworks,
  huggingface: HuggingFace,
  stepfun: Stepfun,
  internlm: InternLM,
  yi: Yi,
  hunyuan: Hunyuan,
  bytedance: ByteDance,
  xiaomi: XiaomiMiMo,
};

function toIconProps(props: IconProps): {
  size?: number | string;
  className?: string;
  style?: CSSProperties;
} {
  const { size = 16, className, style, width, height } = props;
  return {
    size: size ?? width ?? height ?? 16,
    className,
    style,
  };
}

function resolveProviderBrand(kind?: string): BrandKey | null {
  const key = (kind ?? "").toLowerCase();
  if (!key || key === "custom") return null;
  if (key.includes("anthropic") || key.includes("claude")) return "anthropic";
  if (key.includes("azure")) return "azure";
  if (
    key.includes("zhipu") ||
    key.includes("智谱") ||
    key.includes("glm") ||
    key.includes("zai") ||
    key === "z.ai"
  ) {
    return "zhipu";
  }
  if (key.includes("openrouter")) return "openrouter";
  if (key.includes("bailian") || key.includes("dashscope")) return "bailian";
  if (key.includes("qwen")) return "qwen";
  if (key.includes("nvidia")) return "nvidia";
  if (key.includes("moonshot")) return "moonshot";
  if (key.includes("kimi")) return "kimi";
  if (key.includes("volcengine") || key.includes("volc")) return "volcengine";
  if (key.includes("doubao")) return "doubao";
  if (key.includes("minimax")) return "minimax";
  if (key.includes("hunyuan") || key.includes("tencent")) return "hunyuan";
  if (key.includes("openai") || key.includes("gpt")) return "openai";
  if (key.includes("google") || key.includes("gemini")) return "google";
  if (key.includes("deepseek")) return "deepseek";
  if (key.includes("ollama")) return "ollama";
  return null;
}

/** 按 provider kind / id 取品牌图标（@lobehub/icons Mono） */
export function ProviderBrandIcon({
  kind,
  ...props
}: IconProps & { kind?: string }) {
  const brand = resolveProviderBrand(kind) ?? "openai";
  const Icon = BRAND_ICONS[brand];
  return <Icon {...toIconProps(props)} />;
}

export const IconAnthropic = Anthropic;
export const IconOpenAI = OpenAI;
export const IconGemini = Gemini;
export const IconDeepSeek = DeepSeek;
export const IconOllama = Ollama;
export const IconAzure = Azure;
export const IconZhipu = Zhipu;
export const IconOpenRouter = OpenRouter;
export const IconBailian = Bailian;
export const IconQwen = Qwen;
export const IconNvidia = Nvidia;
export const IconMoonshot = Moonshot;
export const IconKimi = Kimi;
export const IconVolcengine = Volcengine;
export const IconDoubao = Doubao;
export const IconMiniMax = Minimax;

/** 从模型 ID 识别已知品牌；不认识则返回 null */
function resolveModelBrand(modelId: string): BrandKey | null {
  const id = modelId.toLowerCase();
  if (id.includes("claude") || id.includes("anthropic")) return "anthropic";
  if (id.includes("gemini") || id.includes("gemma")) return "google";
  if (
    id.includes("gpt") ||
    id.includes("chatgpt") ||
    /(^|[-_/])o[1-4]([-_/]|$)/.test(id)
  ) {
    return "openai";
  }
  if (id.includes("deepseek")) return "deepseek";
  if (
    id.includes("glm") ||
    id.includes("zhipu") ||
    id.includes("chatglm") ||
    id.includes("zai")
  ) {
    return "zhipu";
  }
  if (id.includes("azure")) return "azure";
  if (id.includes("openrouter")) return "openrouter";
  if (id.includes("qwen")) return "qwen";
  if (id.includes("dashscope") || id.includes("bailian")) return "bailian";
  if (id.includes("nvidia") || id.includes("nemotron")) return "nvidia";
  if (id.includes("kimi")) return "kimi";
  if (id.includes("moonshot")) return "moonshot";
  if (id.includes("doubao")) return "doubao";
  if (
    id.includes("bytedance") ||
    id.includes("seedance") ||
    id.includes("seedream")
  )
    return "bytedance";
  if (id.includes("hunyuan") || id.includes("tencent")) return "hunyuan";
  if (id.includes("mimo") || id.includes("xiaomi")) return "xiaomi";
  if (id.includes("volc") || id.includes("ep-")) return "volcengine";
  if (id.includes("minimax") || id.includes("abab")) {
    return "minimax";
  }
  if (
    id.includes("llama") ||
    id.startsWith("meta/") ||
    id.includes("meta-llama")
  )
    return "meta";
  if (id.includes("mistral") || id.startsWith("mistralai/")) return "mistral";
  if (id.includes("grok") || id.startsWith("x-ai/") || id.startsWith("xai/"))
    return "xai";
  if (id.includes("cohere") || id.startsWith("cohere/")) return "cohere";
  if (id.startsWith("groq/")) return "groq";
  if (id.includes("perplexity") || id.startsWith("perplexity/"))
    return "perplexity";
  if (id.startsWith("together/")) return "together";
  if (id.startsWith("fireworks/")) return "fireworks";
  if (id.includes("hugging") || id.startsWith("huggingface/"))
    return "huggingface";
  if (id.includes("step-") || id.startsWith("stepfun/")) return "stepfun";
  if (id.includes("internlm") || id.includes("intern")) return "internlm";
  if (id.startsWith("01-ai/") || id.includes("/yi-")) return "yi";
  return null;
}

/** 模型列表图标：已知品牌用图标，未知用首字母 */
export function ModelBrandIcon({
  modelId,
  className,
  ...props
}: IconProps & { modelId: string; className?: string }) {
  const brand = resolveModelBrand(modelId);
  if (brand) {
    const Icon = BRAND_ICONS[brand];
    return (
      <span
        className={
          className ? `model-brand-icon ${className}` : "model-brand-icon"
        }
      >
        <Icon {...toIconProps(props)} />
      </span>
    );
  }
  const letter = (modelId.match(/[a-zA-Z0-9]/)?.[0] ?? "?").toUpperCase();
  return (
    <span
      className={
        className
          ? `model-brand-icon is-letter ${className}`
          : "model-brand-icon is-letter"
      }
      aria-hidden
    >
      {letter}
    </span>
  );
}

const APP_ICONS: Array<{
  matches: string[];
  Icon: LobeMonoIcon;
}> = [
  { matches: ["hermes agent", "hermes-agent"], Icon: HermesAgent },
  { matches: ["claude code", "claude-code"], Icon: ClaudeCode },
  { matches: ["kilo code", "kilo-code"], Icon: KiloCode },
  { matches: ["roo code", "roo-code"], Icon: RooCode },
  { matches: ["openclaw", "open-claw"], Icon: OpenClaw },
  { matches: ["opencode", "open-code"], Icon: OpenCode },
  { matches: ["openhands", "open-hands"], Icon: OpenHands },
  { matches: ["windsurf"], Icon: Windsurf },
  { matches: ["cursor"], Icon: Cursor },
  { matches: ["lovable"], Icon: Lovable },
  { matches: ["trae"], Icon: Trae },
  { matches: ["v0", "v0.dev"], Icon: V0 },
  { matches: ["cline"], Icon: Cline },
  { matches: ["codex"], Icon: Codex },
  { matches: ["deepseek harness"], Icon: DeepSeek },
];

/** Apps 排行榜图标：优先使用 @lobehub/icons，未收录项回退为首字母。 */
export function AppBrandIcon({
  appId,
  appName,
  className,
  ...props
}: IconProps & { appId: string; appName: string; className?: string }) {
  const lookup = `${appId} ${appName}`.toLowerCase();
  const entry = APP_ICONS.find(({ matches }) =>
    matches.some((candidate) => lookup.includes(candidate)),
  );
  if (entry) {
    const Icon = entry.Icon;
    return (
      <span
        className={className ? `model-brand-icon ${className}` : "model-brand-icon"}
        aria-hidden
      >
        <Icon {...toIconProps(props)} />
      </span>
    );
  }

  const letter = (appName.match(/[a-zA-Z0-9]/)?.[0] ?? "?").toUpperCase();
  return (
    <span
      className={
        className
          ? `model-brand-icon is-letter ${className}`
          : "model-brand-icon is-letter"
      }
      aria-hidden
    >
      {letter}
    </span>
  );
}
