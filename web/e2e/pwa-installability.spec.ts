import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";
import {
	createServer,
	type IncomingMessage,
	type ServerResponse,
} from "node:http";
import path from "node:path";
import { expect, test, type CDPSession, type Page } from "@playwright/test";

test.describe.configure({ mode: "serial" });

test.beforeAll(() => {
	execFileSync("bun", ["run", "build"], {
		cwd: path.resolve(import.meta.dirname, ".."),
		stdio: "inherit",
	});
});

type StaticPwaServer = {
	origin: string;
	setApiVersion: (version: string) => void;
	setServiceWorkerRevision: (revision: number) => void;
	setPwaRelease: (release: "v1" | "v2") => void;
	getApiMeRequests: () => number;
	getManifestRequests: () => number;
	getBrowserManifestRequests: () => number;
	getInstallIconRequests: () => number;
	getServiceWorkerRequests: () => number;
	getSkipWaitingMessages: () => number;
	close: () => Promise<void>;
};

const distDir = path.resolve(import.meta.dirname, "../dist");

const contentTypes = new Map([
	[".css", "text/css; charset=utf-8"],
	[".html", "text/html; charset=utf-8"],
	[".ico", "image/x-icon"],
	[".js", "text/javascript; charset=utf-8"],
	[".json", "application/json; charset=utf-8"],
	[".png", "image/png"],
	[".svg", "image/svg+xml; charset=utf-8"],
	[".webmanifest", "application/manifest+json; charset=utf-8"],
	[".woff", "font/woff"],
	[".woff2", "font/woff2"],
]);

function isContentHashedInstallIconPath(pathname: string) {
	return /^\/pwa\/(?:icon-192|icon-512|maskable-icon-512)\.[0-9a-f]{16}\.png$/.test(
		pathname,
	);
}

type PwaRelease = {
	manifest: Buffer;
	installIcons: Map<string, Buffer>;
};

const pngCrcTable = Uint32Array.from({ length: 256 }, (_, index) => {
	let crc = index;
	for (let bit = 0; bit < 8; bit += 1) {
		crc = (crc & 1) === 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1;
	}
	return crc >>> 0;
});

function pngCrc32(bytes: Buffer): number {
	let crc = 0xffffffff;
	for (const byte of bytes) {
		crc = pngCrcTable[(crc ^ byte) & 0xff] ^ (crc >>> 8);
	}
	return ~crc >>> 0;
}

function addPngTextChunk(png: Buffer, text: string): Buffer {
	const chunkType = Buffer.from("tEXt", "ascii");
	const chunkData = Buffer.from(`octo-rill-release\0${text}`, "latin1");
	const chunk = Buffer.alloc(12 + chunkData.length);
	chunk.writeUInt32BE(chunkData.length, 0);
	chunkType.copy(chunk, 4);
	chunkData.copy(chunk, 8);
	const crc = Buffer.alloc(4);
	crc.writeUInt32BE(pngCrc32(Buffer.concat([chunkType, chunkData])), 0);
	crc.copy(chunk, 8 + chunkData.length);

	const iendTypeOffset = png.lastIndexOf(Buffer.from("IEND", "ascii"));
	if (iendTypeOffset < 4) throw new Error("V1 icon is missing its IEND chunk");
	return Buffer.concat([
		png.subarray(0, iendTypeOffset - 4),
		chunk,
		png.subarray(iendTypeOffset - 4),
	]);
}

