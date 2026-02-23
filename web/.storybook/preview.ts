import type { Preview } from "@storybook/react-vite";
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
	parameters: {
		actions: { argTypesRegex: "^on[A-Z].*" },
		controls: {
			matchers: {
				color: /(background|color)$/i,
				date: /Date$/i,
			},
		},
	},
};

export default preview;
