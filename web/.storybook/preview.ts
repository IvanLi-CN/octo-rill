import type { Preview } from "@storybook/react-vite";
import { createElement } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
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
	"/storybook-guide.html",
]);

declare global {
	var __octoRillStorybookFetchPatched: boolean | undefined;
	var __octoRillStorybookLinkGuardInstalled: boolean | undefined;
	var __octoRillStorybookDocsLinkSyncInstalled: boolean | undefined;
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

function resolveLocalDocsSiteLink(url: URL): URL | null {
	if (!import.meta.env.DEV || url.origin !== window.location.origin) {
		return null;
	}
	if (!LOCAL_DOCS_SITE_PATHS.has(url.pathname)) return null;
	return new URL(
		`${url.pathname}${url.search}${url.hash}`,
		getLocalDocsSiteOrigin(),
	);
}

function rewriteLocalDocsLinks(root: Document | ShadowRoot): void {
	for (const anchor of root.querySelectorAll<HTMLAnchorElement>("a[href]")) {
		const rewrittenTarget = resolveLocalDocsSiteLink(
			new URL(anchor.href, window.location.origin),
		);
		if (!rewrittenTarget) continue;
		anchor.href = rewrittenTarget.toString();
		anchor.target = "_top";
		anchor.rel = anchor.rel
			? `${anchor.rel} noopener noreferrer`.trim()
			: "noopener noreferrer";
	}
}

function installLocalDocsLinkSync(): void {
	if (
		typeof document === "undefined" ||
		globalThis.__octoRillStorybookDocsLinkSyncInstalled ||
		!import.meta.env.DEV
	) {
		return;
	}

	const observeDocument = (targetDocument: Document) => {
		const sync = () => rewriteLocalDocsLinks(targetDocument);
		sync();
		new MutationObserver(sync).observe(targetDocument, {
			childList: true,
			subtree: true,
		});
	};

	observeDocument(document);

	const syncPreviewIframe = () => {
		const previewIframe = document.querySelector<HTMLIFrameElement>(
			"#storybook-preview-iframe",
		);
		if (!previewIframe) return;
		const previewDocument = previewIframe.contentDocument;
		if (!previewDocument) return;
		observeDocument(previewDocument);
	};

	document.addEventListener(
		"DOMContentLoaded",
		() => {
			syncPreviewIframe();
			const previewIframe = document.querySelector<HTMLIFrameElement>(
				"#storybook-preview-iframe",
			);
			previewIframe?.addEventListener("load", syncPreviewIframe);
		},
		{ once: true },
	);
	syncPreviewIframe();

	globalThis.__octoRillStorybookDocsLinkSyncInstalled = true;
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
			if (!isStorybookAppLink(targetUrl)) return;

			event.preventDefault();
		},
		true,
	);

	globalThis.__octoRillStorybookLinkGuardInstalled = true;
}

installLocalDocsLinkSync();

const preview: Preview = {
	tags: ["autodocs"],
	decorators: [
		(Story) => {
			if (typeof document !== "undefined") {
				document.body.style.backgroundImage = "none";
				document.body.style.backgroundColor = "#f7f4ed";
				installLocalDocsLinkSync();
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