async function createPwaReleases(): Promise<Record<"v1" | "v2", PwaRelease>> {
	const manifestPath = path.join(distDir, "manifest.webmanifest");
	const v1Manifest = await readFile(manifestPath);
	const manifest = JSON.parse(v1Manifest.toString("utf8")) as {
		icons?: Array<{
			src?: string;
			sizes?: string;
			type?: string;
			purpose?: string;
		}>;
	};
	const v1InstallIcons = new Map<string, Buffer>();
	for (const icon of manifest.icons ?? []) {
		if (!icon.src) throw new Error("V1 manifest icon is missing src");
		v1InstallIcons.set(
			icon.src,
			await readFile(path.join(distDir, icon.src.slice(1))),
		);
	}

	const v2Manifest = JSON.parse(v1Manifest.toString("utf8")) as typeof manifest;
	const v2InstallIcons = new Map(v1InstallIcons);
	const v1RegularIcon = manifest.icons?.find((icon) =>
		icon.src?.startsWith("/pwa/icon-192."),
	);
	if (!v1RegularIcon?.src)
		throw new Error("V1 manifest did not declare icon-192");
	const v1RegularIconBytes = v1InstallIcons.get(v1RegularIcon.src);
	if (!v1RegularIconBytes) throw new Error("V1 icon-192 bytes are missing");

	// Keep the V2 fixture pixel-identical while changing its content identity.
	const v2RegularIconBytes = addPngTextChunk(v1RegularIconBytes, "v2");
	const v2RegularIconDigest = createHash("sha256")
		.update(v2RegularIconBytes)
		.digest("hex")
		.slice(0, 16);
	const v2RegularIconSrc = `/pwa/icon-192.${v2RegularIconDigest}.png`;
	v2InstallIcons.delete(v1RegularIcon.src);
	v2InstallIcons.set(v2RegularIconSrc, v2RegularIconBytes);
	v2Manifest.icons = (v2Manifest.icons ?? []).map((icon) =>
		icon.src === v1RegularIcon.src ? { ...icon, src: v2RegularIconSrc } : icon,
	);

	return {
		v1: { manifest: v1Manifest, installIcons: v1InstallIcons },
		v2: {
			manifest: Buffer.from(`${JSON.stringify(v2Manifest, null, "\t")}\n`),
			installIcons: v2InstallIcons,
		},
	};
}

async function startStaticPwaServer(): Promise<StaticPwaServer> {
	const releases = await createPwaReleases();
	let apiVersion = "0.1.0";
	let serviceWorkerRevision = 1;
	let activeRelease: PwaRelease = releases.v1;
	let apiMeRequests = 0;
	let manifestRequests = 0;
	let browserManifestRequests = 0;
	let installIconRequests = 0;
	let serviceWorkerRequests = 0;
	let skipWaitingMessages = 0;

	const server = createServer(
		async (request: IncomingMessage, response: ServerResponse) => {
			const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
			if (requestUrl.pathname === "/api/me") {
				apiMeRequests += 1;
				writeJson(response, 401, {
					error: { code: "unauthorized", message: "unauthorized" },
				});
				return;
			}
			if (requestUrl.pathname === "/api/version") {
				writeJson(response, 200, {
					ok: true,
					version: apiVersion,
					source: "APP_EFFECTIVE_VERSION",
				});
				return;
			}
			if (requestUrl.pathname === "/api/health") {
				writeJson(response, 200, { ok: true, version: apiVersion });
				return;
			}
			if (requestUrl.pathname === "/auth/logout") {
				response.writeHead(204, {
					"cache-control": "no-store",
				});
				response.end();
				return;
			}
			if (
				requestUrl.pathname === "/__sw-skip-waiting" &&
				request.method === "POST"
			) {
				skipWaitingMessages += 1;
				response.writeHead(204);
				response.end();
				return;
			}

			try {
				if (requestUrl.pathname === "/manifest.webmanifest") {
					manifestRequests += 1;
					if (request.headers["sec-fetch-dest"] === "manifest") {
						browserManifestRequests += 1;
					}
				}
				if (isContentHashedInstallIconPath(requestUrl.pathname)) {
					installIconRequests += 1;
				}
				let body = activeRelease.installIcons.get(requestUrl.pathname);
				if (requestUrl.pathname === "/manifest.webmanifest") {
					body = activeRelease.manifest;
				}
				if (!body) {
					body = await readFile(resolveDistPath(requestUrl.pathname));
				}
				if (requestUrl.pathname === "/sw.js") {
					serviceWorkerRequests += 1;
					body = Buffer.concat([
						body,
						Buffer.from(
							`
self.__OCTORILL_TEST_SW_REVISION = ${JSON.stringify(serviceWorkerRevision)};
self.addEventListener("message", (event) => {
	if (event.data?.type === "SKIP_WAITING") {
		fetch("/__sw-skip-waiting", { method: "POST" }).catch(() => {});
	}
});
`,
						),
					]);
				}
				response.writeHead(200, {
					"cache-control":
						requestUrl.pathname.startsWith("/assets/") ||
						isContentHashedInstallIconPath(requestUrl.pathname)
							? "public, max-age=31536000, immutable"
							: "no-cache",
					"content-type":
						contentTypes.get(
							path.extname(
								requestUrl.pathname === "/"
									? "/index.html"
									: requestUrl.pathname,
							),
						) ?? "application/octet-stream",
				});
				response.end(body);
			} catch {
				const fallback = await readFile(path.join(distDir, "index.html"));
				response.writeHead(200, {
					"cache-control": "no-store, no-cache, must-revalidate",
					"content-type": "text/html; charset=utf-8",
				});
				response.end(fallback);
			}
		},
	);

	await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
	const address = server.address();
	if (!address || typeof address === "string") {
		throw new Error("failed to bind static PWA server");
	}

	return {
		origin: `http://127.0.0.1:${address.port}`,
		setApiVersion(version: string) {
			apiVersion = version;
		},
		setServiceWorkerRevision(revision: number) {
			serviceWorkerRevision = revision;
		},
		setPwaRelease(release: "v1" | "v2") {
			activeRelease = releases[release];
		},
		getApiMeRequests() {
			return apiMeRequests;
		},
		getManifestRequests() {
			return manifestRequests;
		},
		getBrowserManifestRequests() {
			return browserManifestRequests;
		},
		getInstallIconRequests() {
			return installIconRequests;
		},
		getServiceWorkerRequests() {
			return serviceWorkerRequests;
		},
		getSkipWaitingMessages() {
			return skipWaitingMessages;
		},
		close: () =>
			new Promise<void>((resolve, reject) => {
				server.close((error) => (error ? reject(error) : resolve()));
			}),
	};
}

