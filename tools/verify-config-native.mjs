#!/usr/bin/env node
// macOS native acceptance launcher. Never stops another Astro instance.
import { spawn, spawnSync } from "node:child_process";
import { once } from "node:events";
import { createServer } from "node:net";
import {
	mkdtemp,
	mkdir,
	writeFile,
	copyFile,
	cp,
	readFile,
	chmod,
	realpath,
} from "node:fs/promises";
import { tmpdir } from "node:os";
import { resolve, join } from "node:path";
import { fileURLToPath } from "node:url";
import {
	loadNativeManifest,
	assertNativeBinaryBinding,
	parseNativeArgs,
	nativeQaProfile,
} from "./lib/native-config-acceptance.mjs";

if (process.platform !== "darwin")
	throw new Error("This native acceptance launcher requires macOS");
const repo = resolve(fileURLToPath(new URL("..", import.meta.url)));
const frontend = join(repo, "apps/desktop");
const args = parseNativeArgs(process.argv.slice(2));
const resumed = args.resume ? await loadNativeManifest(args.resume) : undefined;
const scratch =
	resumed?.scratch ?? (await mkdtemp(join(tmpdir(), "astro-config-native-")));
const astroRoot = join(scratch, "home");
const purpose = resumed?.purpose ?? args.purpose;
const profile = nativeQaProfile(purpose);
const identifier = `${profile.prefix}.${scratch.split("-").at(-1).toLowerCase()}`;
const appName = profile.appName;
const app = join(scratch, `${appName}.app`);
const contents = join(app, "Contents");
const log = join(scratch, "native.log");
let vite;
let opener;
let stopping = false;
let build;
let restoreBuild = false;
let nativeExecutable;
const buildArgs = ["build", "-p", "astro-harness", "--bin", "astro-harness"];
const normalBuildEnv = { ...process.env };
delete normalBuildEnv.TAURI_CONFIG;
delete normalBuildEnv.ASTRO_NATIVE_ACCEPTANCE_ROOT;

async function cleanup() {
	if (stopping) return;
	stopping = true;
	build?.kill("SIGTERM");
	// Resolve this exact private executable, never kill by the shared binary name.
	if (opener) {
		const listing =
			spawnSync("ps", ["-axo", "pid=,comm="], { encoding: "utf8" }).stdout ??
			"";
		for (const line of listing.split("\n")) {
			const match = line.trim().match(/^(\d+)\s+(.+)$/);
			if (match?.[2] === nativeExecutable) {
				try {
					process.kill(Number(match[1]), "SIGTERM");
				} catch (error) {
					if (error.code !== "ESRCH") throw error;
				}
			}
		}
	}
	vite?.kill("SIGTERM");
}
process.on("SIGINT", () => {
	void cleanup();
});
process.on("SIGTERM", () => {
	void cleanup();
});

function run(command, args, env = process.env) {
	return new Promise((resolveRun, reject) => {
		const child = spawn(command, args, { cwd: repo, env, stdio: "inherit" });
		build = child;
		child.on("error", reject);
		child.on("exit", (code, signal) => {
			build = undefined;
			code === 0
				? resolveRun()
				: reject(new Error(`${command} failed: ${code ?? signal}`));
		});
	});
}

function nativePids() {
	const output = spawnSync("ps", ["-axo", "pid=,comm="], { encoding: "utf8" });
	if (output.status !== 0)
		throw new Error("Cannot inspect native acceptance process");
	return output.stdout.split("\n").flatMap((line) => {
		const match = line.trim().match(/^(\d+)\s+(.+)$/);
		return match?.[2] === nativeExecutable ? [Number(match[1])] : [];
	});
}

