import { createRouter } from "@tanstack/react-router";

import { routeTree } from "./routeTree.gen";

function parseSearch(searchStr: string) {
	const params = new URLSearchParams(
		searchStr.startsWith("?") ? searchStr.slice(1) : searchStr,
	);
	const result: Record<string, string> = {};

	for (const [key, value] of params.entries()) {
		result[key] = value;
	}

	return result;
}

function stringifySearch(search: Record<string, unknown>) {
	const params = new URLSearchParams();

	for (const [key, value] of Object.entries(search)) {
		if (value === undefined || value === null || value === "") continue;
		if (Array.isArray(value)) {
			for (const item of value) {
				if (item === undefined || item === null || item === "") continue;
				params.append(key, String(item));
			}
			continue;
		}
		params.set(key, String(value));
	}

	const serialized = params.toString();
	return serialized ? `?${serialized}` : "";
}

export const router = createRouter({
	routeTree,
	parseSearch,
	stringifySearch,
	scrollRestoration: true,
	defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
	interface Register {
		router: typeof router;
	}
}
