import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import test from "node:test";

const [tokens, shell, index, app, intensity, wallpaper] = await Promise.all(
  [
    "../../styles/tokens/immersive-light.css",
    "../../styles/features/shell/immersive-light.css",
    "../../styles/index.css",
    "../../App.tsx",
    "./glassIntensity.ts",
    "./wallpaper.ts",
  ].map((path) => readFile(new URL(path, import.meta.url), "utf8")),
);

test("immersive light is the only active material recipe", () => {
  assert.ok(index.indexOf("tokens/immersive-light.css") > 0);
  assert.doesNotMatch(index, /liquid-glass\.css|glass-intensity\.css/);
  assert.match(
    tokens,
    /--content-card-background:\s*var\(--immersive-glass-background\)/,
  );
  assert.match(
    tokens,
    /--menu-overlay-bg:\s*var\(--immersive-overlay-background\)/,
  );
  assert.doesNotMatch(tokens, /--liquid-glass-|--global-liquid-glass-/);
});

test("environment color follows wallpaper, unified color, then active tone", () => {
  assert.match(
    tokens,
    /--immersive-env-primary:\s*var\([\s\S]*?--wallpaper-tone,[\s\S]*?--unified-tone,[\s\S]*?--tone,/,
  );
  assert.match(tokens, /--wallpaper-accent-2,/);
  assert.match(tokens, /--immersive-glass-color:\s*color-mix\(/);
  assert.match(tokens, /var\(--immersive-neutral\) 91%/);
  assert.doesNotMatch(tokens, /#a855f7|166,\s*131,\s*255|183,\s*139,\s*255/i);
  assert.match(wallpaper, /DEFAULT_WALLPAPER_HIGHLIGHT_COLOR = "#22b8a7"/);
});

test("one restrained shell light field owns particles and accessibility fallbacks", () => {
  assert.match(app, /className="shell-immersive-light-field"/);
  assert.equal(
    (app.match(/className="shell-immersive-light-particle"/g) ?? []).length,
    4,
  );
  assert.match(shell, /\.shell-immersive-light-field\s*\{/);
  assert.match(
    shell,
    /\.shell-immersive-light-field\s*\{[\s\S]*?z-index:\s*0;[\s\S]*?pointer-events:\s*none;/,
  );
  assert.match(
    shell,
    /\.app-shell\.has-wallpaper \.shell-immersive-light-field/,
  );
  assert.match(shell, /@keyframes immersive-particle-pulse/);
  assert.match(
    shell,
    /@media \(prefers-reduced-motion: reduce\)[\s\S]*?animation:\s*none/,
  );
  assert.match(
    shell,
    /@media \(prefers-reduced-transparency: reduce\), \(prefers-contrast: more\)[\s\S]*?display:\s*none/,
  );
  assert.ok(
    app.indexOf('className="shell-immersive-light-field"') <
      app.indexOf("{toneFade &&"),
    "the ambient field must stay behind transient shell and content layers",
  );
});

test("balanced immersive light is the default for new installations", () => {
  assert.match(intensity, /DEFAULT_GLASS_INTENSITY = 64/);
  assert.match(tokens, /var\(--glass-intensity, 0\.64\)/);
  assert.match(tokens, /--immersive-particle-opacity:/);
  assert.match(tokens, /--immersive-field-opacity:/);
  assert.match(
    tokens,
    /--immersive-glass-backdrop:\s*blur\([\s\S]*?28px \* var\(--glass-intensity, 0\.64\)/,
  );
  assert.match(
    tokens,
    /--immersive-field-opacity:\s*calc\(0\.32 \* var\(--glass-intensity, 0\.64\)\)/,
  );
});
