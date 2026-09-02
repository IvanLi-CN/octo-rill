import { createHash } from "node:crypto";
import { readFile, readdir, rename, writeFile } from "node:fs/promises";
import path from "node:path";

const distDir = path.resolve(import.meta.dirname, "../dist");
const manifestPath = path.join(distDir, "manifest.webmanifest");
const indexPath = path.join(distDir, "index.html");
const viteAssetExtensions = new Set([
	".css",
	".js",
	".json",
	".png",
	".svg",
	".woff",
	".woff2",
]);
const appShellAssetUrls = new Set([
	"/index.html",
	"/favicon.ico",
	"/favicon.svg",
]);
const pwaScreenshotExtensions = new Set([".png"]);
const brandAssetExtensions = new Set([".svg"]);
const reactionAssetExtensions = new Set([".svg"]);
const installIconAssets = [
	{ relativePath: "pwa/icon-192.png", manifestSrc: "/pwa/icon-192.png" },
	{ relativePath: "pwa/icon-512.png", manifestSrc: "/pwa/icon-512.png" },
	{
		relativePath: "pwa/maskable-icon-512.png",
		manifestSrc: "/pwa/maskable-icon-512.png",
	},
];

async function listFiles(dir) {
	const entries = await readdir(dir, { withFileTypes: true });
	const files = await Promise.all(
		entries.map(async (entry) => {
			const fullPath = path.join(dir, entry.name);
			if (entry.isDirectory()) {
				return listFiles(fullPath);
			}
			if (!entry.isFile()) {
				return [];
			}
			return [fullPath];
		}),
	);
	return files.flat();
}

function toUrl(filePath) {
	return `/${path.relative(distDir, filePath).split(path.sep).join("/")}`;
}

async function contentHash(filePath) {
	return createHash("sha256")
		.update(await readFile(filePath))
		.digest("hex")
		.slice(0, 16);
}

async function contentAddressInstallIcons() {
	return Promise.all(
		installIconAssets.map(async (asset) => {
			const sourcePath = path.join(distDir, asset.relativePath);
			const extension = path.extname(sourcePath);
			const basename = path.basename(sourcePath, extension);
			const digest = await contentHash(sourcePath);
			const hashedPath = path.join(
				path.dirname(sourcePath),
				`${basename}.${digest}${extension}`,
			);
			await rename(sourcePath, hashedPath);
			return {
				...asset,
				hashedPath,
				hashedUrl: toUrl(hashedPath),
			};
		}),
	);
}

const contentAddressedInstallIcons = await contentAddressInstallIcons();
const manifest = JSON.parse(await readFile(manifestPath, "utf8"));
if (!Array.isArray(manifest.icons)) {
	throw new Error("PWA manifest must declare install icons");
}

const manifestIconAssets = new Map(
	contentAddressedInstallIcons
		.filter((asset) => asset.manifestSrc)
		.map((asset) => [asset.manifestSrc, asset]),
);
const rewrittenManifestIcons = new Set();
for (const icon of manifest.icons) {
	const asset = manifestIconAssets.get(icon?.src);
	if (!asset) continue;
	icon.src = asset.hashedUrl;
	rewrittenManifestIcons.add(asset.manifestSrc);
}
if (rewrittenManifestIcons.size !== manifestIconAssets.size) {
	throw new Error("PWA manifest icons do not match the install icon assets");
}
await writeFile(manifestPath, `${JSON.stringify(manifest, null, "\t")}\n`);

function isAllowedPrecacheUrl(url) {
	const extension = path.extname(url);
	if (appShellAssetUrls.has(url)) {
		return true;
	}
	if (url.startsWith("/assets/")) {
		return viteAssetExtensions.has(extension);
	}
	if (url.startsWith("/pwa/")) {
		return (
			url.startsWith("/pwa/screenshots/") &&
			pwaScreenshotExtensions.has(extension)
		);
	}
	if (url.startsWith("/brand/")) {
		return brandAssetExtensions.has(extension);
	}
	if (url.startsWith("/reactions/")) {
		return reactionAssetExtensions.has(extension);
	}
	return false;
}

const files = (await listFiles(distDir))
	.filter((filePath) => {
		const name = path.basename(filePath);
		if (name === "sw.js" || name === "pwa-precache-manifest.json") {
			return false;
		}
		return isAllowedPrecacheUrl(toUrl(filePath));
	})
	.map((filePath) => ({ filePath, url: toUrl(filePath) }))
	.sort((a, b) => a.url.localeCompare(b.url));

if (!files.some((file) => file.url === "/index.html")) {
	throw new Error("PWA precache manifest must include /index.html");
}

const hash = createHash("sha256");
hash.update("/\n");
hash.update(await readFile(indexPath));
hash.update("\n");
for (const file of files) {
	hash.update(file.url);
	hash.update("\n");
	hash.update(await readFile(file.filePath));
	hash.update("\n");
}

const urls = Array.from(new Set(["/", ...files.map((file) => file.url)]));
const revision = hash.digest("hex").slice(0, 16);
const cacheName = `octo-rill-precache-${revision}`;

await writeFile(
	path.join(distDir, "pwa-precache-manifest.json"),
	`${JSON.stringify({ cacheName, urls }, null, 2)}\n`,
);

await writeFile(
	path.join(distDir, "sw.js"),
	`const PRECACHE_CACHE = ${JSON.stringify(cacheName)};
const PRECACHE_URLS = ${JSON.stringify(urls, null, 2)};
const APP_SHELL_URL = "/index.html";

function isBackendPath(pathname) {
	return (
		pathname === "/api" ||
		pathname.startsWith("/api/") ||
		pathname === "/auth" ||
		pathname.startsWith("/auth/")
	);
}

function isSafeSameOriginGet(request, url) {
	return (
		request.method === "GET" &&
		url.origin === self.location.origin &&
		!isBackendPath(url.pathname)
	);
}

self.addEventListener("install", (event) => {
	event.waitUntil(
		caches.open(PRECACHE_CACHE).then((cache) => cache.addAll(PRECACHE_URLS)),
	);
});

self.addEventListener("activate", (event) => {
	event.waitUntil(
		caches.keys().then((keys) =>
			Promise.all(
				keys
					.filter(
						(key) =>
							key.startsWith("octo-rill-precache-") &&
							key !== PRECACHE_CACHE,
					)
					.map((key) => caches.delete(key)),
			),
		).then(() => self.clients.claim()),
	);
});

self.addEventListener("message", (event) => {
	if (event.data?.type === "SKIP_WAITING") {
		self.skipWaiting();
	}
});

self.addEventListener("fetch", (event) => {
	const url = new URL(event.request.url);
	if (!isSafeSameOriginGet(event.request, url)) {
		return;
	}

	if (event.request.mode === "navigate") {
		event.respondWith(
			fetch(event.request).catch(() => caches.match(APP_SHELL_URL)),
		);
		return;
	}

	if (PRECACHE_URLS.includes(url.pathname)) {
		event.respondWith(
			caches
				.match(event.request)
				.then((cached) => cached ?? fetch(event.request)),
		);
	}
});
`,
);
