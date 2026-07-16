# IP City Suggestion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在位置 HITL 卡片中通过公网 IP 推测城市，预填但不自动提交。

**Architecture:** Tauri Rust 命令负责调用 `ipwho.is`、约束超时并校验响应；React 位置 surface 包装组件只在 `location_required` 时调用命令。`A2UIRenderer` 接收可选初始字段，并以“不覆盖用户输入”的规则合并异步城市建议。

**Tech Stack:** Rust 2021、Tauri 2、reqwest 0.12、serde、React 18、TypeScript 5、Node test runner

## Global Constraints

- 不申请 macOS Core Location 权限。
- 不自动提交推测结果；用户必须确认。
- 不读取、记录或持久化 IP。
- 不增加 API Key。
- IP 服务失败必须静默降级为手动输入。
- 只在活动的 `location_required` surface 上发起一次请求。

---

### Task 1: Tauri IP 城市推测命令

**Files:**
- Create: `frontend/src-tauri/src/ip_location.rs`
- Modify: `frontend/src-tauri/src/lib.rs:3-20,240-350`

**Interfaces:**
- Produces: `#[tauri::command] async fn infer_ip_location() -> Result<IpLocationDto, String>`
- Produces DTO serialized as `{ city: string, region: string | null, country: string | null }`

- [ ] **Step 1: 写响应解析失败测试**

在 `ip_location.rs` 中先定义测试模块，覆盖成功、服务拒绝和空城市：

```rust
#[cfg(test)]
mod tests {
    use super::parse_ipwho_response;

    #[test]
    fn parses_valid_city() {
        let dto = parse_ipwho_response(
            r#"{"success":true,"city":"Hangzhou","region":"Zhejiang","country":"China"}"#,
        ).unwrap();
        assert_eq!(dto.city, "Hangzhou");
        assert_eq!(dto.region.as_deref(), Some("Zhejiang"));
        assert_eq!(dto.country.as_deref(), Some("China"));
    }

    #[test]
    fn rejects_failed_response() {
        let err = parse_ipwho_response(r#"{"success":false,"message":"rate limited"}"#)
            .unwrap_err();
        assert_eq!(err, "IP location service rejected the request");
    }

    #[test]
    fn rejects_blank_city() {
        let err = parse_ipwho_response(r#"{"success":true,"city":"  "}"#).unwrap_err();
        assert_eq!(err, "IP location response did not include a city");
    }

    #[test]
    fn rejects_invalid_json() {
        let err = parse_ipwho_response("not json").unwrap_err();
        assert_eq!(err, "Invalid IP location response");
    }
}
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cd frontend/src-tauri && cargo test ip_location
```

Expected: FAIL，`ip_location` 模块或 `parse_ipwho_response` 尚未定义。

- [ ] **Step 3: 实现 DTO、解析与 HTTP 命令**

在 `ip_location.rs` 中实现：

```rust
use std::time::Duration;

use serde::{Deserialize, Serialize};

const IP_LOCATION_URL: &str = "https://ipwho.is/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IpLocationDto {
    pub city: String,
    pub region: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IpWhoResponse {
    success: bool,
    city: Option<String>,
    region: Option<String>,
    country: Option<String>,
}

fn non_blank(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

fn parse_ipwho_response(body: &str) -> Result<IpLocationDto, String> {
    let response: IpWhoResponse =
        serde_json::from_str(body).map_err(|_| "Invalid IP location response".to_owned())?;
    if !response.success {
        return Err("IP location service rejected the request".to_owned());
    }
    let city = non_blank(response.city)
        .ok_or_else(|| "IP location response did not include a city".to_owned())?;
    Ok(IpLocationDto {
        city,
        region: non_blank(response.region),
        country: non_blank(response.country),
    })
}

#[tauri::command]
pub async fn infer_ip_location() -> Result<IpLocationDto, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(5))
        .user_agent("Astro-Agent/0.1")
        .build()
        .map_err(|_| "Could not initialize IP location request".to_owned())?;
    let response = client
        .get(IP_LOCATION_URL)
        .send()
        .await
        .map_err(|_| "Could not reach IP location service".to_owned())?
        .error_for_status()
        .map_err(|_| "IP location service returned an error".to_owned())?;
    let body = response
        .text()
        .await
        .map_err(|_| "Could not read IP location response".to_owned())?;
    parse_ipwho_response(&body)
}
```

