import assert from "node:assert/strict";
import test from "node:test";

import { canStartSideChat } from "./useSideChatSession.ts";

test("side chat starts only from an idle non-empty host session", () => {
  assert.equal(
    canStartSideChat({
      hostSessionId: "host-1",
      messageCount: 2,
      disabled: false,
      currentSessionId: null,
    }),
    true,
  );
  for (const blocked of [
    {
      hostSessionId: null,
      messageCount: 2,
      disabled: false,
      currentSessionId: null,
    },
    {
      hostSessionId: "host-1",
      messageCount: 0,
      disabled: false,
      currentSessionId: null,
    },
    {
      hostSessionId: "host-1",
      messageCount: 2,
      disabled: true,
      currentSessionId: null,
    },
    {
      hostSessionId: "host-1",
      messageCount: 2,
      disabled: false,
      currentSessionId: "side-1",
    },
  ]) {
    assert.equal(canStartSideChat(blocked), false);
  }
});
