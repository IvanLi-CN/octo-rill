import { createRouter } from "@tanstack/react-router";

import {
	buildCurrentDemoSearchObject,
	preserveCurrentDemoSearchInHref,
} from "@/demo/registry";
import { getDemoRouterBasepath } from "@/demo/runtime";
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

const routerInstance = createRouter({
	routeTree,
	basepath: getDemoRouterBasepath(),
	parseSearch,
	stringifySearch,
	scrollRestoration: true,
	defaultPreload: "intent",
});

const rawNavigate = routerInstance.navigate.bind(routerInstance);

routerInstance.navigate = ((options: Record<string, unknown>) => {
	if (typeof window === "undefined") {
		return rawNavigate(options as never);
	}

	const nextOptions = { ...options };
	const currentDemoSearch = buildCurrentDemoSearchObject();
	if (typeof nextOptions.href === "string") {
		nextOptions.href = preserveCurrentDemoSearchInHref(nextOptions.href);
	} else if (
		currentDemoSearch &&
		(!nextOptions.search ||
			(typeof nextOptions.search === "object" &&
				!Array.isArray(nextOptions.search)))
	) {
		nextOptions.search = {
			...currentDemoSearch,
			...(typeof nextOptions.search === "object" && nextOptions.search
				? (nextOptions.search as Record<string, unknown>)
				: {}),
		};
	}

	return rawNavigate(nextOptions as never);
}) as typeof routerInstance.navigate;

export const router = routerInstance;

declare module "@tanstack/react-router" {
	interface Register {
		router: typeof router;
	}
}
