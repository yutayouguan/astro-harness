import assert from "node:assert/strict";
import test from "node:test";
import { filterPets, scenesForPet, type PetRecord } from "./petLibrary.ts";
import { DEFAULT_PET_PREFERENCES } from "./petPreferences.ts";
import type { PetScene } from "./petScene.ts";

const pet: PetRecord = {
  id: "pet-one",
  builtin: true,
  identity: {
    petId: "pet-one",
    petPath: "one.png",
    groomingPath: null,
    sourcePath: null,
    displayName: "奶糖",
    description: null,
    provider: null,
    model: null,
    spriteVersionNumber: 2,
  },
  defaults: { scale: 0.4, behavior: DEFAULT_PET_PREFERENCES },
};
test("library search includes builtin pets without scenes and does not mutate input", () => {
  const custom = {
    ...pet,
    id: "pet-two",
    builtin: false,
    identity: { ...pet.identity, petId: "pet-two", displayName: "Pudding" },
  };
  const pets = [pet, custom];
  assert.deepEqual(filterPets(pets, "  pudding ", "custom"), [custom]);
  assert.deepEqual(filterPets(pets, "", "builtin"), [pet]);
  assert.deepEqual(filterPets(pets, "missing", "all"), []);
  assert.deepEqual(pets, [pet, custom]);
});
test("scene lookup follows pet id, not name or upgraded asset paths", () => {
  const a: PetScene = {
    id: "a",
    name: "Forest",
    pet: pet.identity,
    preferences: null,
    style: null,
    wallpaperPath: null,
    favorite: false,
    inUse: false,
  };
  const b = {
    ...a,
    id: "b",
    pet: { ...pet.identity, petPath: "upgraded.webp" },
    favorite: true,
  };
  const c = { ...a, id: "c", pet: { ...pet.identity, petId: "pet-two" } };
  assert.deepEqual(
    scenesForPet([a, b, c], "pet-one").map((s) => s.id),
    ["b", "a"],
  );
});
