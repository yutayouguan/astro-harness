# Media Model Capability Selectors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace free-text media model fields with capability-filtered selectors and connect a configurable Google music model to the existing `music_gen` tool.

**Architecture:** Keep the existing provider model-list pipeline as the sole source of selectable models. Add `music_gen` to shared model metadata, introduce a small pure TypeScript module for filtering and invalid-value cleanup, and carry `music_model` through provider persistence, gRPC, and `ImageGenTargets` so the existing tool can use it as its default while preserving explicit `clip` / `pro` overrides.

**Tech Stack:** Rust, serde, tonic/protobuf, TypeScript, React, Tauri invoke, Node `node:test`, Cargo tests.

## Global Constraints

- Unknown or capability-mismatched models are hidden; do not infer media capability from model IDs in the frontend.
- Every selector always includes an empty-valued built-in default option.
- An invalid saved media model is cleared only after the current provider model list has resolved, then falls back to the built-in default.
- Media settings remain limited to Google and OpenAI; video and music remain Google-only.
- The existing `music_gen` tool remains the implementation; do not modify the local playback tool `music`.
- Explicit `music_gen(model="clip"|"pro")` overrides provider configuration.
- Do not stage or commit unrelated working-tree changes.

---

### Task 1: Add the independent `music_gen` model capability

**Files:**
- Modify: `apps/desktop/src-tauri/src/litellm_meta.rs`
- Modify: `apps/desktop/src-tauri/src/model_meta.rs`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/lib/model/modelCaps.ts`
- Modify: `apps/desktop/src/lib/model/modelCaps.test.ts`
- Modify: `apps/desktop/src/components/agents/ModelCapabilityIcons.tsx`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: TypeScript test fixtures containing complete `ModelCapabilities` literals, as reported by `rg "audio_gen:" apps/desktop/src --glob '*.{ts,tsx}'`

**Interfaces:**
- Produces: `ModelCapabilities.music_gen: bool` in Rust and `music_gen: boolean` in TypeScript.
- Produces: LiteLLM metadata signals that distinguish music generation from TTS/audio generation.
- Consumed by: Task 2 media filtering and Task 5 selectors.

- [x] **Step 1: Add failing Rust metadata tests**

In `apps/desktop/src-tauri/src/model_meta.rs`, add fixtures covering the documented mode, the current Lyria metadata shape, and the TTS negative case:

```rust
#[test]
fn litellm_music_mode_sets_music_gen_only() {
    crate::litellm_meta::with_fixture(
        r#"{"lyria-test":{"mode":"music_generation","litellm_provider":"gemini"}}"#,
        || {
            let info = enrich_from_id("lyria-test", "google", None);
            assert!(info.capabilities.music_gen);
            assert!(!info.capabilities.audio_gen);
            assert!(!info.capabilities.tools);
        },
    );
}

#[test]
fn litellm_audio_only_chat_sets_music_gen() {
    crate::litellm_meta::with_fixture(
        r#"{"gemini/lyria-test":{"mode":"chat","supports_audio_output":true,"supports_function_calling":false,"supported_output_modalities":["audio"],"litellm_provider":"gemini"}}"#,
        || {
            let info = enrich_from_id("lyria-test", "google", None);
            assert!(info.capabilities.music_gen);
            assert!(!info.capabilities.audio_gen);
        },
    );
}

#[test]
fn litellm_tts_mode_does_not_set_music_gen() {
    crate::litellm_meta::with_fixture(
        r#"{"tts-test":{"mode":"audio_speech","supports_audio_output":true,"litellm_provider":"gemini"}}"#,
        || {
            let info = enrich_from_id("tts-test", "google", None);
            assert!(info.capabilities.audio_gen);
            assert!(!info.capabilities.music_gen);
        },
    );
}
```

- [x] **Step 2: Run the focused Rust tests and verify failure**

Run:

```bash
cargo test -p astro-agent --lib model_meta::tests::litellm_music -- --nocapture
cargo test -p astro-agent --lib model_meta::tests::litellm_audio_only_chat_sets_music_gen -- --nocapture
```

Expected: FAIL because `music_gen` and `supported_output_modalities` do not exist.

- [x] **Step 3: Parse music metadata and map it without model-name guessing**

In `apps/desktop/src-tauri/src/litellm_meta.rs`, add the field to both structs:

```rust
pub supported_output_modalities: Vec<String>,