function resolveDistPath(pathname: string): string {
	const relativePath = pathname === "/" ? "index.html" : pathname.slice(1);
	const resolvedPath = path.resolve(distDir, relativePath);
	if (!resolvedPath.startsWith(distDir + path.sep)) {
		throw new Error("request escaped dist root");
	}
	return resolvedPath;
}

function writeJson(
	response: ServerResponse,
	status: number,
	payload: unknown,
): void {
	response.writeHead(status, {
		"cache-control": "no-store",
		"content-type": "application/json; charset=utf-8",
	});
	response.end(JSON.stringify(payload));
}

async function waitForServiceWorkerControl(page: Page) {
	await page.evaluate(async () => {
		await navigator.serviceWorker.ready;
		if (navigator.serviceWorker.controller) return;

		await new Promise<void>((resolve) => {
			navigator.serviceWorker.addEventListener(
				"controllerchange",
				() => resolve(),
				{ once: true },
			);
		});
	});
}

function manifestPath(value: string | undefined, origin: string) {
	return value ? new URL(value, origin).pathname : value;
}

async function readInstallMetadata(page: Page, cdpSession: CDPSession) {
	const appManifest = (await cdpSession.send("Page.getAppManifest")) as {
		url?: string;
		errors?: Array<{ message?: string }>;
		manifest?: {
			id?: string;
			startUrl?: string;
			scope?: string;
			icons?: Array<{ url?: string }>;
		};
	};
	if (appManifest.errors?.length) {
		throw new Error(
			`browser manifest parser failed: ${appManifest.errors
				.map((error) => error.message)
				.join("; ")}`,
		);
	}
	if (!appManifest.manifest) {
		throw new Error("browser manifest parser returned no manifest");
	}

	const origin = new URL(page.url()).origin;
	const iconUrls = (appManifest.manifest.icons ?? []).map((icon) => {
		if (!icon.url) throw new Error("browser manifest icon is missing url");
		return icon.url;
	});
	const icons = await page.evaluate(async (urls) => {
		return Promise.all(
			urls.map(async (url) => {
				const iconResponse = await fetch(url);
				if (!iconResponse.ok) {
					throw new Error(`icon request failed: ${iconResponse.status}`);
				}
				return {
					src: new URL(url).pathname,
					byteLength: (await iconResponse.arrayBuffer()).byteLength,
				};
			}),
		);
	}, iconUrls);

	return {
		id: manifestPath(appManifest.manifest.id, origin),
		startUrl: manifestPath(appManifest.manifest.startUrl, origin),
		scope: manifestPath(appManifest.manifest.scope, origin),
		icons,
	};
}

