import assert from "node:assert/strict";
import test from "node:test";

import {
  ASTRO_SPACE_ICON_ID,
  MATERIAL_PROJECT_ICONS,
  filterMaterialProjectIcons,
  isMaterialProjectIcon,
  materialProjectIconUrl,
} from "./materialProjectIcons.ts";

test("项目图标自动匹配关闭和展开资源", () => {
  assert.equal(
    materialProjectIconUrl("folder-rust", false),
    "/file-icons/folder-rust.svg",
  );
  assert.equal(
    materialProjectIconUrl("folder-rust", true),
    "/file-icons/folder-rust-open.svg",
  );
});

test("未知或旧图标回退到默认 Material 文件夹", () => {
  assert.equal(materialProjectIconUrl("layers", false), "/file-icons/folder.svg");
  assert.equal(materialProjectIconUrl(null, true), "/file-icons/folder-open.svg");
  assert.equal(isMaterialProjectIcon("layers"), false);
  assert.equal(isMaterialProjectIcon("folder-home"), true);
});

test("品牌主空间图标排在首位并单独取自 project-icons", () => {
  assert.equal(MATERIAL_PROJECT_ICONS[0]?.id, ASTRO_SPACE_ICON_ID);
  assert.equal(isMaterialProjectIcon(ASTRO_SPACE_ICON_ID), true);
  assert.equal(
    materialProjectIconUrl(ASTRO_SPACE_ICON_ID, false),
    "/project-icons/astro-space.svg",
  );
  assert.equal(
    materialProjectIconUrl(ASTRO_SPACE_ICON_ID, true),
    "/project-icons/astro-space-open.svg",
  );
  assert.ok(
    filterMaterialProjectIcons("主空间").some((icon) => icon.id === ASTRO_SPACE_ICON_ID),
  );
});

test("可以按图标名称和文件夹别名搜索", () => {
  assert.ok(filterMaterialProjectIcons("rust").some((icon) => icon.id === "folder-rust"));
  assert.ok(filterMaterialProjectIcons("github").some((icon) => icon.id === "folder-github"));
});
