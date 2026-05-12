import { createHash } from "node:crypto";
import { readFile, readdir, writeFile } from "node:fs/promises";
import path from "node:path";

const distDir = path.resolve(import.meta.dirname, "../dist");
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
	"/manifest.webmanifest",
	"/favicon.ico",
	"/favicon.svg",
]);
const pwaAssetExtensions = new Set([".png"]);
const brandAssetExtensions = new Set([".svg"]);
const reactionAssetExtensions = new Set([".svg"]);

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

function isAllowedPrecacheUrl(url) {
	const extension = path.extname(url);
	if (appShellAssetUrls.has(url)) {
		return true;
	}
	if (url.startsWith("/assets/")) {
		return viteAssetExtensions.has(extension);
	}
	if (url.startsWith("/pwa/")) {
		return pwaAssetExtensions.has(extension);
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
hash.update(await readFile(path.join(distDir, "index.html")));
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