async function dispatchBeforeInstallPrompt(
	page: Page,
	outcome: "accepted" | "dismissed" = "accepted",
) {
	await page.evaluate(async (nextOutcome) => {
		type PromptChoice = {
			outcome: "accepted" | "dismissed";
			platform: string;
		};

		type InstallPromptEvent = Event & {
			prompt: () => Promise<void>;
			userChoice: Promise<PromptChoice>;
		};

		const state = (
			window as Window & {
				__octoRillInstallPromptState?: { promptCalls: number };
			}
		).__octoRillInstallPromptState ?? { promptCalls: 0 };
		(
			window as Window & {
				__octoRillInstallPromptState?: { promptCalls: number };
			}
		).__octoRillInstallPromptState = state;

		const event = new Event("beforeinstallprompt", {
			cancelable: true,
		}) as InstallPromptEvent;
		event.prompt = async () => {
			state.promptCalls += 1;
		};
		event.userChoice = Promise.resolve({
			outcome: nextOutcome,
			platform: "web",
		});
		window.dispatchEvent(event);
	}, outcome);
}

async function dispatchAppInstalled(page: Page) {
	await page.evaluate(() => {
		window.dispatchEvent(new Event("appinstalled"));
	});
}

async function seedAuthenticatedStartupCache(
	page: Page,
	options: {
		tab?: "all" | "releases" | "stars" | "followers";
		feedItems?: unknown[];
	},
) {
	await page.evaluate((input) => {
		const savedAt = Date.now();
		const me = {
			user: {
				id: "local-user-1",
				github_user_id: 30215105,
				login: "IvanLi-CN",
				name: "Ivan Li",
				avatar_url: "https://avatars.githubusercontent.com/u/30215105?v=4",
				email: null,
				is_admin: true,
			},
			access_sync: {
				task_id: null,
				task_type: null,
				event_path: null,
				reason: "none",
			},
			dashboard: {
				daily_boundary_local: "08:00",
				daily_boundary_time_zone: "Asia/Shanghai",
				daily_boundary_utc_offset_minutes: 480,
			},
		};
		window.localStorage.setItem(
			"octo-rill.auth-bootstrap.v3",
			JSON.stringify({ savedAt, me }),
		);
		if (input.feedItems) {
			window.localStorage.setItem(
				"octo-rill.dashboard-warm.v1",
				JSON.stringify({
					savedAt,
					userId: me.user.id,
					routeState: {
						tab: input.tab ?? "all",
						activeReleaseId: null,
						activeReleaseLocatorKey: null,
						releaseReturnTab: "briefs",
					},
					feedRequestType: input.tab ?? "all",
					feedItems: input.feedItems,
					nextCursor: null,
					notifications: [],
					briefs: [],
					selectedBriefId: null,
				}),
			);
		}
	}, options);
}

function cachedReleaseFeedItem() {
	return {
		kind: "release",
		ts: "2026-04-04T02:30:00Z",
		id: "offline-cached-release",
		repo_full_name: "octo-rill/app-shell",
		repo_visual: null,
		title: "v0.8.0",
		body: "Cached release notes remain readable while the network is unavailable.",
		body_truncated: false,
		subtitle: "OctoRill app shell",
		reason: null,
		subject_type: null,
		html_url: "https://github.com/IvanLi-CN/octo-rill/releases/tag/v0.8.0",
		unread: null,
		actor: null,
		translated: null,
		smart: null,
		reactions: null,
	};
}

