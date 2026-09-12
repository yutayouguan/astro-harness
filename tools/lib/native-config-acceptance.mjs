import { readFile, realpath, lstat } from "node:fs/promises";
import { createReadStream } from "node:fs";
import { basename, dirname, join, isAbsolute } from "node:path";

export function parseNativeArgs(args) {
	if (!args.length) return { onboarding: false };
	if (args.length === 1 && args[0] === "--onboarding") return { onboarding: true };
	if (args.length === 2 && args[0] === "--resume") return { onboarding: false, resume: args[1] };
	throw new Error("usage: node tools/verify-config-native.mjs [--onboarding | --resume <manifest.json>]");
}

// Build-isolation check only; this does not authenticate untrusted executables.
export async function assertNativeBinaryBinding(binary, root) {
	const expected = Buffer.from(root);
	let tail = Buffer.alloc(0);
	for await (const chunk of createReadStream(binary)) {
		const bytes = Buffer.concat([tail, chunk]);
		if (bytes.includes(expected)) return;
		tail = bytes.subarray(Math.max(0, bytes.length - expected.length + 1));
	}
	throw new Error(
		"QA executable does not contain its expected isolated data root",
	);
}

// A resume file may only refer back to its own private, marked QA bundle.
export async function loadNativeManifest(file) {
	const manifestFile = await realpath(file);
	const scratch = dirname(manifestFile);
	if (
		basename(manifestFile) !== "manifest.json" ||
		!basename(scratch).startsWith("astro-config-native-")
	) {
		throw new Error("Expected a private native acceptance manifest");
	}
	if ((await lstat(manifestFile)).size > 16_384)
		throw new Error("Acceptance manifest exceeds limit");
	const data = JSON.parse(await readFile(manifestFile, "utf8"));
	const identifier = `com.astroagent.configqa.${basename(scratch).split("-").at(-1).toLowerCase()}`;
	if (data.identifier !== identifier)
		throw new Error("Unexpected QA application identifier");
	for (const [key, expected] of [
		["astroRoot", join(scratch, "home")],
		["app", join(scratch, "Astro Config QA.app")],
	]) {
		if (
			typeof data[key] !== "string" ||
			!isAbsolute(data[key]) ||
			(await realpath(data[key])) !== expected ||
			(await lstat(data[key])).isSymbolicLink()
		) {
			throw new Error(`Acceptance ${key} escapes its private directory`);
		}
	}
	const log = join(scratch, "native.log");
	if (
		typeof data.log !== "string" ||
		!isAbsolute(data.log) ||
		(await realpath(dirname(data.log))) !== scratch
	) {
		throw new Error("Unexpected acceptance log directory");
	}
	if (basename(data.log) !== "native.log")
		throw new Error("Unexpected acceptance log name");
	try {
		if (!(await lstat(log)).isFile())
			throw new Error("Invalid acceptance log file");
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
	const marker = join(scratch, "home/.native-acceptance-root");
	if (
		!(await lstat(marker)).isFile() ||
		(await lstat(marker)).size !==
			Buffer.byteLength("astro-config-native-v1\n") ||
		(await readFile(marker, "utf8")) !== "astro-config-native-v1\n"
	) {
		throw new Error("Missing or invalid native acceptance marker");
	}
	const url = new URL(data.frontendUrl);
	if (
		url.protocol !== "http:" ||
		url.hostname !== "127.0.0.1" ||
		!url.port ||
		url.username ||
		url.password ||
		url.pathname !== "/" ||
		url.search ||
		url.hash
	) {
		throw new Error("Acceptance frontend must use a private loopback port");
	}
	const binary = join(
		scratch,
		"Astro Config QA.app/Contents/MacOS/astro-agent",
	);
	if ((await realpath(binary)) !== binary || !(await lstat(binary)).isFile())
		throw new Error("Invalid acceptance executable");
	await assertNativeBinaryBinding(binary, data.astroRoot);
	return {
		identifier,
		app: join(scratch, "Astro Config QA.app"),
		astroRoot: join(scratch, "home"),
		log,
		frontendUrl: url.origin,
		scratch,
	};
}
