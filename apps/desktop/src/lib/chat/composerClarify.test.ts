import assert from "node:assert/strict";
import { test } from "node:test";
import type { ChatMessage, PendingInterrupt, UiSurface } from "../../types.ts";
import {
  findComposerClarifySurface,
  isClarifySurface,
} from "./composerClarify.ts";

function surface(
  messageId: string,
  component: string,
  status: UiSurface["status"] = "active",
): UiSurface {
  return {
    messageId,
    activityType: "a2ui-surface",
    status,
    operations: [
      {
        updateComponents: {
          surfaceId: messageId,
          components: [{ id: "content", component }],
        },
      },
    ],
  };
}

test("detects ClarifyWizard surfaces without relying on activity labels", () => {
  assert.equal(isClarifySurface(surface("s1", "ClarifyWizard")), true);
  assert.equal(isClarifySurface(surface("s2", "Text")), false);
});

test("selects the active clarify surface linked to the pending interrupt", () => {
  const older = surface("surface-old", "ClarifyWizard");
  const current = {
    ...surface("surface-current", "ClarifyWizard"),
    interrupts: [{ id: "interrupt-current", reason: "input_required" }],
  };
  const messages: ChatMessage[] = [
    {
      id: "assistant-old",
      role: "assistant",
      content: "",
      uiSurfaces: [older],
    },
    {
      id: "assistant-current",
      role: "assistant",
      content: "",
      uiSurfaces: [current],
    },
  ];
  const pending: PendingInterrupt[] = [
    {
      id: "interrupt-current",
      reason: "input_required",
      assistantMessageId: "assistant-current",
    },
  ];

  assert.deepEqual(findComposerClarifySurface(messages, pending), {
    messageId: "assistant-current",
    surface: current,
  });
});

test("does not move resolved clarify surfaces or non-clarify HITL into composer", () => {
  const messages: ChatMessage[] = [
    {
      id: "assistant",
      role: "assistant",
      content: "",
      uiSurfaces: [
        surface("resolved", "ClarifyWizard", "resolved"),
        surface("location", "Text"),
      ],
    },
  ];

  assert.equal(
    findComposerClarifySurface(messages, [
      { id: "location-interrupt", reason: "location_required" },
    ]),
    null,
  );
});

test("does not use a stale clarify surface when an explicit interrupt targets another message", () => {
  const messages: ChatMessage[] = [
    {
      id: "assistant-old",
      role: "assistant",
      content: "",
      uiSurfaces: [surface("clarify-old", "ClarifyWizard")],
    },
  ];

  assert.equal(
    findComposerClarifySurface(messages, [
      {
        id: "interrupt-current",
        reason: "input_required",
        assistantMessageId: "assistant-current",
      },
    ]),
    null,
  );
});
