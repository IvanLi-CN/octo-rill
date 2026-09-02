import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { parse } from "parse5";

const distDir = path.resolve(import.meta.dirname, "../dist");
const manifestPath = path.join(distDir, "manifest.webmanifest");
const indexPath = path.join(distDir, "index.html");
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

function findElements(document, tagName) {
	const elements = [];
	const visit = (node) => {
		if (node?.nodeName === tagName) {
			elements.push(node);
		}
		for (const child of node?.childNodes ?? []) {
			visit(child);
		}
	};
	visit(document);
	return elements;
}

function getAttribute(element, name) {
	return element.attrs.find((attribute) => attribute.name === name)?.value;
}

function hasRel(element, value) {
	return (getAttribute(element, "rel") ?? "")
		.split(/\s+/)
		.filter(Boolean)
		.includes(value);
}

function requireMetaContent(elements, name) {
	const meta = elements.find(
		(element) => getAttribute(element, "name") === name,
	);
	assert(meta, `index includes ${name} meta`);
	return getAttribute(meta, "content");
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
const indexHtml = await readFile(indexPath, "utf8");
const document = parse(indexHtml);
const linkElements = findElements(document, "link");
const metaElements = findElements(document, "meta");
const manifestLinks = linkElements.filter((element) =>
	hasRel(element, "manifest"),
);
assert.equal(manifestLinks.length, 1, "index has one manifest link");
assert.equal(
	getAttribute(manifestLinks[0], "href"),
	"/manifest.webmanifest",
	"index manifest link points to the root manifest",
);
assert.equal(
	linkElements.filter((element) => hasRel(element, "apple-touch-icon")).length,
	0,
	"product index does not declare an Apple touch icon",
);
assert.equal(manifest.name, "OctoRill");
assert.equal(manifest.short_name, "OctoRill");
assert.equal(manifest.id, "/");
assert.equal(manifest.start_url, "/");
assert.equal(manifest.scope, "/");
assert.equal(manifest.display, "standalone");
assert.deepEqual(manifest.display_override, ["standalone", "browser"]);
assert.equal(manifest.theme_color, "#0f172a");
assert.equal(manifest.background_color, "#0f172a");
assert.deepEqual(manifest.categories, ["productivity", "utilities"]);
assert.equal(
	requireMetaContent(metaElements, "mobile-web-app-capable"),
	"yes",
	"index includes Android standalone meta",
);
assert.equal(
	requireMetaContent(metaElements, "apple-mobile-web-app-capable"),
	"yes",
	"index includes iOS standalone meta",
);
assert.equal(
	requireMetaContent(metaElements, "apple-mobile-web-app-title"),
	"OctoRill",
	"index includes iOS app title",
);
assert.equal(
	requireMetaContent(metaElements, "apple-mobile-web-app-status-bar-style"),
	"black-translucent",
	"index includes iOS status bar style",
);
assert(Array.isArray(manifest.icons), "manifest icons must be an array");
assert(
	Array.isArray(manifest.shortcuts),
	"manifest shortcuts must be an array",
);
assert(
	Array.isArray(manifest.screenshots),
	"manifest screenshots must be an array",
);

const expectedShortcuts = new Map([
	["/", "工作台"],
	["/admin", "管理"],
	["/settings", "设置"],
]);

for (const [url, expectedName] of expectedShortcuts) {
	const shortcut = manifest.shortcuts.find(
		(candidate) => candidate?.url === url,
	);
	assert(shortcut, `manifest includes shortcut ${url}`);
	assert.equal(shortcut.name, expectedName);
	assert.equal(shortcut.short_name, expectedName);
	assert.equal(typeof shortcut.description, "string");
	assert(shortcut.description.length > 0, `${url} shortcut has description`);
}

const expectedIcons = new Map([
	["icon-192", { width: 192, height: 192, maskable: false }],
	["icon-512", { width: 512, height: 512, maskable: false }],
	["maskable-icon-512", { width: 512, height: 512, maskable: true }],
]);

for (const [basename, expected] of expectedIcons) {
	const icon = manifest.icons.find((candidate) =>
		candidate?.src?.startsWith(`/pwa/${basename}.`),
	);
	assert(icon, `manifest includes ${basename} install icon`);
	assert.match(
		icon.src,
		new RegExp(`^/pwa/${basename}\\.[0-9a-f]{16}\\.png$`),
		`${basename} icon URL is content hashed`,
	);
	assert.equal(icon.sizes, `${expected.width}x${expected.height}`);
	assert.equal(icon.type, "image/png");
	if (expected.maskable) {
		assert(
			typeof icon.purpose === "string" && icon.purpose.includes("maskable"),
			`${basename} must be maskable`,
		);
	}
	const iconBytes = await readFile(path.join(distDir, icon.src.slice(1)));
	const digest = createHash("sha256")
		.update(iconBytes)
		.digest("hex")
		.slice(0, 16);
	assert.equal(
		icon.src.match(/\.([0-9a-f]{16})\.png$/)?.[1],
		digest,
		`${basename} icon URL matches its file content`,
	);
	const actual = await readPngSize(path.join(distDir, icon.src.slice(1)));
	assert.deepEqual(actual, {
		width: expected.width,
		height: expected.height,
	});
}
assert.equal(
	manifest.icons.length,
	expectedIcons.size,
	"manifest contains only the supported regular and maskable install icons",
);

const expectedScreenshots = new Map([
	[
		"/pwa/screenshots/dashboard-warm-skeleton-mobile-shell.png",
		{
			width: 780,
			height: 1688,
			formFactor: "narrow",
			label: "Mobile dashboard shell",
		},
	],
	[
		"/pwa/screenshots/app-shell-update-notice.png",
		{
			width: 2880,
			height: 2400,
			formFactor: "wide",
			label: "App shell update notice",
		},
	],
]);

for (const [src, expected] of expectedScreenshots) {
	const screenshot = manifest.screenshots.find(
		(candidate) => candidate?.src === src,
	);
	assert(screenshot, `manifest includes screenshot ${src}`);
	assert.equal(screenshot.sizes, `${expected.width}x${expected.height}`);
	assert.equal(screenshot.type, "image/png");
	assert.equal(screenshot.form_factor, expected.formFactor);
	assert.equal(screenshot.label, expected.label);
	const actual = await readPngSize(path.join(distDir, src));
	assert.deepEqual(actual, {
		width: expected.width,
		height: expected.height,
	});
}

const precache = await readJson(precachePath);
assert.match(precache.cacheName, /^octo-rill-precache-[0-9a-f]{16}$/);
assert(Array.isArray(precache.urls), "precache urls must be an array");
assert(precache.urls.includes("/"), "precache includes root app shell alias");
assert(precache.urls.includes("/index.html"), "precache includes index.html");
assert(
	!precache.urls.includes("/manifest.webmanifest"),
	"precache excludes manifest so installers can revalidate it",
);
assert(
	!precache.urls.some(
		(url) => url.startsWith("/pwa/") && !url.startsWith("/pwa/screenshots/"),
	),
	"precache excludes install icons so browsers can fetch current metadata",
);
assert(
	precache.urls.includes(
		"/pwa/screenshots/dashboard-warm-skeleton-mobile-shell.png",
	),
	"precache includes narrow install screenshot",
);
assert(
	precache.urls.includes("/pwa/screenshots/app-shell-update-notice.png"),
	"precache includes wide install screenshot",
);
assert(
	precache.urls.some(
		(url) => url.startsWith("/assets/") && url.endsWith(".js"),
	),
	"precache includes Vite JS assets",
);
assert(
	precache.urls.includes("/reactions/heart.svg"),
	"precache includes static reaction icons",
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
	!serviceWorker.includes("/manifest.webmanifest"),
	"service worker does not pin the manifest",
);
assert(
	!serviceWorker.includes("/pwa/icon-") &&
		!serviceWorker.includes("/pwa/maskable-icon-"),
	"service worker does not pin install icons",
);
assert(
	serviceWorker.includes('worker.postMessage({ type: "SKIP_WAITING" })') ===
		false,
);
assert(serviceWorker.includes('event.data?.type === "SKIP_WAITING"'));
assert(serviceWorker.includes("self.skipWaiting()"));

console.log("PWA build contract tests passed");