test("app exposes installable PWA metadata without blocking anonymous login", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);

		await expect(page.locator('link[rel~="manifest"]')).toHaveAttribute(
			"href",
			"/manifest.webmanifest",
		);
		await expect(page.locator('link[rel~="apple-touch-icon"]')).toHaveCount(0);
		await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
			"content",
			"#0f172a",
		);
		await expect(
			page.locator('meta[name="mobile-web-app-capable"]'),
		).toHaveAttribute("content", "yes");
		await expect(
			page.locator('meta[name="apple-mobile-web-app-capable"]'),
		).toHaveAttribute("content", "yes");
		await expect(
			page.locator('meta[name="apple-mobile-web-app-title"]'),
		).toHaveAttribute("content", "OctoRill");
		await expect(
			page.locator('meta[name="apple-mobile-web-app-status-bar-style"]'),
		).toHaveAttribute("content", "black-translucent");
		await expect(
			page.getByRole("link", { name: "使用 GitHub 登录" }),
		).toBeVisible();

		const manifestResponse = await page.request.get(
			`${server.origin}/manifest.webmanifest`,
		);
		expect(manifestResponse.ok()).toBe(true);
		const manifest = (await manifestResponse.json()) as {
			id?: string;
			start_url?: string;
			scope?: string;
			name?: string;
			display?: string;
			icons?: Array<{ src?: string; purpose?: string }>;
			screenshots?: Array<{ src?: string; form_factor?: string }>;
			shortcuts?: Array<{ name?: string; url?: string }>;
		};
		expect(manifest.id).toBe("/");
		expect(manifest.start_url).toBe("/");
		expect(manifest.scope).toBe("/");
		expect(manifest.name).toBe("OctoRill");
		expect(manifest.display).toBe("standalone");
		expect(manifest.icons?.map((icon) => icon.src)).toEqual(
			expect.arrayContaining([
				expect.stringMatching(/^\/pwa\/icon-192\.[0-9a-f]{16}\.png$/),
				expect.stringMatching(/^\/pwa\/icon-512\.[0-9a-f]{16}\.png$/),
				expect.stringMatching(/^\/pwa\/maskable-icon-512\.[0-9a-f]{16}\.png$/),
			]),
		);
		expect(
			manifest.icons?.some((icon) => icon.purpose?.includes("maskable")),
		).toBe(true);
		expect(manifest.screenshots?.map((screenshot) => screenshot.src)).toEqual([
			"/pwa/screenshots/dashboard-warm-skeleton-mobile-shell.png",
			"/pwa/screenshots/app-shell-update-notice.png",
		]);
		expect(
			manifest.screenshots?.map((screenshot) => screenshot.form_factor),
		).toEqual(["narrow", "wide"]);
		expect(manifest.shortcuts?.map((shortcut) => shortcut.url)).toEqual([
			"/",
			"/admin",
			"/settings",
		]);
	} finally {
		await server.close();
	}
});

test("install metadata stays network-revalidated outside the service worker precache", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);

		const cdpSession = await page.context().newCDPSession(page);
		const manifestRequestsBefore = server.getManifestRequests();
		const installIconRequestsBefore = server.getInstallIconRequests();
		const metadata = await readInstallMetadata(page, cdpSession);
		expect(metadata.icons).toHaveLength(3);
		expect(metadata.icons.every((icon) => icon.byteLength > 0)).toBe(true);

		expect(server.getManifestRequests()).toBeGreaterThan(
			manifestRequestsBefore,
		);
		expect(server.getInstallIconRequests()).toBeGreaterThan(
			installIconRequestsBefore,
		);

		const manifestHeaders = await page.request.get(
			`${server.origin}/manifest.webmanifest`,
		);
		expect(manifestHeaders.headers()["cache-control"]).toBe("no-cache");
		for (const icon of metadata.icons) {
			const iconHeaders = await page.request.get(`${server.origin}${icon.src}`);
			expect(iconHeaders.headers()["cache-control"]).toBe(
				"public, max-age=31536000, immutable",
			);
		}
	} finally {
		await server.close();
	}
});