#[serde(default)]
supported_output_modalities: Vec<String>,
```

Copy it in `RawEntry::into_entry`:

```rust
supported_output_modalities: self.supported_output_modalities,
```

Include `|| !e.supported_output_modalities.is_empty()` in the `parse_map` retention condition.

In `apps/desktop/src-tauri/src/model_meta.rs`, add the serde-defaulted field and update `enrich_model_info`:

```rust
#[serde(default)]
pub music_gen: bool,
```

```rust
let mode_music = mode.contains("music") || mode == "audio_generation";
let audio_only_chat = mode == "chat"
    && entry.supports_audio_output
    && !entry.supports_function_calling
    && entry.supported_output_modalities.len() == 1
    && entry.supported_output_modalities[0].eq_ignore_ascii_case("audio");
let is_music = mode_music || audio_only_chat;
let mode_audio = mode.contains("audio") && !is_music;
let non_chat = mode.contains("embed")
    || mode_image
    || mode_audio
    || mode_video
    || is_music
    || mode.contains("moderation");

info.capabilities.music_gen |= is_music;
if !is_music {
    info.capabilities.audio_gen |= mode_audio || entry.supports_audio_output;
}
```

Apply the `!is_music` guard in both existing non-chat and chat branches so a Lyria entry is not also exposed as TTS.

- [x] **Step 4: Run Rust metadata tests**

Run:

```bash
cargo test -p astro-agent --lib model_meta::tests -- --nocapture
cargo test -p astro-agent --lib litellm_meta::tests -- --nocapture
```

Expected: PASS.

- [x] **Step 5: Add failing frontend capability-order test**

Update `apps/desktop/src/lib/model/modelCaps.test.ts`:

```typescript
test("listActiveModelCaps keeps fixed order", () => {
  assert.deepEqual(
    listActiveModelCaps({
      tools: true,
      reasoning: true,
      vision: false,
      web: true,
      image_gen: true,
      video_gen: false,
      audio_gen: true,
      music_gen: true,
    }),
    ["tools", "reasoning", "web", "image_gen", "audio_gen", "music_gen"],
  );
});
```

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/model/modelCaps.test.ts
```

Expected: FAIL because `music_gen` is not a known capability.

- [x] **Step 6: Extend frontend types, defaults, and icon metadata**

Add `music_gen` to `ModelCapabilities`, `ModelCapKey`, `MODEL_CAP_ORDER`, and `EMPTY_MODEL_CAPABILITIES`; keep `inferModelCapabilities()` returning `music_gen: false`. Add a music icon entry to `ModelCapabilityIcons.tsx` and bilingual `modelCaps.musicGen` messages. Update every complete capability literal found by the `rg` command with:

```typescript
music_gen: false,
```

