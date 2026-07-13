# A2UI Glass Catalog v2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 Astro A2UI 升到 catalog v2：扩展设计系统组件、Soft Dark/Frost 亮暗玻璃拟态、升级 HITL/信息卡模板，并让表单控件真实可交互。

**Architecture:** Rust `a2ui` crate 拥有权威 allowlist + 校验 + 模板；前端 `types.ts` 镜像组件集合，`CatalogAdapter` 用注册表式 switch 渲染；CSS 用 `--a2ui-glass-*` token 跟随 `html[data-theme]`。传输与 interrupt 生命周期不变。

**Tech Stack:** Rust (`a2ui`, `tools`), React 18 + TypeScript, CSS variables, A2UI v0.9 operations JSON

**Spec:** `docs/superpowers/specs/2026-07-13-a2ui-glass-catalog-v2-design.md`

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `a2ui/src/catalog.rs` | `ASTRO_CATALOG_ID` → v2；扩展 `ALLOWED_COMPONENTS` |
| Modify: `a2ui/src/templates.rs` | HITL/info 模板升级；新增 recipe builders |
| Modify: `a2ui/src/lib.rs` | 导出新模板函数（若需要） |
| Modify: `a2ui/tests/validate_test.rs` | v2 组件接受；v1 catalogId / Modal 拒绝 |
| Modify: `a2ui/tests/templates_test.rs` | 新模板校验通过 |
| Create: `a2ui/tests/recipes_test.rs` | metrics/callout/result recipes |
| Modify: `frontend/src/a2ui/types.ts` | catalogId v2；扩展组件与 props |
| Modify: `frontend/src/a2ui/CatalogAdapter.tsx` | 扩展组件 + 表单控件真实渲染 |
| Modify: `frontend/src/a2ui/A2UIRenderer.tsx` | 表单字段 state → action.context |
| Create: `frontend/src/a2ui/formState.ts` | 纯函数：合并字段值到 context |
| Create: `frontend/src/a2ui/formState.test.ts` | node:test |
| Create: `frontend/src/a2ui/types.test.ts` | allowlist 含扩展组件 |
| Modify: `frontend/src/styles/chat.css` | 亮暗玻璃 token + 新组件样式 |
| Modify: `tools/src/builtins/present_ui.rs` | 描述文案 + 可选 recipe 快捷字段（YAGNI：仅更新描述与校验依赖） |
| Modify: `tools/src/builtins/confirm.rs` / `clarify.rs` | 仅依赖模板；若文案过旧则更新 description |
| Modify: `docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md` | catalog 小节指向 v2 spec |

---

### Task 1: Catalog v2 allowlist + validation

**Files:**
- Modify: `a2ui/src/catalog.rs`
- Modify: `a2ui/tests/validate_test.rs`

- [ ] **Step 1: Write failing tests**

Append to `a2ui/tests/validate_test.rs`:

```rust
#[test]
fn catalog_id_is_v2() {
    assert_eq!(ASTRO_CATALOG_ID, "astro://a2ui/catalog/v2");
}

#[test]
fn rejects_v1_catalog_id() {
    let ops = serde_json::json!([{
        "version": "v0.9",
        "createSurface": {
            "surfaceId": "s1",
            "catalogId": "astro://a2ui/catalog/v1"
        }
    }]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("catalog"));
}

#[test]
fn accepts_extension_components() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    {
                        "id": "col",
                        "component": "Column",
                        "children": ["badge", "metric", "callout", "avatar", "chip", "sp"]
                    },
                    { "id": "badge", "component": "Badge", "text": "Live", "variant": "success" },
                    { "id": "metric", "component": "Metric", "label": "CPU", "value": "42%", "hint": "ok" },
                    { "id": "callout", "component": "Callout", "text": "注意", "variant": "warn" },
                    { "id": "avatar", "component": "Avatar", "text": "SH" },
                    { "id": "chip", "component": "Chip", "text": "tag" },
                    { "id": "sp", "component": "Spacer", "size": "md" }
                ]
            }
        }
    ]);
    validate_operations(ops.as_array().unwrap()).unwrap();
}

#[test]
fn rejects_deferred_modal() {
    let ops = serde_json::json!([
        {
            "version": "v0.9",
            "createSurface": {
                "surfaceId": "s1",
                "catalogId": ASTRO_CATALOG_ID
            }
        },
        {
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": "s1",
                "components": [
                    { "id": "m", "component": "Modal", "child": "x" }
                ]
            }
        }
    ]);
    let err = validate_operations(ops.as_array().unwrap()).unwrap_err();
    assert!(err.to_string().contains("Modal"));
}
```