test("same Chromium browser context retrieves V2 install metadata after a V1 release update", async ({
	page,
}) => {
	test.setTimeout(120_000);
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);
		const cdpSession = await page.context().newCDPSession(page);

		const v1 = await readInstallMetadata(page, cdpSession);
		const v1ManifestRequests = server.getManifestRequests();
		const v1BrowserManifestRequests = server.getBrowserManifestRequests();
		const v1InstallIconRequests = server.getInstallIconRequests();
		const v1ServiceWorkerRequests = server.getServiceWorkerRequests();
		expect(v1).toMatchObject({ id: "/", startUrl: "/", scope: "/" });
		expect(v1.icons).toHaveLength(3);
		expect(v1.icons.every((icon) => icon.byteLength > 0)).toBe(true);
		const v1IconSrcs = v1.icons.map((icon) => icon.src);

		server.setPwaRelease("v2");
		server.setApiVersion("0.2.0");
		server.setServiceWorkerRevision(2);
		await page.reload({ waitUntil: "domcontentloaded" });
		await expect(page.locator("body")).toBeVisible();
		await expect
			.poll(() => server.getServiceWorkerRequests())
			.toBeGreaterThan(v1ServiceWorkerRequests);

		const v2 = await readInstallMetadata(page, cdpSession);
		expect(v2).toMatchObject({ id: "/", startUrl: "/", scope: "/" });
		expect(v2.icons).toHaveLength(3);
		expect(v2.icons.every((icon) => icon.byteLength > 0)).toBe(true);
		expect(v2.icons.map((icon) => icon.src)).not.toEqual(v1IconSrcs);
		expect(v2.icons.map((icon) => icon.src)).toEqual(
			expect.arrayContaining([
				expect.stringMatching(/^\/pwa\/icon-192\.[0-9a-f]{16}\.png$/),
				expect.stringMatching(/^\/pwa\/icon-512\.[0-9a-f]{16}\.png$/),
				expect.stringMatching(/^\/pwa\/maskable-icon-512\.[0-9a-f]{16}\.png$/),
			]),
		);
		expect(
			v2.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))?.src,
		).not.toBe(
			v1.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))?.src,
		);
		expect(
			v2.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))
				?.byteLength,
		).not.toBe(
			v1.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))
				?.byteLength,
		);
		expect(server.getManifestRequests()).toBeGreaterThan(v1ManifestRequests);
		expect(server.getBrowserManifestRequests()).toBeGreaterThan(
			v1BrowserManifestRequests,
		);
		expect(server.getInstallIconRequests()).toBeGreaterThan(
			v1InstallIconRequests,
		);
		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"检测到新版本",
		);
	} finally {
		await server.close();
	}
});

test("real installed Chromium PWA retrieves V2 install metadata without reinstall", async ({
	context,
	page,
}) => {
	test.skip(
		process.env.OCTORILL_REAL_PWA_TEST !== "1",
		"requires a ChromeOS runner with the DevTools PWA handler",
	);
	test.setTimeout(120_000);
	const server = await startStaticPwaServer();
	let managementSession: CDPSession | undefined;
	let appPage: Page | undefined;
	let manifestId: string | undefined;
	try {
		await page.goto(server.origin);
		managementSession = await context.newCDPSession(page);
		const appManifest = (await managementSession.send(
			"Page.getAppManifest",
		)) as {
			manifest?: { id?: string };
		};
		manifestId = appManifest.manifest?.id;
		if (!manifestId) throw new Error("browser manifest is missing its id");

		await managementSession.send("PWA.install", { manifestId });
		const launchedPage = context.waitForEvent("page", { timeout: 30_000 });
		await managementSession.send("PWA.launch", { manifestId });
		appPage = await launchedPage;
		await appPage.waitForURL(`${server.origin}/**`);
		await appPage.bringToFront();
		await expect
			.poll(() =>
				appPage?.evaluate(
					() => window.matchMedia("(display-mode: standalone)").matches,
				),
			)
			.toBe(true);
		await waitForServiceWorkerControl(appPage);
		const appSession = await context.newCDPSession(appPage);

		const v1 = await readInstallMetadata(appPage, appSession);
		const v1BrowserManifestRequests = server.getBrowserManifestRequests();
		const v1IconSrcs = v1.icons.map((icon) => icon.src);
		const v1ServiceWorkerRequests = server.getServiceWorkerRequests();
		expect(v1).toMatchObject({ id: "/", startUrl: "/", scope: "/" });
		expect(v1.icons).toHaveLength(3);

		server.setPwaRelease("v2");
		server.setApiVersion("0.2.0");
		server.setServiceWorkerRevision(2);
		await appPage.reload({ waitUntil: "domcontentloaded" });
		await expect
			.poll(() => server.getServiceWorkerRequests())
			.toBeGreaterThan(v1ServiceWorkerRequests);

		const v2 = await readInstallMetadata(appPage, appSession);
		expect(v2).toMatchObject({ id: "/", startUrl: "/", scope: "/" });
		expect(v2.icons).toHaveLength(3);
		expect(v2.icons.map((icon) => icon.src)).not.toEqual(v1IconSrcs);
		expect(server.getBrowserManifestRequests()).toBeGreaterThan(
			v1BrowserManifestRequests,
		);
		expect(
			v2.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))?.src,
		).not.toBe(
			v1.icons.find((icon) => icon.src.startsWith("/pwa/icon-192."))?.src,
		);
		await expect(appPage.locator("[data-version-update-notice]")).toContainText(
			"检测到新版本",
		);
	} finally {
		if (appPage && !appPage.isClosed()) await appPage.close();
		if (managementSession && manifestId) {
			try {
				await managementSession.send("PWA.uninstall", { manifestId });
			} catch {
				// Dedicated PWA runners may clean the temporary profile themselves.
			}
		}
		await server.close();
	}
});