在 `lib.rs` 增加 `mod ip_location;`，并在 `generate_handler!` 注册：

```rust
ip_location::infer_ip_location,
```

- [ ] **Step 4: 运行 Rust 测试**

Run:

```bash
cd frontend/src-tauri && cargo test ip_location
```

Expected: 4 tests PASS。

- [ ] **Step 5: 提交后端切片**

```bash
git add frontend/src-tauri/src/ip_location.rs frontend/src-tauri/src/lib.rs
git commit -m "feat(location): add IP city inference command"
```

---

### Task 2: A2UI 安全异步预填

**Files:**
- Create: `frontend/src/a2ui/initialFieldValues.ts`
- Create: `frontend/src/a2ui/initialFieldValues.test.ts`
- Modify: `frontend/src/a2ui/A2UIRenderer.tsx:5-55`

**Interfaces:**
- Produces: `mergeInitialFieldValues(current, initial): Record<string, unknown>`
- Extends: `A2UIRenderer` prop `initialFieldValues?: Record<string, unknown>`

- [ ] **Step 1: 写“不覆盖用户输入”测试**

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { mergeInitialFieldValues } from "./initialFieldValues.ts";

test("fills an empty field from initial values", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ city: "" }, { city: "Hangzhou" }),
    { city: "Hangzhou" },
  );
});

test("does not overwrite user input", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ city: "Shanghai" }, { city: "Hangzhou" }),
    { city: "Shanghai" },
  );
});

test("keeps unrelated fields", () => {
  assert.deepEqual(
    mergeInitialFieldValues({ note: "x" }, { city: "Hangzhou" }),
    { note: "x", city: "Hangzhou" },
  );
});
```

- [ ] **Step 2: 运行测试确认失败**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/a2ui/initialFieldValues.test.ts
```

Expected: FAIL，模块不存在。

- [ ] **Step 3: 实现纯合并函数**

```typescript
export function mergeInitialFieldValues(
  current: Record<string, unknown>,
  initial: Record<string, unknown>,
): Record<string, unknown> {
  const next = { ...current };
  for (const [key, value] of Object.entries(initial)) {
    const existing = next[key];
    const empty =
      existing == null || (typeof existing === "string" && !existing.trim());
    if (empty) next[key] = value;
  }
  return next;
}
```

- [ ] **Step 4: 让渲染器接收异步初值**

给 `A2UIRenderer` 增加 prop：

```typescript
initialFieldValues?: Record<string, unknown>;
```

在 surface key 重置 effect 之后增加：

```typescript
useEffect(() => {
  if (!initialFieldValues || Object.keys(initialFieldValues).length === 0) return;
  setFieldValues((current) =>
    mergeInitialFieldValues(current, initialFieldValues),
  );
}, [initialFieldValues]);
```

保持 key 变化时清空旧 surface 字段；异步城市到达时只填空字段。

