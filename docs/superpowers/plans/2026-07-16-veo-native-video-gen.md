# Veo 原生 video_gen Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `video_gen` 改为 Google Veo 原生 `predictLongRunning` 优先，失败回退 OpenAI 兼容 `/videos`，并完整支持多参考图与本地/URI/id 续拍。

**Architecture:** 在 `providers::media_http` 增加可单测的 URL/请求体/响应解析助手与 `google_native_generate_video`；保留并扩展 `google_openai_generate_video`；`tools` 层负责参数合并、互斥校验、路径读入、native→compat 回退与落盘。

**Tech Stack:** Rust (`providers` / `tools`)、`reqwest`、`serde_json`、`base64`、schemars；参考官方 REST：[Veo](https://ai.google.dev/gemini-api/docs/veo?hl=zh-cn#rest)。

**参考规格:** `docs/superpowers/specs/2026-07-16-veo-native-video-gen-design.md`

## Global Constraints

- Google 原生：`POST {v1beta}/models/{model}:predictLongRunning`，Header `x-goog-api-key`（不用 Bearer）。
- 轮询：`GET {v1beta}/{operation.name}`，间隔 10s，总超时 10 分钟。
- 下载：`GET video.uri` + `x-goog-api-key`，跟随重定向。
- 默认模型：`veo-3.1-generate-preview`；可读 Google `video_model`。
- 策略：**原生优先 → OpenAI 兼容回退**（与 image 的「兼容优先」相反）。
- 续拍优先级：`extend_video`（本地）→ `extend_video_uri`（先下载再 inline）→ `extend_video_id`（仅兼容）。
- `reference_images` 最多 3；旧字段 `reference_image` 并入数组。
- `negative_prompt` / `style`：仅兼容路径转发；原生忽略并在工具结果注明。
- 本轮不接 Files API、非 Google 视频厂商、独立视频 UI。

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/src/protocol/media_http.rs` | Veo 原生 URL/body/parse、`google_native_generate_video`；扩展 `VideoGenExtras`/`GeneratedVideo`；兼容路径多参考图 |
| `tools/src/builtins/media/video_gen.rs` | 新参数契约、校验、native→compat、落盘与 hint |
| `tools/src/builtins/media/image_gen.rs` | 成功 hint 中的 `reference_image` → `reference_images` |
| `apps/desktop/src/i18n/messages.ts` | 更新 `agentTools.videoGen.desc` 中英 |
| `skills/bundled/storyboard-video/SKILL.md` | 续拍改推 `extend_video`；多参考图；`astro_bundled_rev` +1 |

---

### Task 1: Veo 原生 URL / 请求体 / 响应解析（纯函数 + 单测）

**Files:**
- Modify: `providers/src/protocol/media_http.rs`

**Interfaces:**
- Produces:
  - `pub fn google_v1beta_root(config: &ProviderConfig) -> String` — 始终以 `…/v1beta` 结尾（从 `google_native_base` 推导）
  - `pub fn google_veo_predict_url(config: &ProviderConfig, model: &str) -> String`
  - `pub fn google_operation_url(config: &ProviderConfig, operation_name: &str) -> String`
  - `pub fn build_veo_predict_body(prompt: &str, extras: &VideoGenExtras) -> Value`
  - `pub fn extract_veo_operation_name(create_body: &Value) -> Result<String>`
  - `pub fn extract_veo_video_uri(status_body: &Value) -> Result<String>` — `done`+error / 缺 sample 均 Err
- Consumes: 现有 `google_native_base`、`VideoGenExtras`（本 Task 先把 `reference_images: Vec<VideoImagePart>` 换掉旧 `reference_image`，并增加 `extend_video: Option<VideoImagePart>`、`extend_video_uri: Option<String>`；`extend_video_id` 保留）
- 同步改 `GeneratedVideo`：增加 `pub video_uri: Option<String>`；现有构造处补 `video_uri: None` 或实际 URI

- [ ] **Step 1: 扩展类型（先改签名，让编译指向改动点）**

将 `VideoGenExtras` 改为：

```rust
#[derive(Debug, Clone, Default)]
pub struct VideoGenExtras {
    pub aspect_ratio: Option<String>,
    pub duration_seconds: Option<u32>,
    pub resolution: Option<String>,
    pub negative_prompt: Option<String>,
    pub style: Option<String>,
    pub extend_video_id: Option<String>,
    pub extend_video_uri: Option<String>,
    /// 续拍视频字节（本地文件或已下载 URI）；mime 通常 `video/mp4`
    pub extend_video: Option<VideoImagePart>,
    pub person_generation: Option<String>,
    pub seed: Option<i64>,
    pub image: Option<VideoImagePart>,
    pub last_frame: Option<VideoImagePart>,
    pub reference_images: Vec<VideoImagePart>,
}
```

```rust
#[derive(Debug, Clone)]
pub struct GeneratedVideo {
    pub data: Vec<u8>,
    pub mime_type: String,
    pub operation_id: String,
    pub video_uri: Option<String>,
}
```

暂时把 `google_openai_generate_video` 里旧的 `reference_image` 用法改为遍历 `reference_images`（可先只循环一遍，行为与后 Task 一致）。修复所有构造 `GeneratedVideo` / `VideoGenExtras` 的编译错误。

- [ ] **Step 2: 写失败测试（`media_http.rs` 底部 `#[cfg(test)]`）**

```rust
#[test]
fn google_v1beta_root_strips_openai_suffix() {
    let cfg = ProviderConfig {
        api_key: "k".into(),
        base_url: Some(
            "https://generativelanguage.googleapis.com/v1beta/openai".into(),
        ),
        model: String::new(),
        ..ProviderConfig::default()
    };
    assert_eq!(
        google_v1beta_root(&cfg),
        "https://generativelanguage.googleapis.com/v1beta"
    );
    assert_eq!(
        google_veo_predict_url(&cfg, "veo-3.1-generate-preview"),
        "https://generativelanguage.googleapis.com/v1beta/models/veo-3.1-generate-preview:predictLongRunning"
    );
    assert_eq!(
        google_operation_url(&cfg, "operations/abc123"),
        "https://generativelanguage.googleapis.com/v1beta/operations/abc123"
    );
}

#[test]
fn build_veo_predict_body_text_and_params() {
    let extras = VideoGenExtras {
        aspect_ratio: Some("9:16".into()),
        resolution: Some("4K".into()),
        duration_seconds: Some(8),
        person_generation: Some("allow_adult".into()),
        seed: Some(7),
        ..Default::default()
    };
    let body = build_veo_predict_body("a lion", &extras);
    assert_eq!(body["instances"][0]["prompt"], "a lion");
    assert_eq!(body["parameters"]["aspectRatio"], "9:16");
    assert_eq!(body["parameters"]["resolution"], "4k");
    assert_eq!(body["parameters"]["durationSeconds"], 8);
    assert_eq!(body["parameters"]["personGeneration"], "allow_adult");
    assert_eq!(body["parameters"]["seed"], 7);
    assert!(body["instances"][0].get("image").is_none());
}

#[test]
fn build_veo_predict_body_frames_refs_and_extend() {
    let img = VideoImagePart {
        bytes: b"PNG".to_vec(),
        filename: "a.png".into(),
        mime: "image/png".into(),
    };
    let vid = VideoImagePart {
        bytes: b"MP4".to_vec(),
        filename: "v.mp4".into(),
        mime: "video/mp4".into(),
    };
    let extras = VideoGenExtras {
        image: Some(img.clone()),
        last_frame: Some(img.clone()),
        reference_images: vec![img.clone(), img],
        extend_video: Some(vid),
        ..Default::default()
    };
    let body = build_veo_predict_body("interp", &extras);
    let inst = &body["instances"][0];
    assert!(inst["image"]["inlineData"]["data"].as_str().unwrap().len() > 0);
    assert!(inst["lastFrame"]["inlineData"]["data"].is_string());
    assert_eq!(inst["referenceImages"].as_array().unwrap().len(), 2);
    assert_eq!(inst["referenceImages"][0]["referenceType"], "asset");
    assert!(inst["video"]["inlineData"]["data"].is_string());
    assert_eq!(inst["video"]["inlineData"]["mimeType"], "video/mp4");
}

#[test]
fn extract_veo_video_uri_success_and_error() {
    let ok = serde_json::json!({
        "done": true,
        "response": {
            "generateVideoResponse": {
                "generatedSamples": [{
                    "video": { "uri": "https://example.com/v.mp4" }
                }]
            }
        }
    });
    assert_eq!(
        extract_veo_video_uri(&ok).unwrap(),
        "https://example.com/v.mp4"
    );
    let err = serde_json::json!({
        "done": true,
        "error": { "message": "blocked" }
    });
    assert!(extract_veo_video_uri(&err).unwrap_err().to_string().contains("blocked"));
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test -p providers google_v1beta_root_strips_openai_suffix build_veo_predict_body extract_veo_video_uri -- --nocapture`

Expected: FAIL（函数未定义或旧字段名）

- [ ] **Step 4: 实现纯函数**

```rust
pub fn google_v1beta_root(config: &ProviderConfig) -> String {
    let native = trim_slash(&google_native_base(config));
    if native.ends_with("/v1beta") {
        native
    } else if native.contains("/v1beta/") {
        // 罕见：已含更深路径时仍截到 v1beta
        let idx = native.find("/v1beta").unwrap();
        format!("{}{}", &native[..idx], "/v1beta")
    } else {
        format!("{native}/v1beta")
    }
}

pub fn google_veo_predict_url(config: &ProviderConfig, model: &str) -> String {
    format!(
        "{}/models/{}:predictLongRunning",
        google_v1beta_root(config),
        model.trim()
    )
}

pub fn google_operation_url(config: &ProviderConfig, operation_name: &str) -> String {
    let name = operation_name.trim().trim_start_matches('/');
    format!("{}/{}", google_v1beta_root(config), name)
}

fn inline_data_value(part: &VideoImagePart) -> Value {
    json!({
        "inlineData": {
            "mimeType": part.mime,
            "data": base64::engine::general_purpose::STANDARD.encode(&part.bytes),
        }
    })
}

pub fn build_veo_predict_body(prompt: &str, extras: &VideoGenExtras) -> Value {
    let mut instance = json!({ "prompt": prompt });
    if let Some(img) = &extras.image {
        instance["image"] = inline_data_value(img);
    }
    if let Some(img) = &extras.last_frame {
        instance["lastFrame"] = inline_data_value(img);
    }
    if !extras.reference_images.is_empty() {
        instance["referenceImages"] = Value::Array(
            extras
                .reference_images
                .iter()
                .map(|img| {
                    json!({
                        "image": inline_data_value(img),
                        "referenceType": "asset",
                    })
                })
                .collect(),
        );
    }
    if let Some(vid) = &extras.extend_video {
        instance["video"] = inline_data_value(vid);
    }

    let mut parameters = serde_json::Map::new();
    if let Some(ar) = extras.aspect_ratio.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        parameters.insert("aspectRatio".into(), json!(ar));
    }
    if let Some(sec) = extras.duration_seconds.filter(|s| *s > 0) {
        parameters.insert("durationSeconds".into(), json!(sec));
    }
    if let Some(res) = extras.resolution.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        let normalized = if res.eq_ignore_ascii_case("4k") {
            "4k".to_string()
        } else {
            res.to_ascii_lowercase()
        };
        parameters.insert("resolution".into(), json!(normalized));
    }
    if let Some(pg) = extras
        .person_generation
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        parameters.insert("personGeneration".into(), json!(pg));
    }
    if let Some(seed) = extras.seed {
        parameters.insert("seed".into(), json!(seed));
    }

    let mut body = json!({ "instances": [instance] });
    if !parameters.is_empty() {
        body["parameters"] = Value::Object(parameters);
    }
    body
}

pub fn extract_veo_operation_name(create_body: &Value) -> Result<String> {
    create_body
        .get("name")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 响应缺少 operation name"))
}

pub fn extract_veo_video_uri(status_body: &Value) -> Result<String> {
    if status_body.get("done") != Some(&json!(true))
        && status_body.pointer("/done").and_then(|d| d.as_bool()) != Some(true)
    {
        anyhow::bail!("Veo operation 尚未完成");
    }
    if let Some(msg) = status_body
        .pointer("/error/message")
        .and_then(|m| m.as_str())
        .or_else(|| status_body.get("error").and_then(|e| e.as_str()))
    {
        anyhow::bail!("Veo 生成失败: {msg}");
    }
    status_body
        .pointer("/response/generateVideoResponse/generatedSamples/0/video/uri")
        .and_then(|u| u.as_str())
        .or_else(|| {
            status_body
                .pointer("/response/generate_video_response/generated_samples/0/video/uri")
                .and_then(|u| u.as_str())
        })
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("Veo 完成但缺少 video.uri"))
}
```

注意：`build_veo_predict_body` **不**把 `negative_prompt`/`style`/`extend_video_id`/`extend_video_uri` 写入原生 body（URI 由调用方先下载填入 `extend_video`）。

- [ ] **Step 5: 跑测试通过并提交**

Run: `cargo test -p providers google_v1beta_root_strips_openai_suffix build_veo_predict_body extract_veo_video_uri -- --nocapture`

Expected: PASS

```bash
git add providers/src/protocol/media_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): add Veo native request helpers and extras types

Pure URL/body/parse helpers unlock native predictLongRunning without yet
wiring the HTTP loop.
EOF
)"
```

---

### Task 2: `google_native_generate_video` HTTP 循环 + 兼容多参考图

**Files:**
- Modify: `providers/src/protocol/media_http.rs`

**Interfaces:**
- Produces:
  - `pub async fn google_native_generate_video(client: &Client, prompt: &str, config: &ProviderConfig, extras: &VideoGenExtras, on_progress: Option<&mut dyn FnMut(&str)>) -> Result<GeneratedVideo>`
- Consumes: Task 1 全部助手；下载 URI 时若 `extras.extend_video` 为空且 `extend_video_uri` 有值，先 GET 该 URI（带 API key）填入临时 `VideoImagePart` 再 `build_veo_predict_body`
- 修改 `google_openai_generate_video`：对 `extras.reference_images` 逐个 `multipart_image("reference_images", …)`；`GeneratedVideo.video_uri` 在兼容完成时可填响应 `url`

- [ ] **Step 1: 写单测覆盖「URI 下载后进 body」的同步准备逻辑（可选薄函数）**

若抽取：

```rust
pub fn veo_extras_with_inline_extend(extras: &VideoGenExtras, downloaded: Option<VideoImagePart>) -> VideoGenExtras
```

则测：传入 `extend_video_uri` + downloaded bytes → body 含 `video.inlineData`。若不抽取，可跳过本步，直接实现 HTTP（仍须用 Task 1 测试兜底 body）。

- [ ] **Step 2: 实现 `google_native_generate_video`**

要点（实现时按此顺序）：

1. 校验 `api_key` 非空；`model` 默认 `default_video_model()`。
2. 若 `extras.extend_video.is_none()` 且 `extend_video_uri` 非空：  
   `GET uri` + header `x-goog-api-key` → bytes → `VideoImagePart { mime: "video/mp4", filename: "extend.mp4", bytes }`；合并进用于 body 的 extras 副本。
3. `POST google_veo_predict_url`，headers：`x-goog-api-key`、`content-type: application/json`，body = `build_veo_predict_body`。
4. 解析 `extract_veo_operation_name`；progress：`status=queued operation_name=…`。
5. 循环：`GET google_operation_url`；若 `done` 则 `extract_veo_video_uri`；否则 sleep 10s；超时 10min。
6. `GET uri` + `x-goog-api-key` 下载；返回：

```rust
Ok(GeneratedVideo {
    data: bytes.to_vec(),
    mime_type: "video/mp4".into(),
    operation_id: op_name,
    video_uri: Some(uri),
})
```

错误：create/poll/download HTTP 非成功时读 `/error/message` 后 `bail!`。

- [ ] **Step 3: 更新兼容路径多参考图与 `video_uri`**

在 `google_openai_generate_video` 中：

```rust
for img in extras.reference_images.clone() {
    let (name, part) = multipart_image("reference_images", img)?;
    form = form.part(name, part);
}
```

完成下载后：

```rust
return Ok(GeneratedVideo {
    data: bytes.to_vec(),
    mime_type: "video/mp4".to_string(),
    operation_id: op_id,
    video_uri: Some(url.to_string()),
});
```

- [ ] **Step 4: 编译与现有 providers 测试**

Run: `cargo test -p providers --lib`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add providers/src/protocol/media_http.rs
git commit -m "$(cat <<'EOF'
feat(providers): add google_native_generate_video polling loop

Native Veo predictLongRunning with API-key download; OpenAI compat
path accepts multiple reference_images parts.
EOF
)"
```

---

### Task 3: `video_gen` 工具 — 参数、校验、native→compat、落盘

**Files:**
- Modify: `tools/src/builtins/media/video_gen.rs`
- Modify: `tools/src/builtins/media/image_gen.rs`（hint 文案一行）

**Interfaces:**
- Consumes: `google_native_generate_video`、`google_openai_generate_video`、`VideoGenExtras`、`default_video_model`
- Produces: 工具成功字符串含 `api_path=`、`operation_name`/`operation_id`、`video_uri`、`next_shot_hint`（优先 `extend_video`）

- [ ] **Step 1: 更新 `VideoGenArgs`**

```rust
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct VideoGenArgs {
    pub prompt: String,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
    #[serde(default)]
    pub duration_seconds: Option<u32>,
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub negative_prompt: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    #[serde(default)]
    pub extend_video: Option<String>,
    #[serde(default)]
    pub extend_video_uri: Option<String>,
    #[serde(default)]
    pub extend_video_id: Option<String>,
    #[serde(default)]
    pub reference_images: Option<Vec<String>>,
    #[serde(default)]
    pub reference_image: Option<String>,
    #[serde(default)]
    pub image: Option<String>,
    #[serde(default)]
    pub last_frame: Option<String>,
    #[serde(default)]
    pub person_generation: Option<String>,
    #[serde(default)]
    pub seed: Option<i64>,
}
```

工具 description 改为说明：原生 Veo；`extend_video` 优先；`reference_images` 最多 3；兼容回退。

- [ ] **Step 2: 合并参考图 + 校验 + duration 强制**

在 `dispatch` 内：

```rust
let mut refs: Vec<String> = parsed
    .reference_images
    .unwrap_or_default()
    .into_iter()
    .filter_map(|s| {
        let t = s.trim().to_string();
        if t.is_empty() { None } else { Some(t) }
    })
    .collect();