test("production service worker falls back to cached app shell while bypassing private network paths", async ({
	context,
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);

		await context.setOffline(true);
		await page.goto(`${server.origin}/settings`, {
			waitUntil: "domcontentloaded",
		});
		await expect(page).toHaveTitle("OctoRill");

		const privateRequests = await page.evaluate(async () => {
			const results: Record<string, string> = {};
			for (const [key, input, init] of [
				["api", "/api/version", undefined],
				["auth", "/auth/logout", undefined],
				["authDeepLink", "/auth/github/callback?code=test", undefined],
				["post", "/settings", { method: "POST" }],
			] as const) {
				try {
					await fetch(input, init);
					results[key] = "resolved";
				} catch {
					results[key] = "rejected";
				}
			}
			return results;
		});

		expect(privateRequests).toEqual({
			api: "rejected",
			auth: "rejected",
			authDeepLink: "rejected",
			post: "rejected",
		});
	} finally {
		await context.setOffline(false);
		await server.close();
	}
});

test("offline anonymous boot shows a network boundary instead of an auth failure", async ({
	context,
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);

		await context.setOffline(true);
		await page.goto(server.origin, { waitUntil: "domcontentloaded" });

		await expect(page.getByText("网络连接不可用")).toBeVisible();
		await expect(
			page.getByText(
				"当前处于离线状态，登录和最新数据需要网络连接；已保留可用的应用壳。",
			),
		).toBeVisible();
		await expect(
			page.getByRole("button", { name: "使用 Passkey 登录" }),
		).toBeDisabled();
	} finally {
		await context.setOffline(false);
		await server.close();
	}
});

test("offline authenticated boot keeps cached dashboard content with a small banner", async ({
	context,
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);
		await seedAuthenticatedStartupCache(page, {
			tab: "releases",
			feedItems: [cachedReleaseFeedItem()],
		});

		await context.setOffline(true);
		await page.goto(`${server.origin}/releases`, {
			waitUntil: "domcontentloaded",
		});

		await expect(
			page.locator("[data-dashboard-offline-cache-banner]"),
		).toBeVisible();
		await expect(page.getByText("正在显示缓存内容")).toBeVisible();
		await expect(page.getByText("octo-rill/app-shell")).toBeVisible();
		await expect(
			page.locator("[data-dashboard-offline-empty-state]"),
		).toHaveCount(0);

		const requestsBeforeRetry = server.getApiMeRequests();
		await context.setOffline(false);
		await page.getByRole("button", { name: "重试连接" }).click();
		await expect
			.poll(() => server.getApiMeRequests())
			.toBeGreaterThan(requestsBeforeRetry);
	} finally {
		await context.setOffline(false);
		await server.close();
	}
});