- [ ] **Step 5: 运行单测与构建**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/a2ui/initialFieldValues.test.ts
cd frontend && npm run build
```

Expected: 3 tests PASS；TypeScript/Vite build PASS。

- [ ] **Step 6: 提交 A2UI 切片**

```bash
git add frontend/src/a2ui/initialFieldValues.ts frontend/src/a2ui/initialFieldValues.test.ts frontend/src/a2ui/A2UIRenderer.tsx
git commit -m "feat(a2ui): support safe async field defaults"
```

---

### Task 3: 位置 surface 自动推测并预填城市

**Files:**
- Create: `frontend/src/hooks/ui/useIpCitySuggestion.ts`
- Create: `frontend/src/components/chat/LocationA2UISurface.tsx`
- Create: `frontend/src/lib/chat/locationSurface.ts`
- Create: `frontend/src/lib/chat/locationSurface.test.ts`
- Modify: `frontend/src/components/chat/ChatView.tsx:99,1360-1380`
- Modify: `frontend/src/i18n/messages.ts:253-260,1434-1442`

**Interfaces:**
- Consumes: Tauri command `infer_ip_location`
- Consumes: `A2UIRenderer.initialFieldValues`
- Produces: `isLocationRequiredSurface(surface: UiSurface): boolean`
- Produces: `LocationA2UISurface` with the same `operations`, `disabled`, and `onAction` contract as `A2UIRenderer`

- [ ] **Step 1: 写 surface 判定测试**

```typescript
import { test } from "node:test";
import assert from "node:assert/strict";
import { isLocationRequiredSurface } from "./locationSurface.ts";
import type { UiSurface } from "../../types.ts";

const base: UiSurface = {
  messageId: "m1",
  activityType: "a2ui-surface",
  operations: [],
  status: "active",
};

test("detects an active location interrupt", () => {
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      interrupts: [{ id: "i1", reason: "location_required" }],
    }),
    true,
  );
});

test("rejects resolved and unrelated surfaces", () => {
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      status: "resolved",
      interrupts: [{ id: "i1", reason: "location_required" }],
    }),
    false,
  );
  assert.equal(
    isLocationRequiredSurface({
      ...base,
      interrupts: [{ id: "i2", reason: "confirmation_required" }],
    }),
    false,
  );
});
```

- [ ] **Step 2: 运行判定测试确认失败**

Run:

```bash
cd frontend && node --experimental-strip-types --test src/lib/chat/locationSurface.test.ts
```

Expected: FAIL，模块不存在。

- [ ] **Step 3: 实现判定函数**

```typescript
import type { UiSurface } from "../../types";

export function isLocationRequiredSurface(surface: UiSurface): boolean {
  return (
    surface.status === "active" &&
    Boolean(surface.interrupts?.some((item) => item.reason === "location_required"))
  );
}
```

- [ ] **Step 4: 实现一次性 IP 城市 hook**

`useIpCitySuggestion.ts` 定义：

```typescript
import { invoke } from "@tauri-apps/api/core";
import { useEffect, useState } from "react";

type IpLocation = { city: string; region?: string | null; country?: string | null };
export type IpCitySuggestionState =
  | { status: "loading"; city: null }
  | { status: "success"; city: string }
  | { status: "failed"; city: null };

export function useIpCitySuggestion(enabled: boolean): IpCitySuggestionState {
  const [state, setState] = useState<IpCitySuggestionState>(
    enabled ? { status: "loading", city: null } : { status: "failed", city: null },
  );
  useEffect(() => {
    if (!enabled) return;
    let cancelled = false;
    setState({ status: "loading", city: null });
    void invoke<IpLocation>("infer_ip_location")
      .then((result) => {
        if (!cancelled && result.city.trim()) {
          setState({ status: "success", city: result.city.trim() });
        } else if (!cancelled) {
          setState({ status: "failed", city: null });
        }
      })
      .catch(() => {
        if (!cancelled) setState({ status: "failed", city: null });
      });
    return () => {
      cancelled = true;
    };
  }, [enabled]);
  return state;
}
```

确保成功响应若意外为空也进入 `failed`，不能永久停留在 `loading`。

- [ ] **Step 5: 实现位置包装组件和状态文案**

`LocationA2UISurface.tsx` 调用 hook，并渲染：

```tsx
const suggestion = useIpCitySuggestion(!disabled);
const initialFieldValues = useMemo(
  () => (suggestion.city ? { city: suggestion.city } : undefined),
  [suggestion.city],
);

