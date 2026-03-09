import type { Preview } from "@storybook/react-vite";
import { createElement } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import "../src/index.css";

const STORYBOOK_HEALTH_VERSION = "0.1.0";
const LOCAL_STORYBOOK_DEV_ORIGINS = new Set([
	"http://127.0.0.1:55176",
	"http://localhost:55176",
]);
const LOCAL_DOCS_SITE_ORIGIN = "http://127.0.0.1:50885";
const LOCAL_DOCS_SITE_PATHS = new Set([
	"/index.html",
	"/quick-start.html",
	"/config.html",
	"/product.html",
	"/storybook.html",
	"/storybook-guide.html",
]);

declare global {
	var __octoRillStorybookFetchPatched: boolean | undefined;
	var __octoRillStorybookLinkGuardInstalled: boolean | undefined;
}

if (
	typeof globalThis.fetch === "function" &&
	!globalThis.__octoRillStorybookFetchPatched
) {
	const originalFetch = globalThis.fetch.bind(globalThis);

	globalThis.fetch = async (input, init) => {
		const rawUrl =
			typeof input === "string"
				? input
				: input instanceof URL
					? input.toString()
					: input.url;
		const resolvedUrl = new URL(rawUrl, window.location.origin);

		if (resolvedUrl.pathname === "/api/health") {
			return new Response(
				JSON.stringify({ ok: true, version: STORYBOOK_HEALTH_VERSION }),
				{
					headers: { "Content-Type": "application/json" },
					status: 200,
				},
			);
		}

		return originalFetch(input, init);
	};

	globalThis.__octoRillStorybookFetchPatched = true;
}

function isStorybookAppLink(url: URL): boolean {
	if (url.origin !== window.location.origin) return false;
	if (url.pathname === "/") return true;
	if (url.pathname === "/admin" || url.pathname.startsWith("/admin/")) {
		return true;
	}
	return (
		url.pathname === "/auth/github/login" || url.pathname === "/auth/logout"
	);
}

function resolveLocalDocsSiteLink(url: URL): URL | null {
	if (!LOCAL_STORYBOOK_DEV_ORIGINS.has(window.location.origin)) {
		return null;
	}
	if (url.origin !== window.location.origin) return null;
	if (!LOCAL_DOCS_SITE_PATHS.has(url.pathname)) return null;
	return new URL(
		`${url.pathname}${url.search}${url.hash}`,
		LOCAL_DOCS_SITE_ORIGIN,
	);
}

if (
	typeof document !== "undefined" &&
	!globalThis.__octoRillStorybookLinkGuardInstalled
) {
	document.addEventListener(
		"click",
		(event) => {
			if (
				event.defaultPrevented ||
				event.button !== 0 ||
				event.metaKey ||
				event.ctrlKey ||
				event.altKey ||
				event.shiftKey
			) {
				return;
			}

			const target = event.target;
			if (!(target instanceof Element)) return;

			const anchor = target.closest("a[href]");
			if (!(anchor instanceof HTMLAnchorElement)) return;

			const targetUrl = new URL(anchor.href, window.location.origin);
			const localDocsSiteTarget = resolveLocalDocsSiteLink(targetUrl);
			if (localDocsSiteTarget) {
				event.preventDefault();
				window.top?.location.assign(localDocsSiteTarget.toString());
				return;
			}
			if (!isStorybookAppLink(targetUrl)) return;

			event.preventDefault();
		},
		true,
	);

	globalThis.__octoRillStorybookLinkGuardInstalled = true;
}

const preview: Preview = {
	tags: ["autodocs"],
	decorators: [
		(Story) => {
			if (typeof document !== "undefined") {
				document.body.style.backgroundImage = "none";
				document.body.style.backgroundColor = "#f7f4ed";
			}
			return createElement(TooltipProvider, null, Story());
		},
	],
	parameters: {
		docs: {
			toc: true,
		},
		actions: { argTypesRegex: "^on[A-Z].*" },
		controls: {
			expanded: true,
			matchers: {
				color: /(background|color)$/i,
				date: /Date$/i,
			},
		},
		options: {
			storySort: {
				order: [
					"Pages",
					["Landing", "Dashboard"],
					"Admin",
					["Admin Panel", "Admin Jobs", "Task Type Detail"],
					"Layout",
					["App Meta Footer"],
					"UI",
					["Primitives", "Button", "Card"],
				],
			},
		},
		backgrounds: {
			default: "solid-paper",
			values: [
				{
					name: "solid-paper",
					value: "#f7f4ed",
				},
			],
		},
	},
};

export default preview;