- [ ] **Step 2: Run tests — expect fail**

Run: `cargo test -p a2ui --test validate_test catalog_id_is_v2 -- --nocapture`

Expected: FAIL — catalog still `v1`

- [ ] **Step 3: Update catalog**

Replace `a2ui/src/catalog.rs` with:

```rust
pub const ASTRO_CATALOG_ID: &str = "astro://a2ui/catalog/v2";

pub const ALLOWED_COMPONENTS: &[&str] = &[
    // basic
    "Text",
    "Icon",
    "Divider",
    "Card",
    "Column",
    "Row",
    "Button",
    "TextField",
    "ChoicePicker",
    "CheckBox",
    "Image",
    "List",
    // astro extensions (v2)
    "Badge",
    "Chip",
    "Metric",
    "Avatar",
    "Callout",
    "Spacer",
];
```

- [ ] **Step 4: Run tests — expect pass**

Run: `cargo test -p a2ui --test validate_test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add a2ui/src/catalog.rs a2ui/tests/validate_test.rs
git commit -m "$(cat <<'EOF'
feat(a2ui): bump catalog to v2 with extension components

EOF
)"
```

---

### Task 2: Upgrade HITL + info templates

**Files:**
- Modify: `a2ui/src/templates.rs`
- Modify: `a2ui/tests/templates_test.rs`

- [ ] **Step 1: Write failing template assertions**

Replace/extend `a2ui/tests/templates_test.rs`:

```rust
use a2ui::templates::{build_clarify_surface, build_confirm_surface, build_info_surface};
use a2ui::{validate_operations, ASTRO_CATALOG_ID};
use serde_json::Value;

fn catalog_ids(ops: &[Value]) -> Vec<&str> {
    ops.iter()
        .filter_map(|op| {
            op.get("createSurface")
                .and_then(|c| c.get("catalogId"))
                .and_then(|v| v.as_str())
        })
        .collect()
}

fn all_component_names(ops: &[Value]) -> Vec<String> {
    let mut names = Vec::new();
    for op in ops {
        if let Some(comps) = op
            .pointer("/updateComponents/components")
            .and_then(|v| v.as_array())
        {
            for c in comps {
                if let Some(n) = c.get("component").and_then(|v| v.as_str()) {
                    names.push(n.to_string());
                }
            }
        }
    }
    names
}

#[test]
fn confirm_template_validates_and_uses_v2() {
    let ops = build_confirm_surface("surf-confirm-1", "删除文件？", "将永久删除 report.pdf");
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "Avatar" || n == "Badge"));
    assert!(names.iter().any(|n| n == "Button"));
}

#[test]
fn clarify_template_validates() {
    let ops = build_clarify_surface(
        "surf-clarify-1",
        "选哪个环境？",
        &["staging".into(), "production".into()],
    );
    validate_operations(&ops).unwrap();
    assert_eq!(catalog_ids(&ops), vec![ASTRO_CATALOG_ID]);
}

#[test]
fn info_template_validates_with_optional_image() {
    let ops = build_info_surface(
        "surf-info-1",
        "部署摘要",
        "3 服务已更新",
        Some("https://example.com/a.png"),
    );
    validate_operations(&ops).unwrap();
    let names = all_component_names(&ops);
    assert!(names.iter().any(|n| n == "Image"));
}
```

- [ ] **Step 2: Run — expect fail on Avatar/Badge assertion**

Run: `cargo test -p a2ui --test templates_test confirm_template_validates_and_uses_v2 -- --nocapture`

Expected: FAIL — confirm template lacks Avatar/Badge

- [ ] **Step 3: Upgrade `build_confirm_surface`**

In `a2ui/src/templates.rs`, change confirm components to (keep action names `approve`/`deny`):

