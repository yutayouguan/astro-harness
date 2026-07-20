import { test } from "node:test";
import assert from "node:assert/strict";
import {
  DEFAULT_SHELL_GRADIENT,
  effectiveUnifiedTone,
  isNearBlack,
  isNearWhite,
  underlayFromGradient,
  unifiedSurfaceMode,
} from "./shellGradient.ts";

test("detects near white and near black", () => {
  assert.equal(isNearWhite("#ffffff"), true);
  assert.equal(isNearWhite("#2563eb"), false);
  assert.equal(isNearBlack("#0f172a"), true);
  assert.equal(isNearBlack("#000000"), true);
  assert.equal(isNearBlack("#94a3b8"), false);
});

test("unified surface mode for extreme picks", () => {
  assert.equal(unifiedSurfaceMode("light", "#ffffff"), "light-neutral");
  assert.equal(unifiedSurfaceMode("dark", "#0f172a"), "dark-neutral");
  assert.equal(unifiedSurfaceMode("light", "#2563eb"), "default");
});

test("effective unified tone falls back on light white / dark black", () => {
  assert.equal(effectiveUnifiedTone("light", "#ffffff"), "#64748b");
  assert.equal(effectiveUnifiedTone("dark", "#0f172a"), "#94a3b8");
  assert.equal(effectiveUnifiedTone("light", "#2563eb"), "#2563eb");
});

test("underlayFromGradient tints extreme colors", () => {
  const whiteGrad = {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary, color: "#ffffff" },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary, color: "#ffffff" },
  };
  const blackGrad = {
    ...DEFAULT_SHELL_GRADIENT,
    primary: { ...DEFAULT_SHELL_GRADIENT.primary, color: "#0f172a" },
    secondary: { ...DEFAULT_SHELL_GRADIENT.secondary, color: "#0f172a" },
  };
  const lightUnder = underlayFromGradient("light", whiteGrad);
  const darkUnder = underlayFromGradient("dark", blackGrad);
  assert.notEqual(lightUnder, "#ffffff");
  assert.notEqual(darkUnder, "#0f172a");
});
