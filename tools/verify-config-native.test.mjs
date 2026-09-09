import test from "node:test";
import assert from "node:assert/strict";
import {
	mkdtemp,
	mkdir,
	writeFile,
	rm,
	realpath,
	symlink,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, basename } from "node:path";
import { loadNativeManifest } from "./lib/native-config-acceptance.mjs";

test("resume rejects an unbound executable", async (t) => {
	const { file, data } = await fixture(t);
	await writeFile(
		join(data.app, "Contents/MacOS/astro-agent"),
		"ordinary executable fixture",
	);
	await assert.rejects(loadNativeManifest(file), /expected isolated data root/);
});

async function fixture(t) {
	const root = await realpath(
		await mkdtemp(join(tmpdir(), "astro-config-native-")),
	);
	t.after(() => rm(root, { recursive: true, force: true }));
	const app = join(root, "Astro Config QA.app");
	const astroRoot = join(root, "home");
	await mkdir(join(app, "Contents/MacOS"), { recursive: true });
	await mkdir(astroRoot);
	await writeFile(
		join(app, "Contents/MacOS/astro-agent"),
		`never executed in unit tests; binding=${astroRoot}`,
	);
	await writeFile(
		join(astroRoot, ".native-acceptance-root"),
		"astro-config-native-v1\n",
	);
	const data = {
		identifier: `com.astroagent.configqa.${basename(root).split("-").at(-1).toLowerCase()}`,
		app,
		astroRoot,
		log: join(root, "native.log"),
		frontendUrl: "http://127.0.0.1:54321",
	};
	const file = join(root, "manifest.json");
	await writeFile(file, JSON.stringify(data));
	return { root, data, file };
}

test("resume accepts its own marked QA directory", async (t) => {
	const { root, file, data } = await fixture(t);
	assert.deepEqual(await loadNativeManifest(file), { ...data, scratch: root });
});

test("resume rejects external URLs and identity changes", async (t) => {
	const { file, data } = await fixture(t);
	for (const change of [
		{ frontendUrl: "https://example.com" },
		{ frontendUrl: "http://127.0.0.1:54321/foreign" },
		{ identifier: "com.astroagent.desktop" },
	]) {
		await writeFile(file, JSON.stringify({ ...data, ...change }));
		await assert.rejects(loadNativeManifest(file));
	}
});

test("resume rejects redirected data and log paths", async (t) => {
	const { root, file, data } = await fixture(t);
	await writeFile(file, JSON.stringify({ ...data, astroRoot: root }));
	await assert.rejects(loadNativeManifest(file));
	await writeFile(
		file,
		JSON.stringify({ ...data, log: join(tmpdir(), "native.log") }),
	);
	await assert.rejects(loadNativeManifest(file));
});

test("resume rejects a symlink marker", {
	skip: process.platform === "win32",
}, async (t) => {
	const { root, file, data } = await fixture(t);
	const marker = join(data.astroRoot, ".native-acceptance-root");
	await rm(marker);
	await writeFile(join(root, "other"), "astro-config-native-v1\n");
	await symlink(join(root, "other"), marker);
	await assert.rejects(loadNativeManifest(file));
});