- [x] **Step 7: Run frontend tests and typecheck**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/model/modelCaps.test.ts src/lib/model/autoModelSelect.test.ts src/lib/chat/shouldShowThinkingControls.test.ts
cd frontend && npx tsc -b --pretty false
```

Expected: all tests PASS and TypeScript exits 0.

- [x] **Step 8: Commit the capability slice**

```bash
git add apps/desktop/src-tauri/src/litellm_meta.rs apps/desktop/src-tauri/src/model_meta.rs apps/desktop/src/types.ts apps/desktop/src/lib/model/modelCaps.ts apps/desktop/src/lib/model/modelCaps.test.ts apps/desktop/src/components/agents/ModelCapabilityIcons.tsx apps/desktop/src/i18n/messages.ts
git add $(rg -l "music_gen: false" apps/desktop/src --glob '*.{ts,tsx}')
git commit -m "feat(models): distinguish music generation capability"
```

---

### Task 2: Build and test pure media-model selector logic

**Files:**
- Create: `apps/desktop/src/lib/providers/mediaModelOptions.ts`
- Create: `apps/desktop/src/lib/providers/mediaModelOptions.test.ts`

**Interfaces:**
- Consumes: `ModelInfo` and `ModelCapabilities.music_gen` from Task 1.
- Produces: `MediaCapabilityKey`, `MEDIA_CAPABILITY_BY_FIELD`, `filterModelsByCapability`, `buildMediaModelOptions`, and `sanitizeMediaModelValue`.
- Consumed by: Task 5 `ProvidersPanel`.

- [x] **Step 1: Write failing pure-function tests**

Create `apps/desktop/src/lib/providers/mediaModelOptions.test.ts`:

```typescript
import assert from "node:assert/strict";
import { test } from "node:test";
import type { ModelInfo } from "../../types.ts";
import {
  buildMediaModelOptions,
  filterModelsByCapability,
  sanitizeMediaModelValue,
} from "./mediaModelOptions.ts";

const model = (
  id: string,
  caps: Partial<ModelInfo["capabilities"]>,
): ModelInfo => ({
  id,
  capabilities: {
    vision: false,
    web: false,
    reasoning: false,
    tools: false,
    image_gen: false,
    video_gen: false,
    audio_gen: false,
    music_gen: false,
    ...caps,
  },
});

const models = [
  model("image-a", { image_gen: true }),
  model("tts-a", { audio_gen: true }),
  model("music-a", { music_gen: true }),
  model("unknown-a", {}),
];

test("filters strictly by requested capability", () => {
  assert.deepEqual(
    filterModelsByCapability(models, "music_gen").map((m) => m.id),
    ["music-a"],
  );
});

test("always prepends the empty built-in default and hides unknown models", () => {
  assert.deepEqual(
    buildMediaModelOptions(models, "image_gen", "default-image"),
    [
      { value: "", modelId: "default-image" },
      { value: "image-a", modelId: "image-a" },
    ],
  );
});

test("clears saved values absent from the filtered options", () => {
  const options = buildMediaModelOptions(models, "music_gen", "default-music");
  assert.equal(sanitizeMediaModelValue("unknown-a", options), "");
  assert.equal(sanitizeMediaModelValue("music-a", options), "music-a");
  assert.equal(sanitizeMediaModelValue("", options), "");
});
```

- [x] **Step 2: Run the test and verify failure**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/providers/mediaModelOptions.test.ts
```

Expected: FAIL with module-not-found for `mediaModelOptions.ts`.

- [x] **Step 3: Implement the pure module**

Create `apps/desktop/src/lib/providers/mediaModelOptions.ts`:

```typescript
import type { ModelInfo } from "../../types";

export type MediaCapabilityKey =
  | "image_gen"
  | "video_gen"
  | "audio_gen"
  | "music_gen"
  | "vision";

export type MediaModelField =
  | "image_model"
  | "video_model"
  | "tts_model"
  | "music_model"
  | "vision_model";

export type MediaModelOption = {
  value: string;
  modelId: string;
};

export const MEDIA_CAPABILITY_BY_FIELD: Record<
  MediaModelField,
  MediaCapabilityKey
> = {
  image_model: "image_gen",
  video_model: "video_gen",
  tts_model: "audio_gen",
  music_model: "music_gen",
  vision_model: "vision",
};

export function filterModelsByCapability(
  models: ModelInfo[],
  capability: MediaCapabilityKey,
): ModelInfo[] {
  return models.filter((model) => model.capabilities?.[capability] === true);
}

export function buildMediaModelOptions(
  models: ModelInfo[],
  capability: MediaCapabilityKey,
  defaultModelId: string,
): MediaModelOption[] {
  return [
    { value: "", modelId: defaultModelId },
    ...filterModelsByCapability(models, capability).map((model) => ({
      value: model.id,
      modelId: model.id,
    })),
  ];
}

export function sanitizeMediaModelValue(
  value: string,
  options: MediaModelOption[],
): string {
  const trimmed = value.trim();
  if (!trimmed) return "";
  return options.some((option) => option.value === trimmed) ? trimmed : "";
}
```

