import { execFileSync } from "node:child_process";
import { readFile } from "node:fs/promises";
import {
	createServer,
	type IncomingMessage,
	type ServerResponse,
} from "node:http";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";

test.describe.configure({ mode: "serial" });

test.beforeAll(() => {
	execFileSync("bun", ["run", "build"], {
		cwd: path.resolve(import.meta.dirname, ".."),
		stdio: "inherit",
	});
});

async function installAnonymousApiMocks(page: Page) {
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());

		if (req.method() === "GET" && url.pathname === "/api/me") {
			return route.fulfill({
				status: 401,
				contentType: "application/json",
				body: JSON.stringify({
					error: {
						code: "unauthorized",
						message: "unauthorized",
					},
				}),
			});
		}

		if (req.method() === "GET" && url.pathname === "/api/version") {
			return route.fulfill({
				status: 200,
				contentType: "application/json",
				body: JSON.stringify({
					ok: true,
					version: "0.1.0",
					source: "APP_EFFECTIVE_VERSION",
				}),
			});
		}

		return route.fulfill({
			status: 404,
			contentType: "application/json",
			body: JSON.stringify({
				error: {
					code: "not_found",
					message: `unhandled ${req.method()} ${url.pathname}`,
				},
			}),
		});
	});
}

type StaticPwaServer = {
	origin: string;
	setApiVersion: (version: string) => void;
	setServiceWorkerRevision: (revision: number) => void;
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

async function startStaticPwaServer(): Promise<StaticPwaServer> {
	let apiVersion = "0.1.0";
	let serviceWorkerRevision = 1;
	let serviceWorkerRequests = 0;
	let skipWaitingMessages = 0;

	const server = createServer(
		async (request: IncomingMessage, response: ServerResponse) => {
			const requestUrl = new URL(request.url ?? "/", "http://127.0.0.1");
			if (requestUrl.pathname === "/api/me") {
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

			const filePath = resolveDistPath(requestUrl.pathname);
			try {
				let body = await readFile(filePath);
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
					"cache-control": requestUrl.pathname.startsWith("/assets/")
						? "public, max-age=31536000, immutable"
						: "no-cache",
					"content-type":
						contentTypes.get(path.extname(filePath)) ??
						"application/octet-stream",
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

test("app exposes installable PWA metadata without blocking anonymous login", async ({
	page,
}) => {
	await installAnonymousApiMocks(page);
	await page.goto("/");

	await expect(page.locator('link[rel="manifest"]')).toHaveAttribute(
		"href",
		"/manifest.webmanifest",
	);
	await expect(page.locator('link[rel="apple-touch-icon"]')).toHaveAttribute(
		"href",
		"/pwa/apple-touch-icon.png",
	);
	await expect(page.locator('meta[name="theme-color"]')).toHaveAttribute(
		"content",
		"#0f172a",
	);
	await expect(
		page.getByRole("link", { name: "使用 GitHub 登录" }),
	).toBeVisible();

	const manifestResponse = await page.request.get("/manifest.webmanifest");
	expect(manifestResponse.ok()).toBe(true);
	const manifest = (await manifestResponse.json()) as {
		id?: string;
		name?: string;
		display?: string;
		icons?: Array<{ src?: string; purpose?: string }>;
		screenshots?: Array<{ src?: string; form_factor?: string }>;
		shortcuts?: Array<{ name?: string; url?: string }>;
	};
	expect(manifest.id).toBe("/");
	expect(manifest.name).toBe("OctoRill");
	expect(manifest.display).toBe("standalone");
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