```rust
pub fn build_confirm_surface(surface_id: &str, title: &str, body: &str) -> Vec<Value> {
    vec![
        json!({
            "version": "v0.9",
            "createSurface": {
                "surfaceId": surface_id,
                "catalogId": ASTRO_CATALOG_ID
            }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    {
                        "id": "col",
                        "component": "Column",
                        "children": ["header", "body", "actions"]
                    },
                    {
                        "id": "header",
                        "component": "Row",
                        "children": ["avatar", "header_text", "badge"]
                    },
                    {
                        "id": "avatar",
                        "component": "Avatar",
                        "name": "shield"
                    },
                    {
                        "id": "header_text",
                        "component": "Column",
                        "children": ["title"]
                    },
                    {
                        "id": "title",
                        "component": "Text",
                        "text": title,
                        "variant": "h2"
                    },
                    {
                        "id": "badge",
                        "component": "Badge",
                        "text": "Confirm",
                        "variant": "warn"
                    },
                    {
                        "id": "body",
                        "component": "Text",
                        "text": body
                    },
                    {
                        "id": "actions",
                        "component": "Row",
                        "children": ["approve", "deny"]
                    },
                    {
                        "id": "approve",
                        "component": "Button",
                        "child": "approve_label",
                        "variant": "primary",
                        "action": { "event": { "name": "approve" } }
                    },
                    {
                        "id": "approve_label",
                        "component": "Text",
                        "text": "Approve"
                    },
                    {
                        "id": "deny",
                        "component": "Button",
                        "child": "deny_label",
                        "variant": "secondary",
                        "action": { "event": { "name": "deny" } }
                    },
                    {
                        "id": "deny_label",
                        "component": "Text",
                        "text": "Deny"
                    }
                ]
            }
        }),
    ]
}
```

Upgrade `build_clarify_surface` header similarly: add optional `Badge` text `"Clarify"` variant `info` before question; keep option Buttons + `choose` events.

Upgrade `build_info_surface`: after title, insert a `Badge` `"Info"` / `info` when no image; keep image path as today.

- [ ] **Step 4: Run templates tests**

Run: `cargo test -p a2ui --test templates_test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add a2ui/src/templates.rs a2ui/tests/templates_test.rs
git commit -m "$(cat <<'EOF'
feat(a2ui): upgrade HITL and info templates for catalog v2

EOF
)"
```

---

### Task 3: Recipe templates (metrics / callout / result)

**Files:**
- Modify: `a2ui/src/templates.rs`
- Modify: `a2ui/src/lib.rs` (re-export if templates already `pub mod`)
- Create: `a2ui/tests/recipes_test.rs`

- [ ] **Step 1: Write failing recipe tests**

`a2ui/tests/recipes_test.rs`:

```rust
use a2ui::templates::{
    build_callout_surface, build_metrics_surface, build_result_surface,
};
use a2ui::validate_operations;

#[test]
fn metrics_recipe_validates() {
    let ops = build_metrics_surface(
        "surf-m1",
        "系统指标",
        &[
            ("CPU".into(), "42%".into(), Some("正常".into())),
            ("内存".into(), "1.2G".into(), None),
        ],
    );
    validate_operations(&ops).unwrap();
}

#[test]
fn callout_recipe_validates() {
    let ops = build_callout_surface("surf-c1", "注意", "即将重启", "warn");
    validate_operations(&ops).unwrap();
}

#[test]
fn result_recipe_validates() {
    let ops = build_result_surface("surf-r1", "完成", "部署成功", "success");
    validate_operations(&ops).unwrap();
}
```

- [ ] **Step 2: Run — expect compile fail**

Run: `cargo test -p a2ui --test recipes_test -- --nocapture`

Expected: FAIL — missing functions

- [ ] **Step 3: Implement recipes in `templates.rs`**

```rust
/// Metrics list inside a glass Card (title + Metric rows).
pub fn build_metrics_surface(
    surface_id: &str,
    title: &str,
    metrics: &[(String, String, Option<String>)],
) -> Vec<Value> {
    let mut col_children = vec!["title".to_string()];
    for (i, _) in metrics.iter().enumerate() {
        col_children.push(format!("m{i}"));
    }
    let mut components = vec![
        json!({ "id": "root", "component": "Card", "child": "col" }),
        json!({ "id": "col", "component": "Column", "children": col_children }),
        json!({ "id": "title", "component": "Text", "text": title, "variant": "h2" }),
    ];
    for (i, (label, value, hint)) in metrics.iter().enumerate() {
        let mut m = json!({
            "id": format!("m{i}"),
            "component": "Metric",
            "label": label,
            "value": value
        });
        if let Some(h) = hint.as_ref().map(|s| s.as_str().trim()).filter(|s| !s.is_empty()) {
            m["hint"] = json!(h);
        }
        components.push(m);
    }
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": { "surfaceId": surface_id, "components": components }
        }),
    ]
}

pub fn build_callout_surface(
    surface_id: &str,
    title: &str,
    body: &str,
    variant: &str,
) -> Vec<Value> {
    let v = match variant {
        "warn" | "danger" | "success" | "info" => variant,
        _ => "info",
    };
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["title", "callout"] },
                    { "id": "title", "component": "Text", "text": title, "variant": "h2" },
                    { "id": "callout", "component": "Callout", "text": body, "variant": v }
                ]
            }
        }),
    ]
}

pub fn build_result_surface(
    surface_id: &str,
    title: &str,
    body: &str,
    status: &str,
) -> Vec<Value> {
    let v = match status {
        "warn" | "danger" | "success" | "info" => status,
        _ => "success",
    };
    vec![
        json!({
            "version": "v0.9",
            "createSurface": { "surfaceId": surface_id, "catalogId": ASTRO_CATALOG_ID }
        }),
        json!({
            "version": "v0.9",
            "updateComponents": {
                "surfaceId": surface_id,
                "components": [
                    { "id": "root", "component": "Card", "child": "col" },
                    { "id": "col", "component": "Column", "children": ["header", "body"] },
                    { "id": "header", "component": "Row", "children": ["title", "badge"] },
                    { "id": "title", "component": "Text", "text": title, "variant": "h2" },
                    { "id": "badge", "component": "Badge", "text": v, "variant": v },
                    { "id": "body", "component": "Text", "text": body }
                ]
            }
        }),
    ]
}
```

