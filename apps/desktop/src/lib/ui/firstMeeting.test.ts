import assert from "node:assert/strict";
import { test } from "node:test";
import {
  canAutoStartFirstMeeting,
  firstMeetingPrompt,
} from "./firstMeeting.ts";
import { peelLeadingSkillSlashes } from "../chat/composerResolve.ts";

const eligible = {
  meeting: { status: "pending" as const, session_id: null },
  ready: true,
  providerReady: true,
  empty: true,
  input: "",
  busy: false,
};

test("first meeting starts only after handoff on an empty, ready chat", () => {
  assert.equal(canAutoStartFirstMeeting(eligible), true);
  for (const blocked of [
    { ready: false },
    { providerReady: false },
    { empty: false },
    { input: "my task draft" },
    { busy: true },
    { meeting: null },
  ]) {
    assert.equal(canAutoStartFirstMeeting({ ...eligible, ...blocked }), false);
  }
});

test("deferred and previously claimed meetings never auto-submit again", () => {
  for (const status of ["deferred", "started"] as const) {
    assert.equal(
      canAutoStartFirstMeeting({
        ...eligible,
        meeting: { status, session_id: null },
      }),
      false,
    );
  }
});

test("both locales invoke the bundled skill through the real composer parser", () => {
  for (const locale of ["zh", "en"]) {
    const parsed = peelLeadingSkillSlashes(firstMeetingPrompt(locale), [
      "first-meeting",
    ]);
    assert.deepEqual(parsed.skills, ["first-meeting"]);
    assert.ok(parsed.rest.length > 40);
  }
});
