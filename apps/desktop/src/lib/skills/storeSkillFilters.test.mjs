import assert from "node:assert/strict";
import test from "node:test";
import {
  STORE_CATEGORY_IDS,
  filterStoreSkills,
} from "./storeSkillFilters.ts";

const skills = [
  {
    id: "skillhub:one",
    name: "One",
    description: "",
    source: "skillhub",
    store: "skillhub",
    installs: 1,
    install_ref: "skillhub:one",
    homepage: null,
    category: "dev-programming",
    requires_api_key: true,
  },
  {
    id: "skillhub:two",
    name: "Two",
    description: "",
    source: "skillhub",
    store: "skillhub",
    installs: 2,
    install_ref: "skillhub:two",
    homepage: null,
    category: "dev-programming",
    requires_api_key: false,
  },
  {
    id: "skillhub:three",
    name: "Three",
    description: "",
    source: "skillhub",
    store: "skillhub",
    installs: 3,
    install_ref: "skillhub:three",
    homepage: null,
    category: null,
    requires_api_key: null,
  },
];

test("catalog exposes the complete scene taxonomy", () => {
  assert.deepEqual(STORE_CATEGORY_IDS, [
    "pay-skill",
    "office-efficiency",
    "content-creation",
    "dev-programming",
    "data-analysis",
    "design-media",
    "ai-agent",
    "knowledge-management",
    "business-ops",
    "education",
    "professional",
    "it-ops-security",
    "life-service",
  ]);
});

test("category and API Key filters compose", () => {
  assert.deepEqual(
    filterStoreSkills(skills, "dev-programming", "required").map(
      (skill) => skill.id,
    ),
    ["skillhub:one"],
  );
  assert.deepEqual(
    filterStoreSkills(skills, "dev-programming", "not-required").map(
      (skill) => skill.id,
    ),
    ["skillhub:two"],
  );
});

test("unknown API Key requirements are not mislabeled as key-free", () => {
  assert.deepEqual(
    filterStoreSkills(skills, "all", "not-required").map((skill) => skill.id),
    ["skillhub:two"],
  );
});
