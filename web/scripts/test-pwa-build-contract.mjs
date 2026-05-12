import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import path from "node:path";

const distDir = path.resolve(import.meta.dirname, "../dist");
const manifestPath = path.join(distDir, "manifest.webmanifest");
const precachePath = path.join(distDir, "pwa-precache-manifest.json");
const serviceWorkerPath = path.join(distDir, "sw.js");

async function readJson(filePath) {
	return JSON.parse(await readFile(filePath, "utf8"));
}

async function readPngSize(filePath) {
	const buffer = await readFile(filePath);
	assert.equal(buffer.toString("ascii", 1, 4), "PNG", `${filePath} is PNG`);
	assert.equal(
		buffer.toString("ascii", 12, 16),
		"IHDR",
		`${filePath} has IHDR`,
	);
	return {
		width: buffer.readUInt32BE(16),
		height: buffer.readUInt32BE(20),
	};
}

function assertNoPrivatePath(urls) {
	for (const url of urls) {
		assert.equal(typeof url, "string", "precache URL must be a string");
		assert(!url.startsWith("/api"), `precache must exclude API URL: ${url}`);
		assert(!url.startsWith("/auth"), `precache must exclude auth URL: ${url}`);
		assert(
			!url.includes("://"),
			`precache must stay same-origin relative: ${url}`,
		);
	}
}

const manifest = await readJson(manifestPath);
assert.equal(manifest.name, "OctoRill");
assert.equal(manifest.short_name, "OctoRill");
assert.equal(manifest.id, "/");
assert.equal(manifest.start_url, "/");
assert.equal(manifest.scope, "/");
assert.equal(manifest.display, "standalone");
assert.equal(manifest.theme_color, "#0f172a");
assert.equal(manifest.background_color, "#0f172a");
assert.deepEqual(manifest.categories, ["productivity", "utilities", "news"]);
assert(Array.isArray(manifest.icons), "manifest icons must be an array");

const expectedIcons = new Map([
	["/pwa/icon-192.png", { width: 192, height: 192, maskable: false }],
	["/pwa/icon-512.png", { width: 512, height: 512, maskable: false }],
	["/pwa/maskable-icon-512.png", { width: 512, height: 512, maskable: true }],
]);

for (const [src, expected] of expectedIcons) {
	const icon = manifest.icons.find((candidate) => candidate?.src === src);
	assert(icon, `manifest includes ${src}`);
	assert.equal(icon.sizes, `${expected.width}x${expected.height}`);
	assert.equal(icon.type, "image/png");
	if (expected.maskable) {
		assert(
			typeof icon.purpose === "string" && icon.purpose.includes("maskable"),
			`${src} must be maskable`,
		);
	}
	const actual = await readPngSize(path.join(distDir, src));
	assert.deepEqual(actual, {
		width: expected.width,
		height: expected.height,
	});
}

const appleTouchIcon = await readPngSize(
	path.join(distDir, "pwa/apple-touch-icon.png"),
);
assert.deepEqual(appleTouchIcon, { width: 180, height: 180 });

const expectedScreenshots = new Map([
	[
		"/pwa/screenshot-wide.png",
		{ width: 3200, height: 2400, formFactor: "wide" },
	],
	[
		"/pwa/screenshot-narrow.png",
		{ width: 780, height: 1688, formFactor: "narrow" },
	],
]);
assert(
	Array.isArray(manifest.screenshots),
	"manifest screenshots must be array",
);
for (const [src, expected] of expectedScreenshots) {
	const screenshot = manifest.screenshots.find(
		(candidate) => candidate?.src === src,
	);
	assert(screenshot, `manifest includes screenshot ${src}`);
	assert.equal(screenshot.sizes, `${expected.width}x${expected.height}`);
	assert.equal(screenshot.type, "image/png");
	assert.equal(screenshot.form_factor, expected.formFactor);
	assert.equal(typeof screenshot.label, "string", `${src} has label`);
	const actual = await readPngSize(path.join(distDir, src));
	assert.deepEqual(actual, {
		width: expected.width,
		height: expected.height,
	});
}

assert(Array.isArray(manifest.shortcuts), "manifest shortcuts must be array");
assert.deepEqual(
	manifest.shortcuts.map((shortcut) => shortcut.url),
	["/", "/releases", "/briefs"],
);
for (const shortcut of manifest.shortcuts) {
	assert.equal(typeof shortcut.name, "string", "shortcut has name");
	assert(shortcut.name.length > 0, "shortcut name is not empty");
	assert(
		shortcut.icons?.some((icon) => icon.src === "/pwa/icon-192.png"),
		`${shortcut.name} shortcut uses install icon`,
	);
}

const precache = await readJson(precachePath);
assert.match(precache.cacheName, /^octo-rill-precache-[0-9a-f]{16}$/);
assert(Array.isArray(precache.urls), "precache urls must be an array");
assert(precache.urls.includes("/"), "precache includes root app shell alias");
assert(precache.urls.includes("/index.html"), "precache includes index.html");
assert(
	precache.urls.includes("/manifest.webmanifest"),
	"precache includes manifest",
);
assert(
	precache.urls.includes("/pwa/icon-192.png"),
	"precache includes install icon",
);
assert(
	precache.urls.includes("/pwa/screenshot-wide.png"),
	"precache includes wide screenshot",
);
assert(
	precache.urls.includes("/pwa/screenshot-narrow.png"),
	"precache includes narrow screenshot",
);
assert(
	precache.urls.some(
		(url) => url.startsWith("/assets/") && url.endsWith(".js"),
	),
	"precache includes Vite JS assets",
);
assert(
	!precache.urls.includes("/vite.svg"),
	"precache excludes default Vite SVG",
);
assertNoPrivatePath(precache.urls);

const serviceWorker = await readFile(serviceWorkerPath, "utf8");
assert(serviceWorker.includes("function isBackendPath(pathname)"));
assert(serviceWorker.includes('pathname.startsWith("/api/")'));
assert(serviceWorker.includes('pathname.startsWith("/auth/")'));
assert(serviceWorker.includes('request.method === "GET"'));
assert(serviceWorker.includes("url.origin === self.location.origin"));
assert(serviceWorker.includes('event.request.mode === "navigate"'));
assert(
	serviceWorker.includes('worker.postMessage({ type: "SKIP_WAITING" })') ===
		false,
);
assert(serviceWorker.includes('event.data?.type === "SKIP_WAITING"'));
assert(serviceWorker.includes("self.skipWaiting()"));

console.log("PWA build contract tests passed");
