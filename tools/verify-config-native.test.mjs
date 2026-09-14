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
import { loadNativeManifest, nativeQaProfile, parseNativeArgs } from "./lib/native-config-acceptance.mjs";

test("native onboarding is explicit and cannot reset a resumed profile", () => {
	assert.deepEqual(parseNativeArgs([]), { onboarding: false });
	assert.deepEqual(parseNativeArgs(["--onboarding"]), { onboarding: true });
	assert.deepEqual(parseNativeArgs(["--resume", "qa.json"]), { onboarding: false, resume: "qa.json" });
	assert.throws(() => parseNativeArgs(["--onboarding", "--resume", "qa.json"]));
	assert.throws(() => parseNativeArgs(["--resume"]));
	assert.deepEqual(parseNativeArgs(["--pet"]), { onboarding: false, purpose: "pet" });
	assert.throws(() => parseNativeArgs(["--pet", "--onboarding"]));
});

test("resume rejects an unbound executable", async (t) => {
	const { file, data } = await fixture(t);
	await writeFile(
		join(data.app, "Contents/MacOS/astro-agent"),
		"ordinary executable fixture",
	);
	await assert.rejects(loadNativeManifest(file), /expected isolated data root/);
});

async function fixture(t, purpose) {
	const root = await realpath(
		await mkdtemp(join(tmpdir(), "astro-config-native-")),
	);
	t.after(() => rm(root, { recursive: true, force: true }));
	const profile = nativeQaProfile(purpose);
	const app = join(root, `${profile.appName}.app`);
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
		identifier: `${profile.prefix}.${basename(root).split("-").at(-1).toLowerCase()}`,
		...(purpose ? { purpose } : {}),
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

test("pet acceptance resumes only its separately identified bundle", async (t) => {
	const { root, file, data } = await fixture(t, "pet");
	assert.deepEqual(await loadNativeManifest(file), { ...data, scratch: root });
	for (const purpose of ["config", "../pet", "production", null]) {
		await writeFile(file, JSON.stringify({ ...data, purpose }));
		await assert.rejects(loadNativeManifest(file));
	}
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