- [x] **Step 4: Run the focused test**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/providers/mediaModelOptions.test.ts
```

Expected: 3 tests PASS.

- [x] **Step 5: Commit the pure selector logic**

```bash
git add apps/desktop/src/lib/providers/mediaModelOptions.ts apps/desktop/src/lib/providers/mediaModelOptions.test.ts
git commit -m "feat(providers): add media model filtering helpers"
```

---

### Task 3: Carry `music_model` through provider persistence and runtime transport

**Files:**
- Modify: `apps/desktop/src-tauri/src/providers_commands.rs`
- Modify: `proto/proto/astro.proto`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-tools/src/engine/context.rs`
- Modify: Rust tests and constructors reported by `rg "ImageGenCreds \\{|ImageGenTargets::from_parts" --glob '*.rs'`

**Interfaces:**
- Produces: optional persisted `ProviderConfig.music_model`, DTO/input `music_model`, `ImageGenTarget.music_model`, protobuf `ChatRequest.image_gen_music_model`, and runtime `ImageGenCreds.music_model`.
- Consumed by: Task 4 `music_gen` model resolution and Task 5 frontend save payload.

- [x] **Step 1: Add failing provider backward-compatibility and default-resolution tests**

In `apps/desktop/src-tauri/src/providers_commands.rs` tests, add:

```rust
#[test]
fn provider_without_music_model_deserializes_to_empty() {
    let json = r#"{
      "id":"google-1","kind":"google","display_name":"Google",
      "endpoint":"https://generativelanguage.googleapis.com/v1beta/openai",
      "model":"gemini-3.5-flash","enabled":true
    }"#;
    let provider: ProviderConfig = serde_json::from_str(json).unwrap();
    assert_eq!(provider.music_model, "");
}

#[test]
fn google_music_model_defaults_to_lyria_clip() {
    assert_eq!(
        default_music_model_for_kind(&ProviderKind::Google),
        "lyria-3-clip-preview"
    );
    assert_eq!(default_music_model_for_kind(&ProviderKind::Openai), "");
}
```

- [x] **Step 2: Run the focused provider tests and verify failure**

Run:

```bash
cargo test -p astro-agent --lib provider_without_music_model -- --nocapture
cargo test -p astro-agent --lib google_music_model_defaults -- --nocapture
```

Expected: FAIL because the field and helper do not exist.

- [x] **Step 3: Add the persisted field and resolved target**

In `providers_commands.rs`, mirror `vision_model` across `ProviderConfig`, `ProviderConfig::new`, `ProviderConfigDto`, `ProviderConfigInput`, `to_dto`, and `save_provider`:

```rust
#[serde(default, skip_serializing_if = "String::is_empty")]
pub music_model: String,
```

Add it to `ImageGenTarget` and resolve only for Google:

```rust
fn default_music_model_for_kind(kind: &ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Google => "lyria-3-clip-preview",
        _ => "",
    }
}

music_model: resolve_media_model(
    &provider.music_model,
    default_music_model_for_kind(&provider.kind),
),
```

- [x] **Step 4: Add protobuf and runtime credential fields**

Append to `ChatRequest` in `proto/proto/astro.proto` without renumbering existing fields:

```protobuf
// Google 音乐生成模型覆盖（空=lyria-3-clip-preview）
string image_gen_music_model = 27;
```

