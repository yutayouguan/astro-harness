import test from "node:test";
import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { inspectPetApng, apngFrameAt } from "./petApng.ts";
import { availablePetActions } from "./petActionCatalog.ts";

test("all 26 APNG actions match their manifests", () => {
  let count = 0;
  for (const pet of ["naitang", "pudding"]) {
    const root = new URL(`../../assets/pets/${pet}/apng/`, import.meta.url);
    const manifest = JSON.parse(
      readFileSync(new URL("pet.json", root), "utf8"),
    );
    for (const file of readdirSync(root).filter((name) =>
      name.endsWith(".apng"),
    )) {
      const bytes = readFileSync(new URL(file, root));
      const info = inspectPetApng(
        bytes.buffer.slice(
          bytes.byteOffset,
          bytes.byteOffset + bytes.byteLength,
        ),
      );
      assert.equal(
        info.frames,
        manifest.motionClips[file.slice(0, -5)].durationsMs.length,
      );
      count++;
    }
    assert.deepEqual(
      availablePetActions(manifest).sort(),
      pet === "naitang"
        ? ["grooming", "kneading"]
        : ["head-tilt", "nap", "stretch", "tail-wag"],
    );
  }
  assert.equal(count, 26);
});

test("APNG validation rejects truncation, oversize and missing animation control", () => {
  const source = readFileSync(
    new URL("../../assets/pets/naitang/apng/idle.apng", import.meta.url),
  );
  const valid = () => Uint8Array.from(source).buffer;
  assert.throws(() => inspectPetApng(valid().slice(0, 50)));
  const oversized = valid();
  new DataView(oversized).setUint32(16, 10000);
  assert.throws(() => inspectPetApng(oversized));
  const truncated = valid();
  new DataView(truncated).setUint32(8, 0xffffffff);
  assert.throws(() => inspectPetApng(truncated));
});

test("APNG sampler uses embedded durations and holds final pose on completion", () => {
  assert.deepEqual(apngFrameAt([180, 90, 200], 180, false), {
    index: 1,
    done: false,
  });
  assert.deepEqual(apngFrameAt([180, 90, 200], 470, false), {
    index: 2,
    done: true,
  });
  assert.deepEqual(apngFrameAt([180, 90, 200], 470, true), {
    index: 0,
    done: false,
  });
});