- [ ] **Step 4: Run recipe tests**

Run: `cargo test -p a2ui --test recipes_test`

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add a2ui/src/templates.rs a2ui/tests/recipes_test.rs
git commit -m "$(cat <<'EOF'
feat(a2ui): add metrics, callout, and result recipe surfaces

EOF
)"
```

---

### Task 4: Frontend types + allowlist mirror

**Files:**
- Modify: `frontend/src/a2ui/types.ts`
- Create: `frontend/src/a2ui/types.test.ts`

- [ ] **Step 1: Write failing test**

`frontend/src/a2ui/types.test.ts`:

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { ALLOWED_COMPONENTS, ASTRO_CATALOG_ID } from "./types.ts";

test("catalog id is v2", () => {
  assert.equal(ASTRO_CATALOG_ID, "astro://a2ui/catalog/v2");
});

test("allowlist includes extension components", () => {
  for (const name of ["Badge", "Chip", "Metric", "Avatar", "Callout", "Spacer"]) {
    assert.equal(ALLOWED_COMPONENTS.has(name), true, name);
  }
});
```

- [ ] **Step 2: Run — expect fail**

Run: `cd frontend && node --experimental-strip-types --test src/a2ui/types.test.ts`

Expected: FAIL on catalog id or missing components

- [ ] **Step 3: Update `types.ts`**

```ts
export type A2uiComponent = {
  id: string;
  component: string;
  text?: string;
  variant?: string;
  child?: string;
  children?: string[];
  name?: string;
  src?: string;
  url?: string;
  label?: string;
  value?: string | boolean;
  hint?: string;
  size?: string;
  options?: Array<string | { label?: string; value?: string }>;
  action?: {
    event?: {
      name?: string;
      context?: Record<string, unknown>;
    };
  };
  [key: string]: unknown;
};

export const ASTRO_CATALOG_ID = "astro://a2ui/catalog/v2";

export const ALLOWED_COMPONENTS = new Set([
  "Text",
  "Icon",
  "Divider",
  "Card",
  "Column",
  "Row",
  "Button",
  "TextField",
  "ChoicePicker",
  "CheckBox",
  "Image",
  "List",
  "Badge",
  "Chip",
  "Metric",
  "Avatar",
  "Callout",
  "Spacer",
]);
```

Keep existing `A2uiOperation` type unchanged.

- [ ] **Step 4: Run test — pass**

- [ ] **Step 5: Commit**

```bash
git add frontend/src/a2ui/types.ts frontend/src/a2ui/types.test.ts
git commit -m "$(cat <<'EOF'
feat(frontend): mirror A2UI catalog v2 types and allowlist

EOF
)"
```

---

### Task 5: Soft Dark / Frost glass CSS

**Files:**
- Modify: `frontend/src/styles/chat.css` (a2ui section ~412+)

- [ ] **Step 1: Replace a2ui CSS block with themed tokens**

Find `.a2ui-surface` through `.a2ui-icon` and replace/extend with:

```css
.a2ui-surface {
  margin: 0 0 10px;
  --a2ui-glass-bg: color-mix(in srgb, var(--panel, var(--bg)) 72%, transparent);
  --a2ui-glass-border: color-mix(in srgb, var(--ink) 14%, transparent);
  --a2ui-glass-blur: 14px;
  --a2ui-glass-shadow: 0 8px 28px color-mix(in srgb, var(--ink) 12%, transparent);
  --a2ui-glass-fill: color-mix(in srgb, var(--ink) 5%, transparent);
  --a2ui-accent-fill: color-mix(in srgb, var(--accent, #3b82f6) 22%, transparent);
  --a2ui-accent-border: color-mix(in srgb, var(--accent, #3b82f6) 42%, transparent);
}

html[data-theme="dark"] .a2ui-surface {
  --a2ui-glass-bg: rgba(255, 255, 255, 0.08);
  --a2ui-glass-border: rgba(255, 255, 255, 0.18);
  --a2ui-glass-shadow: 0 8px 32px rgba(0, 0, 0, 0.35);
  --a2ui-glass-fill: rgba(255, 255, 255, 0.06);
}

html[data-theme="light"] .a2ui-surface {
  --a2ui-glass-bg: rgba(255, 255, 255, 0.55);
  --a2ui-glass-border: rgba(255, 255, 255, 0.72);
  --a2ui-glass-shadow: 0 10px 36px rgba(15, 23, 42, 0.1);
  --a2ui-glass-fill: rgba(15, 23, 42, 0.04);
}

.a2ui-surface.is-disabled {
  opacity: 0.72;
}

.a2ui-card {
  border: 1px solid var(--a2ui-glass-border);
  background: var(--a2ui-glass-bg);
  backdrop-filter: blur(var(--a2ui-glass-blur)) saturate(1.35);
  -webkit-backdrop-filter: blur(var(--a2ui-glass-blur)) saturate(1.35);
  box-shadow: var(--a2ui-glass-shadow);
  border-radius: 16px;
  padding: 14px 16px;
}

.a2ui-column {
  display: flex;
  flex-direction: column;
  gap: 10px;
}

.a2ui-row {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  align-items: center;
}

.a2ui-list {
  margin: 0;
  padding-left: 1.1em;
}

.a2ui-text {
  margin: 0;
  line-height: 1.45;
  color: var(--ink);
}

.a2ui-h1 {
  font-size: 1.25rem;
  font-weight: 650;
}

.a2ui-h2 {
  font-size: 1.05rem;
  font-weight: 650;
}

.a2ui-caption {
  font-size: 0.85rem;
  color: var(--ink-mute);
}

.a2ui-button {
  appearance: none;
  border: 1px solid var(--a2ui-glass-border);
  background: var(--a2ui-glass-fill);
  color: var(--ink);
  border-radius: 10px;
  padding: 7px 12px;
  cursor: pointer;
  font: inherit;
}

.a2ui-button.is-primary {
  background: var(--a2ui-accent-fill);
  border-color: var(--a2ui-accent-border);
}

.a2ui-button:disabled {
  cursor: not-allowed;
  opacity: 0.55;
}

.a2ui-image {
  max-width: 100%;
  border-radius: 10px;
  display: block;
}

.a2ui-divider {
  border: 0;
  border-top: 1px solid var(--a2ui-glass-border);
  margin: 4px 0;
}

.a2ui-unknown,
.a2ui-field-placeholder {
  font-size: 0.85rem;
  color: var(--ink-mute);
  padding: 6px 8px;
  border-radius: 8px;
  border: 1px dashed var(--a2ui-glass-border);
}

.a2ui-icon {
  opacity: 0.7;
}

.a2ui-badge,
.a2ui-chip {
  display: inline-flex;
  align-items: center;
  font-size: 0.75rem;
  font-weight: 600;
  padding: 3px 10px;
  border-radius: 999px;
  border: 1px solid var(--a2ui-glass-border);
  background: var(--a2ui-glass-fill);
  color: var(--ink);
}

.a2ui-badge.is-success {
  border-color: color-mix(in srgb, #34d399 45%, transparent);
  background: color-mix(in srgb, #34d399 18%, transparent);
}
.a2ui-badge.is-warn {
  border-color: color-mix(in srgb, #fbbf24 45%, transparent);
  background: color-mix(in srgb, #fbbf24 18%, transparent);
}
.a2ui-badge.is-danger {
  border-color: color-mix(in srgb, #f87171 45%, transparent);
  background: color-mix(in srgb, #f87171 18%, transparent);
}
.a2ui-badge.is-info {
  border-color: var(--a2ui-accent-border);
  background: var(--a2ui-accent-fill);
}

.a2ui-metric {
  background: var(--a2ui-glass-fill);
  border: 1px solid var(--a2ui-glass-border);
  border-radius: 12px;
  padding: 10px 12px;
  min-width: 88px;
  text-align: center;
}
.a2ui-metric-label {
  font-size: 0.75rem;
  color: var(--ink-mute);
}
.a2ui-metric-value {
  font-size: 1.15rem;
  font-weight: 650;
  color: var(--ink);
}
.a2ui-metric-hint {
  font-size: 0.72rem;
  color: var(--ink-mute);
}

.a2ui-avatar {
  width: 40px;
  height: 40px;
  border-radius: 12px;
  display: inline-flex;
  align-items: center;
  justify-content: center;
  background: var(--a2ui-glass-fill);
  border: 1px solid var(--a2ui-glass-border);
  font-size: 0.85rem;
  font-weight: 650;
  overflow: hidden;
}
.a2ui-avatar img {
  width: 100%;
  height: 100%;
  object-fit: cover;
}

.a2ui-callout {
  border-radius: 12px;
  padding: 10px 12px;
  font-size: 0.9rem;
  border: 1px solid var(--a2ui-accent-border);
  background: var(--a2ui-accent-fill);
  color: var(--ink);
}
.a2ui-callout.is-warn {
  border-color: color-mix(in srgb, #fbbf24 45%, transparent);
  background: color-mix(in srgb, #fbbf24 16%, transparent);
}

.a2ui-spacer-sm { height: 6px; }
.a2ui-spacer-md { height: 12px; }
.a2ui-spacer-lg { height: 20px; }

.a2ui-field,
.a2ui-choice,
.a2ui-check {
  display: flex;
  flex-direction: column;
  gap: 6px;
  width: 100%;
}
.a2ui-field input,
.a2ui-choice select {
  appearance: none;
  border: 1px solid var(--a2ui-glass-border);
  background: var(--a2ui-glass-fill);
  color: var(--ink);
  border-radius: 10px;
  padding: 8px 10px;
  font: inherit;
}
.a2ui-check {
  flex-direction: row;
  align-items: center;
  gap: 8px;
}
```