In `crates/agent-tools/src/engine/context.rs`, add:

```rust
/// 音乐生成模型（Google Lyria）；空则使用 clip 默认。
pub music_model: String,
```

Add `music_model: &str` after `video_model: &str` in `ImageGenTargets::from_parts`; set the primary field with `music_model.trim().to_string()` and the fallback field to `String::new()`.

- [x] **Step 5: Repair every compiler-identified constructor and transport call**

Use:

```bash
rg "ImageGenCreds \\{|ImageGenTargets::from_parts|ChatRequest \\{" --glob '*.rs'
```

For Tauri `apps/desktop/src-tauri/src/commands.rs`, populate:

```rust
image_gen_music_model: primary
    .map(|target| target.music_model.clone())
    .unwrap_or_default(),
```

For `crates/agent-server/src/grpc/astro_service.rs`, pass:

```rust
&req.image_gen_music_model,
```

For direct test `ImageGenCreds` literals, use:

```rust
music_model: String::new(),
```

- [x] **Step 6: Run formatting, focused tests, and workspace check**

Run:

```bash
cargo fmt --all -- --check
cargo test -p astro-agent --lib providers_commands::tests -- --nocapture
cargo check -p proto -p tools -p backend -p astro-agent
```

Expected: formatting check, tests, and all four package checks PASS.

- [x] **Step 7: Commit the transport slice**

```bash
git add apps/desktop/src-tauri/src/providers_commands.rs proto/proto/astro.proto apps/desktop/src-tauri/src/commands.rs backend/src/grpc/astro_service.rs tools/src/engine/context.rs
git add $(rg -l "music_model:" --glob '*.rs')
git commit -m "feat(providers): carry configured music model to tools"
```

---

### Task 4: Make the existing `music_gen` tool honor provider configuration

**Files:**
- Modify: `crates/agent-tools/src/builtin/media/music_gen.rs`

**Interfaces:**
- Consumes: `ImageGenCreds.music_model` from Task 3.
- Produces: `validate_music_gen_args(args, configured_music_model)` with precedence `explicit arg > configured model > clip default`.

- [x] **Step 1: Add failing precedence tests and update existing call sites**

Change test calls to pass `""`, then add:

```rust
#[test]
fn configured_model_is_used_when_tool_arg_is_missing() {
    let args = MusicGenArgs {
        prompt: "piano".into(),
        model: None,
        reference_images: None,
        format: None,
    };
    let (model, _) =
        validate_music_gen_args(&args, "lyria-3-pro-preview").unwrap();
    assert_eq!(model, "lyria-3-pro-preview");
}

#[test]
fn explicit_alias_overrides_configured_model() {
    let args = MusicGenArgs {
        prompt: "piano".into(),
        model: Some("clip".into()),
        reference_images: None,
        format: None,
    };
    let (model, _) =
        validate_music_gen_args(&args, "lyria-3-pro-preview").unwrap();
    assert_eq!(model, "lyria-3-clip-preview");
}

#[test]
fn empty_configuration_falls_back_to_clip() {
    let args = MusicGenArgs {
        prompt: "piano".into(),
        model: None,
        reference_images: None,
        format: None,
    };
    let (model, _) = validate_music_gen_args(&args, "").unwrap();
    assert_eq!(model, "lyria-3-clip-preview");
}
```

- [x] **Step 2: Run focused tests and verify failure**

Run:

```bash
cargo test -p tools --lib music_gen::tests -- --nocapture
```

Expected: FAIL because `validate_music_gen_args` accepts one argument and ignores configuration.

- [x] **Step 3: Implement model precedence**

Change the function signature and alias source:

