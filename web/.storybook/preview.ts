import type { Preview } from "@storybook/react-vite";
import { createElement } from "react";
import { TooltipProvider } from "@/components/ui/tooltip";
import "../src/index.css";

const STORYBOOK_HEALTH_VERSION = "0.1.0";

declare global {
	var __octoRillStorybookFetchPatched: boolean | undefined;
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
