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
	setServiceWorkerRevision: (revision: number) => void;
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
	let serviceWorkerRevision = 1;
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
					version: "0.1.0",
					source: "APP_EFFECTIVE_VERSION",
				});
				return;
			}
			if (requestUrl.pathname === "/api/health") {
				writeJson(response, 200, { ok: true, version: "0.1.0" });
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
		setServiceWorkerRevision(revision: number) {
			serviceWorkerRevision = revision;
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
		name?: string;
		display?: string;
		icons?: Array<{ src?: string; purpose?: string }>;
	};
	expect(manifest.name).toBe("OctoRill");
	expect(manifest.display).toBe("standalone");
	expect(
		manifest.icons?.some((icon) => icon.purpose?.includes("maskable")),
	).toBe(true);
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
			post: "rejected",
		});
	} finally {
		await context.setOffline(false);
		await server.close();
	}
});

test("waiting service worker uses the existing update notice and activates only after refresh", async ({
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

		await expect(page.locator("[data-version-update-notice]")).toContainText(
			"检测到新前端版本",
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