```rust
pub fn validate_music_gen_args(
    args: &MusicGenArgs,
    configured_music_model: &str,
) -> anyhow::Result<(String, MusicAudioFormat)> {
    if args.prompt.trim().is_empty() {
        anyhow::bail!("music_gen 需要 prompt");
    }
    let requested = args
        .model
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let configured = configured_music_model.trim();
    let model_source = requested
        .or_else(|| (!configured.is_empty()).then_some(configured))
        .unwrap_or("clip");
    let model_id = resolve_lyria_model_id(model_source)?;
    let is_pro = model_id.contains("pro");

    let fmt_raw = args
        .format
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("mp3")
        .to_ascii_lowercase();
    let format = match fmt_raw.as_str() {
        "mp3" => MusicAudioFormat::Mp3,
        "wav" => MusicAudioFormat::Wav,
        other => anyhow::bail!("format 无效: {other}（mp3 | wav）"),
    };
    if format == MusicAudioFormat::Wav && !is_pro {
        anyhow::bail!("wav 仅 lyria-3-pro 支持（请设 model=pro）");
    }

    if let Some(refs) = &args.reference_images {
        let count = refs.iter().filter(|path| !path.trim().is_empty()).count();
        if count > 10 {
            anyhow::bail!("reference_images 最多 10 张");
        }
    }
    Ok((model_id, format))
}
```

In `dispatch`, obtain credentials before validation:

```rust
let creds = ctx.image_gen_targets.google().ok_or_else(|| {
    anyhow::anyhow!("music_gen 需要 Google API Key（未配置 Google，且不回退 OpenAI）")
})?;
let (model_id, format) =
    validate_music_gen_args(&parsed, &creds.music_model)?;
```

- [x] **Step 4: Run tool and provider tests**

Run:

```bash
cargo test -p tools --lib music_gen::tests -- --nocapture
cargo test -p providers --lib interactions_http::tests::resolve_lyria -- --nocapture
```

Expected: all tests PASS.

- [x] **Step 5: Commit the tool behavior**

```bash
git add tools/src/builtin/media/music_gen.rs
git commit -m "feat(tools): honor configured music generation model"
```

---

### Task 5: Replace media text fields with capability-filtered selectors

**Files:**
- Modify: `apps/desktop/src/components/settings/ProvidersPanel.tsx`
- Modify: `apps/desktop/src/types.ts`
- Modify: `apps/desktop/src/i18n/messages.ts`
- Modify: `apps/desktop/src/styles/features/providers.css`
- Test: `apps/desktop/src/lib/providers/mediaModelOptions.test.ts`

**Interfaces:**
- Consumes: Task 2 option helpers and Task 3 `ProviderDto.music_model`.
- Produces: five media selectors, post-resolution invalid-value cleanup, and provider save payload including `music_model`.

- [x] **Step 1: Extend the selector tests for all five field mappings**

Add to `mediaModelOptions.test.ts`:

