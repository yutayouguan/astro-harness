import assert from "node:assert/strict";
import test from "node:test";

import {
  materialFileIconUrl,
  materialFolderIconUrl,
} from "./materialFileIcons.ts";

test("文件名优先于扩展名", () => {
  assert.equal(materialFileIconUrl("Cargo.lock"), "/file-icons/lock.svg");
  assert.equal(materialFileIconUrl(".gitignore"), "/file-icons/git.svg");
  assert.equal(materialFileIconUrl("package.json"), "/file-icons/nodejs.svg");
});

test("扩展名匹配忽略大小写并支持路径", () => {
  assert.equal(materialFileIconUrl("main.RS"), "/file-icons/rust.svg");
  assert.equal(
    materialFileIconUrl("crates/agent-core/src/lib.rs"),
    "/file-icons/rust.svg",
  );
});

test("复合后缀优先于末段扩展名", () => {
  assert.equal(materialFileIconUrl("chat.test.ts"), "/file-icons/test-ts.svg");
  assert.equal(materialFileIconUrl("chat.ts"), "/file-icons/typescript.svg");
});

test("无匹配时回退默认图标", () => {
  assert.equal(materialFileIconUrl("notes.qqqqq"), "/file-icons/file.svg");
  assert.equal(
    materialFolderIconUrl("some-random-dir"),
    "/file-icons/folder.svg",
  );
  assert.equal(
    materialFolderIconUrl("some-random-dir", true),
    "/file-icons/folder-open.svg",
  );
});

test("文件夹名带前缀变体也能命中，并区分展开态", () => {
  assert.equal(materialFolderIconUrl("src"), "/file-icons/folder-src.svg");
  assert.equal(
    materialFolderIconUrl("src", true),
    "/file-icons/folder-src-open.svg",
  );
  assert.equal(
    materialFolderIconUrl(".github"),
    "/file-icons/folder-github.svg",
  );
});
