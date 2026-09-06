import type { StoreSkill } from "../../types";

export const STORE_CATEGORY_IDS = [
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
] as const;

export type StoreCategory = (typeof STORE_CATEGORY_IDS)[number];
export type StoreCategoryFilter = "all" | StoreCategory;
export type StoreApiKeyFilter = "all" | "required" | "not-required";

export function filterStoreSkills(
  skills: StoreSkill[],
  category: StoreCategoryFilter,
  apiKey: StoreApiKeyFilter,
): StoreSkill[] {
  return skills.filter((skill) => {
    if (category !== "all" && skill.category !== category) return false;
    if (apiKey === "required" && skill.requires_api_key !== true) return false;
    if (apiKey === "not-required" && skill.requires_api_key !== false)
      return false;
    return true;
  });
}