```typescript
import { MEDIA_CAPABILITY_BY_FIELD } from "./mediaModelOptions.ts";

test("maps every media field to its independent capability", () => {
  assert.deepEqual(MEDIA_CAPABILITY_BY_FIELD, {
    image_model: "image_gen",
    video_model: "video_gen",
    tts_model: "audio_gen",
    music_model: "music_gen",
    vision_model: "vision",
  });
});
```

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/providers/mediaModelOptions.test.ts
```

Expected: PASS, proving the helper contract before component integration.

- [x] **Step 2: Extend DTO and draft state**

In `apps/desktop/src/types.ts` add:

```typescript
/** 音乐生成模型（空=内置默认；Google only） */
music_model?: string;
```

In `ProvidersPanel.tsx`, add `music_model` to `Draft`, `draftFromProvider`, `providerSaveInput`, and the selected-field synchronization effect. Extend the defaults shape:

```typescript
google: {
  image: "gemini-3.1-flash-image",
  video: "veo-3.1-generate-preview",
  tts: "gemini-3.1-flash-tts-preview",
  music: "lyria-3-clip-preview",
  vision: "gemini-3.5-flash",
},
openai: {
  image: "gpt-image-2",
  video: "",
  tts: "gpt-4o-mini-tts",
  music: "",
  vision: "gpt-4o",
},
```

- [x] **Step 3: Track when model metadata has resolved**

Add:

```typescript
const [modelsResolved, setModelsResolved] = useState(false);
```

Set it to `false` when the selected provider changes. Set it to `true` after a cache hit, after `list_provider_models` succeeds or fails, and immediately when the provider cannot list models because it has no key. Do not sanitize while it is false.

- [x] **Step 4: Build memoized options and sanitize invalid saved values**

Import Task 2 helpers and create a local builder:

```typescript
const mediaOptions = useCallback(
  (capability: MediaCapabilityKey, defaultModelId: string) =>
    buildMediaModelOptions(models, capability, defaultModelId).map((option) => ({
      value: option.value,
      label:
        option.value === ""
          ? t("providers.mediaDefaultOption", { model: option.modelId })
          : option.modelId,
      icon: option.value ? <ModelBrandIcon modelId={option.modelId} /> : undefined,
    })),
  [models, t],
);
```

After options are defined, sanitize only once metadata is resolved:

```typescript
useEffect(() => {
  if (!modelsResolved || !selected || !draft) return;
  const defaults = MEDIA_MODEL_DEFAULTS[selected.kind];
  if (!defaults) return;
  setDraft((current) => {
    if (!current) return current;
    const next = {
      ...current,
      image_model: sanitizeMediaModelValue(
        current.image_model,
        buildMediaModelOptions(models, "image_gen", defaults.image),
      ),
      tts_model: sanitizeMediaModelValue(
        current.tts_model,
        buildMediaModelOptions(models, "audio_gen", defaults.tts),
      ),
      vision_model: sanitizeMediaModelValue(
        current.vision_model,
        buildMediaModelOptions(models, "vision", defaults.vision),
      ),
      video_model:
        selected.kind === "google"
          ? sanitizeMediaModelValue(
              current.video_model,
              buildMediaModelOptions(models, "video_gen", defaults.video),
            )
          : "",
      music_model:
        selected.kind === "google"
          ? sanitizeMediaModelValue(
              current.music_model,
              buildMediaModelOptions(models, "music_gen", defaults.music),
            )
          : "",
    };
    return JSON.stringify(next) === JSON.stringify(current) ? current : next;
  });
}, [models, modelsResolved, selected?.id, selected?.kind]);
```

- [x] **Step 5: Replace the four inputs and add the Google music selector**

For each media field, render `SelectMenu` with `className="providers-media-model-select"`, the draft value, translated aria label, and corresponding filtered options. The image selector pattern is:

```tsx
<SelectMenu
  className="providers-media-model-select"
  value={draft.image_model}
  aria-label={t("providers.imageModel")}
  onChange={(value) =>
    setDraft((current) =>
      current ? { ...current, image_model: value } : current,
    )
  }
  options={mediaOptions(
    "image_gen",
    MEDIA_MODEL_DEFAULTS[selected.kind]?.image ?? "",
  )}
/>
```

Use `video_gen`, `audio_gen`, `music_gen`, and `vision` for the other fields. Render video and music only for Google. Do not insert the current value as an orphan option.

- [x] **Step 6: Add translations and selector styling**

Add bilingual keys:

```typescript
"providers.musicModel": "音乐生成模型",
"providers.mediaDefaultOption": "内置默认（{model}）",
```

```typescript
"providers.musicModel": "Music generation model",
"providers.mediaDefaultOption": "Built-in default ({model})",
```

In `apps/desktop/src/styles/features/providers.css`, make the SelectMenu fill the existing field:

```css
.providers-media-model-select {
  width: 100%;
  min-width: 0;
}