- [ ] **Step 2: Manual smoke**

Run app (`npm run tauri dev` if already running), toggle light/dark, confirm existing HITL card picks up glass. No automated CSS test required.

- [ ] **Step 3: Commit**

```bash
git add frontend/src/styles/chat.css
git commit -m "$(cat <<'EOF'
style(a2ui): add Soft Dark and Frost glass tokens

EOF
)"
```

---

### Task 6: CatalogAdapter — extension components

**Files:**
- Modify: `frontend/src/a2ui/CatalogAdapter.tsx`

- [ ] **Step 1: Extend `RenderCtx` and add cases**

Update `RenderCtx`:

```ts
type RenderCtx = {
  byId: Map<string, A2uiComponent>;
  disabled: boolean;
  onAction: (name: string, context: Record<string, unknown>) => void;
  unknownLabel: string;
  fieldValues: Record<string, unknown>;
  setFieldValue: (id: string, value: unknown) => void;
};
```

Add switch cases **before** `default` (keep Button as-is for now):

```tsx
    case "Badge": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = typeof node.variant === "string" ? node.variant : "info";
      return (
        <span className={`a2ui-badge is-${variant}`}>{text}</span>
      );
    }
    case "Chip": {
      const text = typeof node.text === "string" ? node.text : "";
      const eventName = node.action?.event?.name;
      if (eventName) {
        return (
          <button
            type="button"
            className="a2ui-chip"
            disabled={ctx.disabled}
            onClick={() =>
              ctx.onAction(eventName, node.action?.event?.context ?? {})
            }
          >
            {text}
          </button>
        );
      }
      return <span className="a2ui-chip">{text}</span>;
    }
    case "Metric": {
      const label = typeof node.label === "string" ? node.label : "";
      const value = node.value != null ? String(node.value) : "";
      const hint = typeof node.hint === "string" ? node.hint : "";
      return (
        <div className="a2ui-metric">
          <div className="a2ui-metric-label">{label}</div>
          <div className="a2ui-metric-value">{value}</div>
          {hint ? <div className="a2ui-metric-hint">{hint}</div> : null}
        </div>
      );
    }
    case "Avatar": {
      const src =
        (typeof node.src === "string" && node.src) ||
        (typeof node.url === "string" && node.url) ||
        "";
      const text = typeof node.text === "string" ? node.text : "";
      const name = typeof node.name === "string" ? node.name : "";
      return (
        <div className="a2ui-avatar" aria-hidden>
          {src ? <img src={src} alt="" /> : text || name || "•"}
        </div>
      );
    }
    case "Callout": {
      const text = typeof node.text === "string" ? node.text : "";
      const variant = node.variant === "warn" ? "warn" : "info";
      return (
        <div className={`a2ui-callout is-${variant}`}>{text}</div>
      );
    }
    case "Spacer": {
      const size =
        node.size === "sm" || node.size === "lg" ? node.size : "md";
      return <div className={`a2ui-spacer-${size}`} />;
    }
```

