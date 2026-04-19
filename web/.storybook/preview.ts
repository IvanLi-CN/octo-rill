import type { Preview } from "@storybook/react-vite";
import { createElement } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ThemeProvider } from "@/theme/ThemeProvider";
import { normalizeThemePreference } from "@/theme/theme";
import "../src/index.css";

const STORYBOOK_HEALTH_VERSION = "0.1.0";
const DEFAULT_LOCAL_DOCS_SITE_ORIGIN = "http://127.0.0.1:50885";
const DOCS_ORIGIN_STORAGE_KEY = "octo-rill.docs-origin";
const LOCAL_DOCS_SITE_PATHS = new Set([
	"/index.html",
	"/quick-start.html",
	"/config.html",
	"/product.html",
	"/storybook.html",
]);

declare global {
	var __octoRillStorybookFetchPatched: boolean | undefined;
	var __octoRillStorybookDocsLinkSyncInstalled: boolean | undefined;
}

const observedDocsRoots = new WeakSet<Document | ShadowRoot>();
const guardedStorybookDocuments = new WeakSet<Document>();

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

function isStorybookAppLink(
	url: URL,
	currentOrigin = window.location.origin,
): boolean {
	if (url.origin !== currentOrigin) return false;
	if (url.pathname === "/") return true;
	if (url.pathname === "/admin" || url.pathname.startsWith("/admin/")) {
		return true;
	}
	return (
		url.pathname === "/settings" ||
		url.pathname === "/auth/github/login" ||
		url.pathname === "/auth/linuxdo/login" ||
		url.pathname === "/auth/logout"
	);
}

function isValidOrigin(rawValue: string | null): rawValue is string {
	if (!rawValue) return false;
	try {
		new URL(rawValue);
		return true;
	} catch {
		return false;
	}
}

function getLocalDocsSiteOrigin(): string {
	if (typeof window === "undefined") return DEFAULT_LOCAL_DOCS_SITE_ORIGIN;

	const docsOriginFromUrl = new URLSearchParams(window.location.search).get(
		"docsOrigin",
	);
	if (isValidOrigin(docsOriginFromUrl)) {
		window.localStorage.setItem(DOCS_ORIGIN_STORAGE_KEY, docsOriginFromUrl);
		return docsOriginFromUrl;
	}

	const docsOriginFromStorage = window.localStorage.getItem(
		DOCS_ORIGIN_STORAGE_KEY,
	);
	if (isValidOrigin(docsOriginFromStorage)) {
		return docsOriginFromStorage;
	}

	return (
		import.meta.env.VITE_DOCS_SITE_ORIGIN || DEFAULT_LOCAL_DOCS_SITE_ORIGIN
	);
}

function isDocsSitePath(pathname: string): boolean {
	for (const candidate of LOCAL_DOCS_SITE_PATHS) {
		if (pathname === candidate || pathname.endsWith(candidate)) {
			return true;
		}
	}
	return false;
}

function resolveDocsSiteLink(url: URL): URL | null {
	if (url.origin !== window.location.origin) {
		return null;
	}
	if (!isDocsSitePath(url.pathname)) return null;
	if (import.meta.env.DEV && LOCAL_DOCS_SITE_PATHS.has(url.pathname)) {
		return new URL(
			`${url.pathname}${url.search}${url.hash}`,
			getLocalDocsSiteOrigin(),
		);
	}
	return url;
}

function rewriteDocsLinks(root: Document | ShadowRoot): void {
	for (const anchor of root.querySelectorAll<HTMLAnchorElement>("a[href]")) {
		const rewrittenTarget = resolveDocsSiteLink(
			new URL(anchor.href, window.location.origin),
		);
		if (!rewrittenTarget) continue;
		anchor.href = rewrittenTarget.toString();
		anchor.target = "_top";
		const relParts = new Set(anchor.rel.split(/\s+/).filter(Boolean));
		relParts.add("noopener");
		relParts.add("noreferrer");
		anchor.rel = Array.from(relParts).join(" ");
	}
}

function installStorybookAppLinkGuard(targetDocument: Document): void {
	if (guardedStorybookDocuments.has(targetDocument)) return;

	targetDocument.addEventListener(
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

			const targetUrl = new URL(
				anchor.href,
				targetDocument.location?.origin ?? window.location.origin,
			);
			if (!isStorybookAppLink(targetUrl, targetDocument.location.origin)) {
				return;
			}

			event.preventDefault();
		},
		true,
	);

	guardedStorybookDocuments.add(targetDocument);
}

function observeStorybookDocument(targetDocument: Document): void {
	if (observedDocsRoots.has(targetDocument)) {
		rewriteDocsLinks(targetDocument);
		installStorybookAppLinkGuard(targetDocument);
		return;
	}

	const sync = () => rewriteDocsLinks(targetDocument);
	sync();
	installStorybookAppLinkGuard(targetDocument);
	new MutationObserver(sync).observe(targetDocument, {
		childList: true,
		subtree: true,
	});
	observedDocsRoots.add(targetDocument);
}

function installDocsLinkSync(): void {
	if (
		typeof document === "undefined" ||
		globalThis.__octoRillStorybookDocsLinkSyncInstalled
	) {
		return;
	}

	observeStorybookDocument(document);

	const syncPreviewIframe = () => {
		const previewIframe = document.querySelector<HTMLIFrameElement>(
			"#storybook-preview-iframe",
		);
		if (!previewIframe) return;
		const previewDocument = previewIframe.contentDocument;
		if (previewDocument) {
			observeStorybookDocument(previewDocument);
		}
		if (previewIframe.dataset.octoRillDocsSyncBound !== "true") {
			previewIframe.addEventListener("load", syncPreviewIframe);
			previewIframe.dataset.octoRillDocsSyncBound = "true";
		}
	};

	if (document.readyState === "loading") {
		document.addEventListener(
			"DOMContentLoaded",
			() => {
				syncPreviewIframe();
			},
			{ once: true },
		);
	}

	syncPreviewIframe();
	globalThis.__octoRillStorybookDocsLinkSyncInstalled = true;
}

installDocsLinkSync();

const preview: Preview = {
	tags: ["autodocs"],
	globalTypes: {
		theme: {
			description: "Global theme for previewed components and pages",
			toolbar: {
				title: "Theme",
				icon: "mirror",
				items: [
					{ value: "light", title: "Light" },
					{ value: "dark", title: "Dark" },
					{ value: "system", title: "System" },
				],
				dynamicTitle: true,
			},
		},
	},
	initialGlobals: {
		theme: "light",
	},
	decorators: [
		(Story, context) => {
			const previewTheme = normalizeThemePreference(
				String(context.globals.theme ?? "light"),
			);
			if (typeof document !== "undefined") {
				document.body.style.backgroundImage = "none";
				document.body.style.backgroundColor = "var(--background)";
				installDocsLinkSync();
			}
			return createElement(
				ThemeProvider,
				{
					defaultPreference: previewTheme,
					key: `storybook-theme-${previewTheme}`,
					persist: false,
				},
				createElement(TooltipProvider, null, Story()),
			);
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
				{
					name: "night-ink",
					value: "#0f172a",
				},
			],
		},
	},
};

export default preview;
