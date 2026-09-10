import test from "node:test";
import assert from "node:assert/strict";
import {
  interfaceTourCopy,
  shouldOfferInterfaceTour,
} from "./interfaceTour.ts";

test("only a valid unresolved version offers the tour", () => {
  assert.equal(shouldOfferInterfaceTour({ resolved_version: 0 }), true);
  for (const version of [1, 2, 99, -1, NaN, 0.5]) {
    assert.equal(
      shouldOfferInterfaceTour({ resolved_version: version }),
      false,
    );
  }
});

test("both locales cover the same five stable targets", () => {
  const expected = ["composer", "model", "sidebar", "plugins", "settings"];
  for (const copy of Object.values(interfaceTourCopy)) {
    assert.deepEqual(
      copy.steps.map(([id]) => id),
      expected,
    );
    assert.ok(
      copy.begin && copy.dismiss && copy.replayAction && copy.saveError,
    );
    assert.ok(
      copy.steps.every(([, title, description]) => title && description),
    );
  }
});