Update `renderCatalogTree` signature later in Task 7 to pass field state; for this task, temporarily pass:

```ts
fieldValues: {},
setFieldValue: () => {},
```

so TypeScript compiles.

- [ ] **Step 2: Typecheck**

Run: `cd frontend && npx tsc -b --pretty false`

Expected: no errors related to CatalogAdapter

- [ ] **Step 3: Commit**

```bash
git add frontend/src/a2ui/CatalogAdapter.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): render A2UI v2 extension components

EOF
)"
```

---

### Task 7: Form controls + field context merge

**Files:**
- Create: `frontend/src/a2ui/formState.ts`
- Create: `frontend/src/a2ui/formState.test.ts`
- Modify: `frontend/src/a2ui/CatalogAdapter.tsx`
- Modify: `frontend/src/a2ui/A2UIRenderer.tsx`

- [ ] **Step 1: Failing tests for merge helper**

`frontend/src/a2ui/formState.ts` (create empty stub first if preferred TDD):

```ts
export function mergeActionContext(
  base: Record<string, unknown> | undefined,
  fieldValues: Record<string, unknown>,
): Record<string, unknown> {
  return { ...fieldValues, ...(base ?? {}) };
}
```

`frontend/src/a2ui/formState.test.ts`:

```ts
import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeActionContext } from "./formState.ts";

test("event context overrides field defaults", () => {
  const merged = mergeActionContext(
    { value: "from-button" },
    { value: "from-field", other: 1 },
  );
  assert.deepEqual(merged, { value: "from-button", other: 1 });
});

test("empty base keeps fields", () => {
  assert.deepEqual(mergeActionContext(undefined, { a: true }), { a: true });
});
```

- [ ] **Step 2: Run formState tests — pass**

- [ ] **Step 3: Wire real form controls in CatalogAdapter**

Replace TextField / ChoicePicker / CheckBox placeholders:

```tsx
    case "TextField": {
      const label = typeof node.label === "string" ? node.label : typeof node.text === "string" ? node.text : "";
      const current =
        ctx.fieldValues[node.id] != null
          ? String(ctx.fieldValues[node.id])
          : typeof node.value === "string"
            ? node.value
            : "";
      return (
        <label className="a2ui-field">
          {label ? <span className="a2ui-caption">{label}</span> : null}
          <input
            type="text"
            disabled={ctx.disabled}
            value={current}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.value)}
          />
        </label>
      );
    }
    case "ChoicePicker": {
      const label = typeof node.label === "string" ? node.label : "";
      const options = Array.isArray(node.options) ? node.options : [];
      const current =
        ctx.fieldValues[node.id] != null
          ? String(ctx.fieldValues[node.id])
          : typeof node.value === "string"
            ? node.value
            : "";
      return (
        <label className="a2ui-choice">
          {label ? <span className="a2ui-caption">{label}</span> : null}
          <select
            disabled={ctx.disabled}
            value={current}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.value)}
          >
            <option value="">—</option>
            {options.map((opt, i) => {
              if (typeof opt === "string") {
                return (
                  <option key={i} value={opt}>
                    {opt}
                  </option>
                );
              }
              const v = opt.value ?? opt.label ?? "";
              return (
                <option key={i} value={v}>
                  {opt.label ?? v}
                </option>
              );
            })}
          </select>
        </label>
      );
    }
    case "CheckBox": {
      const label = typeof node.label === "string" ? node.label : typeof node.text === "string" ? node.text : "";
      const checked =
        typeof ctx.fieldValues[node.id] === "boolean"
          ? Boolean(ctx.fieldValues[node.id])
          : Boolean(node.value);
      return (
        <label className="a2ui-check">
          <input
            type="checkbox"
            disabled={ctx.disabled}
            checked={checked}
            onChange={(e) => ctx.setFieldValue(node.id, e.target.checked)}
          />
          <span>{label}</span>
        </label>
      );
    }
```

Update Button click:

```tsx
onClick={() =>
  ctx.onAction(
    eventName,
    mergeActionContext(context, ctx.fieldValues),
  )
}
```

Import `mergeActionContext` from `./formState`.