try {
	let port;
	if (resumed) port = Number(new URL(resumed.frontendUrl).port);
	else {
		const socket = createServer();
		socket.listen(0, "127.0.0.1");
		await once(socket, "listening");
		port = socket.address().port;
		await new Promise((done) => socket.close(done));
	}
	const url = `http://127.0.0.1:${port}`;
	let ready = false;
	let viteError;
	vite = spawn(
		process.execPath,
		[
			join(frontend, "node_modules/vite/bin/vite.js"),
			"--config",
			join(repo, "tools/native-acceptance-vite.config.mjs"),
			"--host",
			"127.0.0.1",
			"--port",
			String(port),
			"--strictPort",
		],
		{ cwd: frontend, stdio: ["ignore", "pipe", "inherit"] },
	);
	vite.on("error", (error) => {
		viteError = error;
	});
	vite.stdout.on("data", (chunk) => {
		process.stdout.write(chunk);
		if (chunk.toString().includes(url)) ready = true;
	});
	for (let attempt = 0; attempt < 100; attempt++) {
		if (viteError) throw viteError;
		if (vite.exitCode !== null) throw new Error("Private Vite server exited");
		if (ready) break;
		await new Promise((done) => setTimeout(done, 100));
	}
	if (!ready) throw new Error("Private Vite server did not become ready");

	let manifest = resumed;
	if (!resumed) {
		// Override the compiled Tauri identity as well as bundle metadata, isolating the singleton socket.
		await mkdir(astroRoot, { recursive: true });
		await writeFile(
			join(astroRoot, ".native-acceptance-root"),
			"astro-config-native-v1\n",
		);
		restoreBuild = true;
		await run("cargo", buildArgs, {
			...process.env,
			ASTRO_NATIVE_ACCEPTANCE_ROOT: astroRoot,
			TAURI_CONFIG: JSON.stringify({
				identifier,
				productName: appName,
				build: { devUrl: url },
			}),
		});
		await mkdir(join(contents, "MacOS"), { recursive: true });
		await mkdir(join(contents, "Resources"), { recursive: true });
		await copyFile(
			join(repo, "target/debug/astro-harness"),
			join(contents, "MacOS/astro-harness"),
		);
		nativeExecutable = await realpath(join(contents, "MacOS/astro-harness"));
		await assertNativeBinaryBinding(nativeExecutable, astroRoot);
		await chmod(join(contents, "MacOS/astro-harness"), 0o755);
		await cp(
			join(frontend, "src-tauri/resources"),
			join(contents, "Resources"),
			{ recursive: true },
		);
		await writeFile(
			join(contents, "Info.plist"),
			`<?xml version="1.0" encoding="UTF-8"?>
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>astro-harness</string>
<key>CFBundleIdentifier</key><string>${identifier}</string>
<key>CFBundleName</key><string>${appName}</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSPrincipalClass</key><string>NSApplication</string>
</dict></plist>\n`,
		);
		// Restore the ordinary dev artifact before opening the copied QA bundle.
		await run("cargo", buildArgs, normalBuildEnv);
		restoreBuild = false;
		if (stopping) throw new Error("Native acceptance cancelled");
		for (const directory of ["mcp", "hooks", "ui"])
			await mkdir(join(astroRoot, directory), { recursive: true });
		// Onboarding mode starts fresh and must pass the real verification command.
		await writeFile(
			join(astroRoot, "ui/onboarding.json"),
			JSON.stringify({
				version: 1,
				step: args.onboarding ? "intro" : "complete",
				completed: !args.onboarding,
				should_show: args.onboarding,
				inferred_existing_install: false,
				updated_at: null,
			}),
		);
		await writeFile(
			join(astroRoot, "config.toml"),
			`# Native acceptance fixture; not a user's configuration.
[desktop]
settings_version = 1
[config_sources]
mcp = ["mcp/servers.toml"]
hooks = ["hooks/rules.toml"]
[cache.models]
enabled = false
[cache.mcp]
enabled = false
`,
		);
		await writeFile(
			join(astroRoot, "mcp/servers.toml"),
			`# Changes must stay in this source file.
[mcp_servers.config_qa]
name = "Config QA (disabled)"
command = "/usr/bin/true"
enabled = false
`,
		);
		await writeFile(
			join(astroRoot, "hooks/rules.toml"),
			`[[hooks.PreToolUse]]
id = "config-qa"
[[hooks.PreToolUse.hooks]]
type = "command"
command = "/usr/bin/true"
[hooks.state.config-qa]
enabled = false
`,
		);
		manifest = { identifier, app, astroRoot, log, frontendUrl: url, ...(purpose === "pet" ? { purpose } : {}) };
		await writeFile(
			join(scratch, "manifest.json"),
			JSON.stringify(manifest, null, 2),
		);
	} else {
		nativeExecutable = await realpath(join(contents, "MacOS/astro-harness"));
	}
	if (stopping) throw new Error("Native acceptance cancelled");
	console.log(`NATIVE_CONFIG_QA=${JSON.stringify(manifest)}`);
	opener = true;
	if (!nativePids().length)
		await run("/usr/bin/open", ["-n", "--stdout", log, "--stderr", log, app]);
	let observed = false;
	let missing = 0;
	for (let attempt = 0; ; attempt++) {
		if (viteError || vite.exitCode !== null || vite.signalCode) {
			throw new Error("Private frontend stopped during native acceptance");
		}
		if (nativePids().length) {
			observed = true;
			missing = 0;
		} else if (observed && ++missing >= 8) break;
		if (!observed && attempt >= 120)
			throw new Error("Native application did not start");
		if (stopping) break;
		await new Promise((done) => setTimeout(done, 250));
	}
	opener = undefined;
	try {
		await readFile(join(astroRoot, ".env"));
		throw new Error("Unexpected .env file in isolated native acceptance root");
	} catch (error) {
		if (error.code !== "ENOENT") throw error;
	}
	console.log(`Native acceptance app closed; artifacts retained at ${scratch}`);
} finally {
	await cleanup();
	if (restoreBuild) await run("cargo", buildArgs, normalBuildEnv);
}