return (
  <div className="location-a2ui-surface">
    <p className="a2ui-caption">
      {suggestion.status === "loading"
        ? t("chat.location.ipLoading")
        : suggestion.status === "success"
          ? t("chat.location.ipSuggested")
          : t("chat.location.ipFailed")}
    </p>
    <A2UIRenderer
      operations={operations}
      disabled={disabled}
      initialFieldValues={initialFieldValues}
      onAction={onAction}
    />
  </div>
);
```

增加中英文键：

```typescript
"chat.location.ipLoading": "正在根据 IP 推测城市…",
"chat.location.ipSuggested": "已根据 IP 推测城市，请确认或修改",
"chat.location.ipFailed": "无法自动推测城市，请手动填写",
```

英文对应：

```typescript
"chat.location.ipLoading": "Detecting your city from your IP…",
"chat.location.ipSuggested": "City suggested from your IP — confirm or edit it",
"chat.location.ipFailed": "Could not detect your city — enter it manually",
```

- [ ] **Step 6: 在 ChatView 仅替换位置 HITL 渲染**

`pushSurface` 中：

```tsx
isLocationRequiredSurface(surface) ? (
  <LocationA2UISurface
    operations={surface.operations}
    disabled={surface.status !== "active"}
    onAction={(name, context) => onUiAction?.(m.id, name, context)}
  />
) : surface.interrupts && surface.interrupts.length > 0 ? (
  <A2UIRenderer
    operations={surface.operations}
    disabled={surface.status !== "active"}
    onAction={(name, context) => onUiAction?.(m.id, name, context)}
  />
) : (
  <A2UISurfaceCard
    surface={surface}
    onAction={(name, context) => onUiAction?.(m.id, name, context)}
  />
)
```

- [ ] **Step 7: 运行前端测试和构建**

Run:

```bash
cd frontend && node --experimental-strip-types --test \
  src/a2ui/initialFieldValues.test.ts \
  src/lib/chat/locationSurface.test.ts
cd frontend && npm run build
```

Expected: 5 tests PASS；build PASS。

- [ ] **Step 8: 提交前端功能切片**

```bash
git add \
  frontend/src/hooks/ui/useIpCitySuggestion.ts \
  frontend/src/components/chat/LocationA2UISurface.tsx \
  frontend/src/lib/chat/locationSurface.ts \
  frontend/src/lib/chat/locationSurface.test.ts \
  frontend/src/components/chat/ChatView.tsx \
  frontend/src/i18n/messages.ts
git commit -m "feat(location): prefill city from IP"
```

---

### Task 4: 全量验证

**Files:**
- No source changes expected

**Interfaces:**
- Verifies Tasks 1-3 together.

- [ ] **Step 1: 运行格式检查**

```bash
cd frontend/src-tauri && cargo fmt --check
```

Expected: PASS。若失败，运行 `cargo fmt`，只提交本任务相关 Rust 文件的格式变化。

- [ ] **Step 2: 运行 Rust 测试**

```bash
cd frontend/src-tauri && cargo test
```

Expected: PASS。

- [ ] **Step 3: 运行前端测试与构建**

```bash
cd frontend && node --experimental-strip-types --test \
  src/a2ui/initialFieldValues.test.ts \
  src/lib/chat/locationSurface.test.ts
cd frontend && npm run build
```

Expected: tests 与 build 均 PASS。

- [ ] **Step 4: 手工验收**

启动：

```bash
cd frontend && npm run tauri dev
```

触发需要天气位置的请求，确认：

1. 位置卡显示 IP 推测状态。
2. 城市输入框被预填。
3. 在返回前手工输入不会被覆盖。
4. 用户点击确认后才提交。
5. 断网时仍可手工输入城市。

- [ ] **Step 5: 检查仓库状态**

```bash
git status --short
```

Expected: 无本功能未提交文件；不触碰无关工作。