if let Some(one) = opt_path(parsed.reference_image.as_deref()) {
    refs.push(one.to_string());
}
if refs.len() > 3 {
    anyhow::bail!("video_gen: reference_images 最多 3 张");
}

let image_path = opt_path(parsed.image.as_deref());
let last_frame_path = opt_path(parsed.last_frame.as_deref());
let extend_path = opt_path(parsed.extend_video.as_deref());
let extend_uri = opt_path(parsed.extend_video_uri.as_deref());
let extend_id = opt_path(parsed.extend_video_id.as_deref());

if last_frame_path.is_some() && image_path.is_none() {
    anyhow::bail!("video_gen: last_frame 需要同时提供 image");
}
if !refs.is_empty() && (image_path.is_some() || last_frame_path.is_some()) {
    anyhow::bail!("video_gen: reference_images 不能与 image/last_frame 同时使用");
}
let has_extend = extend_path.is_some() || extend_uri.is_some() || extend_id.is_some();
if has_extend && (image_path.is_some() || last_frame_path.is_some() || !refs.is_empty()) {
    anyhow::bail!("video_gen: 续拍不能与 image/last_frame/reference_images 同时使用");
}

let res_lower = parsed
    .resolution
    .as_deref()
    .map(|s| s.trim().to_ascii_lowercase());
