# Storyboard video_gen + Skill Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expand `video_gen` with style / extend / reference / first+last frame; ship bundled `storyboard-video` Skill seeded into public skills.

**Architecture:** Extend `google_openai_generate_video` with optional text + base64 image fields (OpenAI-compat `…/videos`); tool resolves workspace paths; skills crate embeds SKILL.md and copies on startup seed.

**Tech Stack:** Rust `providers`/`tools`/`skills`, Tauri seed hook, frontend `useAgentTools`.

**参考:** `docs/superpowers/specs/2026-07-15-storyboard-video-skill-design.md`

## Global Constraints

- Same `video_gen` toolset (no new toolset / Agent).
- Params: style, extend_video_id, reference_image, image, last_frame (+ existing).
- `last_frame` requires `image`; duration soft-set to 8 when refs/extend/interpolation used.
- Bundled skill local copy — not GitHub npx.

## File Structure

| File | Responsibility |
|------|----------------|
| `providers/.../media_http.rs` | Advanced form fields |
| `tools/.../video_gen.rs` | Args, path→base64, validation |
| `skills/bundled/storyboard-video/SKILL.md` | Skill body |
| `skills/src/bundled_seed.rs` (+ lib export) | Seed from embed |
| `apps/desktop/.../default_skills_seed.rs` | Call bundled seed |
| `useAgentTools.ts` / i18n | UI params |

---

### Task 1: media_http advanced fields

- Extend `google_openai_generate_video` with options struct or extra params: `style`, `extend_video_id`, `image_b64` (+mime), `last_frame_b64`, `reference_image_b64`.
- Append as multipart `.text(...)` keys per OpenAI-compat table (`image`, `style`, `extend_video_id`; `last_frame` / `reference_images` as JSON string if needed).
- Commit: `feat(providers): pass Veo style/extend/reference/frame fields`

### Task 2: video_gen tool

- Add args; validate last_frame⇒image; read files; if any of ref/extend/frames set and duration missing, default `duration_seconds=8`.
- Update description + useAgentTools + i18n.
- Commit: `feat(tools): wire advanced video_gen inputs`

### Task 3: bundled storyboard-video skill

- Write `skills/bundled/storyboard-video/SKILL.md`.
- `seed_bundled_skills()` via `include_str!`; invoke from startup seed + unit test.
- Commit: `feat(skills): ship and seed storyboard-video skill`

### Task 4: mark spec implemented

- Update design status; commit docs.