- [ ] **Step 4: Hold field state in `A2UIRenderer`**

```tsx
import { useState } from "react";
// ...
const [fieldValues, setFieldValues] = useState<Record<string, unknown>>({});
// in renderCatalogTree opts:
fieldValues,
setFieldValue: (id, value) =>
  setFieldValues((prev) => ({ ...prev, [id]: value })),
```

Extend `renderCatalogTree` opts type accordingly.

- [ ] **Step 5: Typecheck + formState tests**

Run:

```bash
cd frontend && npx tsc -b --pretty false
cd frontend && node --experimental-strip-types --test src/a2ui/formState.test.ts
```

Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add frontend/src/a2ui/formState.ts frontend/src/a2ui/formState.test.ts \
  frontend/src/a2ui/CatalogAdapter.tsx frontend/src/a2ui/A2UIRenderer.tsx
git commit -m "$(cat <<'EOF'
feat(frontend): enable interactive A2UI form fields

EOF
)"
```

---

### Task 8: Tool descriptions for AI composition

**Files:**
- Modify: `tools/src/builtins/present_ui.rs`
- Modify: `tools/src/builtins/confirm.rs` (description only if needed)
- Modify: `tools/src/builtins/clarify.rs` (description only if needed)

- [ ] **Step 1: Expand `present_ui` description**

Set description roughly to:

```text
Present a read-only informational UI card in chat (no interrupt). Prefer shortcut fields title/body/image_url, or pass full A2UI v0.9 operations[] with catalogId astro://a2ui/catalog/v2. Allowed components: Text Icon Divider Card Column Row Button TextField ChoicePicker CheckBox Image List Badge Chip Metric Avatar Callout Spacer. Root should be Card. Use variant for semantics; never put hex colors in JSON. Example metric row: Metric{label,value,hint} inside Column inside Card.
```

- [ ] **Step 2: Ensure tools still compile**

Run: `cargo test -p tools -- --nocapture` (or at least `cargo check -p tools`)

Templates already use v2 via `ASTRO_CATALOG_ID`; confirm/clarify need no logic change unless their tests hardcode v1.

- [ ] **Step 3: Commit**

```bash
git add tools/src/builtins/present_ui.rs tools/src/builtins/confirm.rs tools/src/builtins/clarify.rs
git commit -m "$(cat <<'EOF'
docs(tools): guide models toward A2UI catalog v2 composition

EOF
)"
```

---

### Task 9: Cross-link GenUI spec + final verification

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md`

- [ ] **Step 1: Update catalog subsection**

In section 「Catalog subset」, change:

- `catalogId` to `astro://a2ui/catalog/v2`
- Note extension components + link to `2026-07-13-a2ui-glass-catalog-v2-design.md`
- Keep Modal/Tabs/... as out of MVP but 「见 glass catalog v2 forward-compat」

- [ ] **Step 2: Full verification**

```bash
cargo test -p a2ui
cd frontend && npx tsc -b --pretty false
cd frontend && node --experimental-strip-types --test src/a2ui/*.test.ts
```

Expected: all green

- [ ] **Step 3: Manual checklist**

- [ ] Dark theme: confirm card glass readable  
- [ ] Light theme: same card Frost readable  
- [ ] present_ui with free operations including Metric/Badge renders  
- [ ] TextField + Button submit includes field id in action context  

- [ ] **Step 4: Commit**

```bash
git add docs/superpowers/specs/2026-07-13-declarative-genui-a2ui-design.md
git commit -m "$(cat <<'EOF'
docs: point GenUI catalog section at A2UI glass v2

EOF
)"
```

---

## Spec coverage self-check

| Spec requirement | Task |
|------------------|------|
| Catalog v2 ID + allowlist | Task 1 |
| Extension components Badge…Spacer | Task 1, 6 |
| Soft Dark + Frost theme tokens | Task 5 |
| HITL templates upgraded | Task 2 |
| Info + recipes | Task 2, 3 |
| Hybrid present_ui / AI guidance | Task 8 |
| Forms interactive + context merge | Task 7 |
| Reject Modal etc., easy later add | Task 1 (reject) + Task 6 registry-style switch |
| Errors / tests | Tasks 1–4, 7, 9 |
| Cross-link GenUI doc | Task 9 |

## Placeholder / consistency notes

- `mergeActionContext`: event context **wins** over field values (explicit button `value` not overwritten).
- Metric props: `label` / `value` / `hint` — same names in Rust recipes and TS types.
- Do not implement Modal/Tabs/Slider/Video/AudioPlayer in this plan.