let needs_eight = has_extend
    || !refs.is_empty()
    || last_frame_path.is_some()
    || matches!(res_lower.as_deref(), Some("1080p") | Some("4k"));
// duration force + duration_note 同现有逻辑
```

加载：

- `load_image_part` 用于 image / last_frame / refs
- 新增 `load_video_part`（与 `load_image_part` 相同但默认 mime `video/mp4`）用于 `extend_video`

构建 `VideoGenExtras { reference_images: …, extend_video: …, extend_video_uri: …, extend_video_id: …, … }`。

- [ ] **Step 3: native → compat 调用**

```rust
let video = match google_native_generate_video(
    &client, prompt, &config, &extras, Some(&mut on_progress),
).await {
    Ok(v) => {
        on_progress("api_path=native");
        v
    }
    Err(native_err) => {
        on_progress(&format!(
            "fallback=openai_compat reason={}",
            native_err.to_string().replace('\n', " ")
        ));
        google_openai_generate_video(
            &client, prompt, &config, &extras, Some(&mut on_progress),
        )
        .await
        .map_err(|compat_err| {
            anyhow::anyhow!(
                "Google 原生视频失败: {native_err}; 兼容回退失败: {compat_err}"
            )
        })?
    }
};
```

成功文本字段：

- `api_path=native` 或根据 progress / 是否走过 fallback 判断（建议在 match 分支设 `let api_path = "native"|"compat"`）
- `operation_name=` 或 `operation_id=`（可用同一 `video.operation_id` 字段，原生写 name）
- 若有 `video.video_uri`：输出一行 `video_uri=…`
- 若原生且提供了 `negative_prompt`/`style`：`native_ignored=negative_prompt,style`
- `next_shot_hint: video_gen(prompt=\"…\", extend_video=\"{rel}\", duration_seconds=8, …)`

落盘逻辑保持现有 `vid-…mp4`。

- [ ] **Step 4: 更新 image_gen hint**

将 hint 中 `reference_image` 改为 `reference_images`（仍可提单路径字符串放进数组）。

- [ ] **Step 5: 编译相关 crate**

Run: `cargo test -p providers --lib && cargo check -p tools`

Expected: PASS / 无错误

- [ ] **Step 6: Commit**

```bash
git add tools/src/builtins/media/video_gen.rs tools/src/builtins/media/image_gen.rs
git commit -m "$(cat <<'EOF'
feat(tools): route video_gen through native Veo with compat fallback

Upgrade args for multi-reference and extend_video; try predictLongRunning
first, then OpenAI-compatible /videos.
EOF
)"
```

---

### Task 4: 文案与 storyboard skill

**Files:**
- Modify: `apps/desktop/src/i18n/messages.ts`（中英 `agentTools.videoGen.desc`）
- Modify: `skills/bundled/storyboard-video/SKILL.md`（`astro_bundled_rev: 3`）

- [ ] **Step 1: i18n**

中文：`Google Veo 原生生成（失败回退兼容接口）；建议 image_gen 首尾帧，续拍用 extend_video；写入 generated/videos`

英文：`Google Veo native (OpenAI-compat fallback); prefer image_gen frames, extend via extend_video; writes generated/videos`

- [ ] **Step 2: skill**

- 步骤 4 续拍改为：优先把上一段相对路径作 `extend_video=`；可选保留 `extend_video_id` 给回退。
- 参数表：`reference_images`（最多 3）替换/并列旧 `reference_image`；增加 `extend_video` / `extend_video_uri`。
- `astro_bundled_rev`：`2` → `3`。

- [ ] **Step 3: Commit**

```bash
git add apps/desktop/src/i18n/messages.ts skills/bundled/storyboard-video/SKILL.md
git commit -m "$(cat <<'EOF'
docs: align video_gen copy and storyboard skill with Veo native

Point extend and reference_images at the upgraded tool contract.
EOF
)"
```

---

### Task 5: 验收清单（人工 / 有 Key 时）

- [ ] **Step 1: 单元回归**

Run: `cargo test -p providers --lib`

Expected: 全部 PASS，含 Task 1 新增用例。

- [ ] **Step 2: 有 Google Key 时（可选）**

1. Agent 调用 `video_gen` 纯文生 → `api_path=native`、本地 mp4。
2. 用结果路径 `extend_video` 续拍 → 成功。
3. 临时把 model 改成无效值 → 应出现 `fallback=openai_compat` 或双失败合并消息（取决于兼容是否也拒）。

- [ ] **Step 3: 无额外 commit**（除非修 bug）

---

## Spec coverage（自检）

| 规格要求 | Task |
|----------|------|
| 原生 predictLongRunning + 轮询 + key 下载 | 1–2 |
| OpenAI 兼容回退 | 2–3 |
| aspect/resolution/duration/person/seed | 1, 3 |
| 首尾帧 / ≤3 参考图 / 续拍本地+URI+id | 1–3 |
| negative_prompt/style 仅兼容 | 1, 3 |
| progress / next_shot_hint / 落盘 | 3 |
| 单测不打真网 | 1 |
| skill + i18n | 4 |

## Placeholder scan

无 TBD/TODO；函数名与类型与各 Task Interfaces 一致（`google_native_generate_video`、`VideoGenExtras.reference_images`、`GeneratedVideo.video_uri`）。