test("offline authenticated boot shows an offline empty state when the active page has no cache", async ({
	context,
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);
		await seedAuthenticatedStartupCache(page, {});

		await context.setOffline(true);
		await page.goto(`${server.origin}/stars`, {
			waitUntil: "domcontentloaded",
		});

		await expect(
			page.locator("[data-dashboard-offline-empty-state]"),
		).toBeVisible();
		await expect(page.getByText("离线时没有可用缓存")).toBeVisible();
		await expect(
			page.getByText("当前页面之前没有保存到本地的内容"),
		).toBeVisible();
		await expect(
			page.locator("[data-dashboard-offline-cache-banner]"),
		).toHaveCount(0);
	} finally {
		await context.setOffline(false);
		await server.close();
	}
});

test("version drift checks for a waiting service worker and activates only after refresh", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);
		const initialServiceWorkerRequests = server.getServiceWorkerRequests();

		server.setApiVersion("0.2.0");
		server.setServiceWorkerRevision(2);
		await page.evaluate(() => {
			document.dispatchEvent(new Event("visibilitychange"));
		});

		await expect
			.poll(() => server.getServiceWorkerRequests())
			.toBeGreaterThan(initialServiceWorkerRequests);
		await expect
			.poll(async () =>
				page.evaluate(async () => {
					const registration = await navigator.serviceWorker.ready;
					return registration.waiting !== null;
				}),
			)
			.toBe(true);
		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"检测到新版本",
		);
		await expect.poll(() => server.getSkipWaitingMessages()).toBe(0);

		await page.getByRole("button", { name: "刷新" }).click();
		await expect
			.poll(() => server.getSkipWaitingMessages())
			.toBeGreaterThanOrEqual(1);
	} finally {
		await server.close();
	}
});

test("version drift before service worker registration still triggers an update check", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);
		const initialServiceWorkerRequests = server.getServiceWorkerRequests();

		server.setApiVersion("0.2.0");
		server.setServiceWorkerRevision(2);
		await page.reload({ waitUntil: "domcontentloaded" });

		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"检测到新版本",
		);
		await expect
			.poll(() => server.getServiceWorkerRequests())
			.toBeGreaterThan(initialServiceWorkerRequests);
		await expect
			.poll(async () =>
				page.evaluate(async () => {
					const registration = await navigator.serviceWorker.ready;
					return registration.waiting !== null;
				}),
			)
			.toBe(true);
		await expect.poll(() => server.getSkipWaitingMessages()).toBe(0);
	} finally {
		await server.close();
	}
});

test("install prompt appears when beforeinstallprompt fires and hides after appinstalled", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);

		await dispatchBeforeInstallPrompt(page);
		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"可安装为独立应用",
		);
		await expect(page.getByRole("button", { name: "安装" })).toBeVisible();

		await dispatchAppInstalled(page);
		await expect(page.getByRole("button", { name: "安装" })).toHaveCount(0);
	} finally {
		await server.close();
	}
});

test("install prompt click calls the native prompt and still coexists with refresh update actions", async ({
	page,
}) => {
	const server = await startStaticPwaServer();
	try {
		await page.goto(server.origin);
		await waitForServiceWorkerControl(page);

		server.setServiceWorkerRevision(2);
		await page.evaluate(async () => {
			const registration = await navigator.serviceWorker.ready;
			await registration.update();
		});
		await dispatchBeforeInstallPrompt(page);

		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"也可安装为独立应用",
		);
		await expect(page.getByRole("button", { name: "刷新" })).toBeVisible();
		await expect(page.getByRole("button", { name: "安装" })).toBeVisible();

		await page.getByRole("button", { name: "安装" }).click();
		await expect
			.poll(async () =>
				page.evaluate(() => {
					const state = (
						window as Window & {
							__octoRillInstallPromptState?: { promptCalls: number };
						}
					).__octoRillInstallPromptState;
					return state?.promptCalls ?? 0;
				}),
			)
			.toBe(1);

		await page.getByRole("button", { name: "刷新" }).click();
		await expect
			.poll(() => server.getSkipWaitingMessages())
			.toBeGreaterThanOrEqual(1);
	} finally {
		await server.close();
	}
});