.providers-media-model-select .select-menu-trigger {
  width: 100%;
  min-height: 42px;
  justify-content: space-between;
}
```

- [x] **Step 7: Run frontend tests, typecheck, and lints**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/providers/mediaModelOptions.test.ts src/lib/model/modelCaps.test.ts src/lib/model/autoModelSelect.test.ts src/lib/chat/shouldShowThinkingControls.test.ts
cd frontend && npx tsc -b --pretty false
```

Expected: all Node tests PASS and TypeScript exits 0. Then inspect IDE diagnostics for the five changed frontend files and fix only newly introduced errors.

- [x] **Step 8: Commit the UI slice**

```bash
git add apps/desktop/src/components/settings/ProvidersPanel.tsx apps/desktop/src/types.ts apps/desktop/src/i18n/messages.ts apps/desktop/src/styles/features/providers.css apps/desktop/src/lib/providers/mediaModelOptions.test.ts
git commit -m "feat(providers): select media models by capability"
```

---

### Task 6: Verify the complete media-model flow

**Files:**
- Verify only; modify only files implicated by a failing check.

**Interfaces:**
- Consumes: Tasks 1–5.
- Produces: a verified end-to-end capability-filtered configuration path.

- [x] **Step 1: Run all focused frontend tests**

```bash
cd frontend && node --experimental-strip-types --test \
  src/lib/providers/mediaModelOptions.test.ts \
  src/lib/model/modelCaps.test.ts \
  src/lib/model/autoModelSelect.test.ts \
  src/lib/chat/shouldShowThinkingControls.test.ts \
  src/lib/media/parseGeneratedMedia.test.ts
```

Expected: all tests PASS.

- [x] **Step 2: Run focused Rust tests**

```bash
cargo test -p astro-agent --lib model_meta::tests -- --nocapture
cargo test -p astro-agent --lib providers_commands::tests -- --nocapture
cargo test -p tools --lib music_gen::tests -- --nocapture
cargo test -p providers --lib interactions_http::tests::resolve_lyria -- --nocapture
```

Expected: all tests PASS.

- [x] **Step 3: Run formatting and compilation checks**

```bash
cargo fmt --all -- --check
cargo check -p proto -p providers -p tools -p backend -p astro-agent
cd frontend && npx tsc -b --pretty false
```

Expected: every command exits 0.

- [x] **Step 4: Perform desktop smoke checks**

In the Tauri application:

1. Open a Google provider with a valid key and refresh models.
2. Confirm each selector shows the built-in default plus only models carrying its exact capability.
3. Confirm unknown models never appear and an invalid prior value clears to the default option after model resolution.
4. Confirm Google shows video and music selectors; OpenAI shows neither.
5. Save `lyria-3-pro-preview`, start a new Agent run, and invoke `music_gen` without `model`; verify output reports `model=lyria-3-pro-preview`.
6. Invoke `music_gen` with `model=clip`; verify output reports `model=lyria-3-clip-preview`.

- [x] **Step 5: Commit only if verification required fixes**

```bash
git status --short
git diff
git add apps/desktop/src-tauri/src/litellm_meta.rs apps/desktop/src-tauri/src/model_meta.rs apps/desktop/src-tauri/src/providers_commands.rs apps/desktop/src-tauri/src/commands.rs proto/proto/astro.proto backend/src/grpc/astro_service.rs tools/src/engine/context.rs tools/src/builtin/media/music_gen.rs apps/desktop/src/types.ts apps/desktop/src/lib/model/modelCaps.ts apps/desktop/src/lib/model/modelCaps.test.ts apps/desktop/src/lib/providers/mediaModelOptions.ts apps/desktop/src/lib/providers/mediaModelOptions.test.ts apps/desktop/src/components/agents/ModelCapabilityIcons.tsx apps/desktop/src/components/settings/ProvidersPanel.tsx apps/desktop/src/i18n/messages.ts apps/desktop/src/styles/features/providers.css
git commit -m "fix(providers): address media selector verification"
```

If no files changed, do not create an empty commit.

