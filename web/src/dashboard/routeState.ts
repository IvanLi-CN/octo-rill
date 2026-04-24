import type { ReleaseDetailResponse } from "@/api";
import { normalizeReleaseId } from "@/lib/releaseId";
import type { DashboardTab } from "@/pages/DashboardControlBand";

export type DashboardReleaseLocator = {
	owner: string;
	repo: string;
	tag: string;
};

export type DashboardReleaseTarget = {
	releaseId: string | null;
	locator: DashboardReleaseLocator | null;
	fromTab: DashboardTab;
};

export type DashboardRouteState = {
	tab: DashboardTab;
	activeReleaseId: string | null;
	activeReleaseLocator: DashboardReleaseLocator | null;
	releaseReturnTab: DashboardTab;
};

export type DashboardWarmRouteState = {
	tab: DashboardTab;
	activeReleaseId: string | null;
	activeReleaseLocatorKey: string | null;
	releaseReturnTab: DashboardTab;
};

const DASHBOARD_TAB_PATHS: Record<DashboardTab, string> = {
	all: "/",
	releases: "/releases",
	stars: "/stars",
	followers: "/followers",
	briefs: "/briefs",
	inbox: "/inbox",
};

const DASHBOARD_TAB_ROUTE_KEYS = new Set<DashboardTab>([
	"all",
	"releases",
	"stars",
	"followers",
	"briefs",
	"inbox",
]);

const DASHBOARD_TAB_QUERY_KEYS = new Set<DashboardTab>([
	"all",
	"releases",
	"stars",
	"followers",
	"briefs",
	"inbox",
]);

const DASHBOARD_PATH_TO_TAB = new Map<string, DashboardTab>(
	Object.entries(DASHBOARD_TAB_PATHS).map(([tab, path]) => [
		path,
		tab as DashboardTab,
	]),
);

function normalizePathname(pathname: string | null | undefined) {
	return pathname?.replace(/\/+$/, "") || "/";
}

export function normalizeDashboardTab(
	value: string | null | undefined,
): DashboardTab {
	return DASHBOARD_TAB_ROUTE_KEYS.has(value as DashboardTab)
		? (value as DashboardTab)
		: "all";
}

export function normalizeDashboardReturnTab(
	value: string | null | undefined,
): DashboardTab {
	if (value == null) return "briefs";
	return DASHBOARD_TAB_QUERY_KEYS.has(value as DashboardTab)
		? (value as DashboardTab)
		: "briefs";
}

function decodeSegment(value: string) {
	try {
		return decodeURIComponent(value);
	} catch {
		return value;
	}
}

function encodeSegment(value: string) {
	return encodeURIComponent(value);
}

export function buildDashboardTabPath(tab: DashboardTab) {
	return DASHBOARD_TAB_PATHS[tab];
}

export function buildDashboardReleasePath(locator: DashboardReleaseLocator) {
	return `/${encodeSegment(locator.owner)}/${encodeSegment(locator.repo)}/releases/tag/${encodeSegment(locator.tag)}`;
}

export function buildDashboardReleaseHref(
	locator: DashboardReleaseLocator,
	fromTab: DashboardTab = "briefs",
) {
	const params = new URLSearchParams();
	params.set("from", fromTab);
	const query = params.toString();
	return `${buildDashboardReleasePath(locator)}${query ? `?${query}` : ""}`;
}

export function buildDashboardRouteUrl(routeState: DashboardRouteState) {
	if (routeState.activeReleaseLocator) {
		return buildDashboardReleaseHref(
			routeState.activeReleaseLocator,
			routeState.releaseReturnTab,
		);
	}
	if (routeState.activeReleaseId) {
		const params = new URLSearchParams();
		params.set("tab", routeState.releaseReturnTab);
		params.set("release", routeState.activeReleaseId);
		return `/?${params.toString()}`;
	}
	return buildDashboardTabPath(routeState.tab);
}

export function buildDashboardWarmRouteState(
	routeState: DashboardRouteState,
): DashboardWarmRouteState {
	return {
		tab: routeState.tab,
		activeReleaseId: routeState.activeReleaseId,
		activeReleaseLocatorKey: routeState.activeReleaseLocator
			? buildDashboardReleaseLocatorKey(routeState.activeReleaseLocator)
			: null,
		releaseReturnTab: routeState.releaseReturnTab,
	};
}

export function buildDashboardReleaseLocatorKey(
	locator: DashboardReleaseLocator,
) {
	return `${locator.owner.toLowerCase()}/${locator.repo.toLowerCase()}#${locator.tag}`;
}

function parseDashboardReleasePathname(
	pathname: string,
): DashboardReleaseLocator | null {
	const segments = normalizePathname(pathname).split("/").filter(Boolean);
	if (segments.length !== 5) return null;
	if (segments[2] !== "releases" || segments[3] !== "tag") return null;
	const owner = decodeSegment(segments[0] ?? "").trim();
	const repo = decodeSegment(segments[1] ?? "").trim();
	const tag = decodeSegment(segments[4] ?? "");
	if (!owner || !repo || !tag) return null;
	return { owner, repo, tag };
}

function searchParamsFromInput(
	input:
		| string
		| URLSearchParams
		| {
				tab?: string | null;
				release?: string | null;
				from?: string | null;
		  }
		| null
		| undefined,
) {
	if (input instanceof URLSearchParams) {
		return input;
	}
	if (typeof input === "string") {
		return new URLSearchParams(input.startsWith("?") ? input.slice(1) : input);
	}
	const params = new URLSearchParams();
	if (input?.tab) params.set("tab", input.tab);
	if (input?.release) params.set("release", input.release);
	if (input?.from) params.set("from", input.from);
	return params;
}

export function parseLegacyDashboardRouteState(input: {
	tab?: string | null;
	release?: string | null;
	from?: string | null;
}): DashboardRouteState {
	const releaseId = normalizeReleaseId(input.release);
	if (releaseId) {
		return {
			tab: "briefs",
			activeReleaseId: releaseId,
			activeReleaseLocator: null,
			releaseReturnTab: normalizeDashboardReturnTab(input.tab),
		};
	}

	return {
		tab: normalizeDashboardTab(input.tab),
		activeReleaseId: null,
		activeReleaseLocator: null,
		releaseReturnTab: "briefs",
	};
}

export function parseDashboardRouteState(input: {
	pathname?: string | null;
	search?:
		| string
		| URLSearchParams
		| {
				tab?: string | null;
				release?: string | null;
				from?: string | null;
		  }
		| null;
	tab?: string | null;
	release?: string | null;
	from?: string | null;
	owner?: string | null;
	repo?: string | null;
	tag?: string | null;
}): DashboardRouteState {
	if (input.owner && input.repo && input.tag) {
		const locator = {
			owner: input.owner,
			repo: input.repo,
			tag: input.tag,
		};
		const fromTab = normalizeDashboardReturnTab(input.from);
		return {
			tab: fromTab,
			activeReleaseId: null,
			activeReleaseLocator: locator,
			releaseReturnTab: fromTab,
		};
	}

	const pathname = normalizePathname(input.pathname ?? "/");
	const searchParams = searchParamsFromInput(
		input.search ?? {
			tab: input.tab,
			release: input.release,
			from: input.from,
		},
	);
	const releaseLocator = parseDashboardReleasePathname(pathname);
	if (releaseLocator) {
		const fromTab = normalizeDashboardReturnTab(searchParams.get("from"));
		return {
			tab: fromTab,
			activeReleaseId: null,
			activeReleaseLocator: releaseLocator,
			releaseReturnTab: fromTab,
		};
	}

	if (pathname === "/") {
		return parseLegacyDashboardRouteState({
			tab: searchParams.get("tab"),
			release: searchParams.get("release"),
			from: searchParams.get("from"),
		});
	}

	const tab = DASHBOARD_PATH_TO_TAB.get(pathname) ?? "all";
	return {
		tab,
		activeReleaseId: null,
		activeReleaseLocator: null,
		releaseReturnTab: "briefs",
	};
}

export function parseDashboardRouteStateFromLocation(
	pathname: string,
	search: string | URLSearchParams,
) {
	return parseDashboardRouteState({ pathname, search });
}

export function validateDashboardSearch(search: Record<string, unknown>) {
	return {
		tab: typeof search.tab === "string" ? search.tab : undefined,
		release: typeof search.release === "string" ? search.release : undefined,
		from: typeof search.from === "string" ? search.from : undefined,
	};
}

export function routeStateHasActiveRelease(routeState: DashboardRouteState) {
	return Boolean(routeState.activeReleaseId || routeState.activeReleaseLocator);
}

export function isDashboardPathname(pathname: string) {
	const normalized = normalizePathname(pathname);
	return (
		normalized === "/" ||
		DASHBOARD_PATH_TO_TAB.has(normalized) ||
		parseDashboardReleasePathname(normalized) !== null
	);
}

export function releaseLocatorFromHtmlUrl(
	htmlUrl: string | null | undefined,
): DashboardReleaseLocator | null {
	if (!htmlUrl) return null;
	try {
		const url = new URL(htmlUrl);
		if (url.hostname !== "github.com" && url.hostname !== "www.github.com") {
			return null;
		}
		return parseDashboardReleasePathname(url.pathname);
	} catch {
		return null;
	}
}

export function releaseLocatorFromReleaseDetail(
	detail: Pick<
		ReleaseDetailResponse,
		"repo_full_name" | "tag_name" | "html_url"
	>,
): DashboardReleaseLocator | null {
	const fromHtmlUrl = releaseLocatorFromHtmlUrl(detail.html_url);
	if (fromHtmlUrl) return fromHtmlUrl;
	if (!detail.repo_full_name || !detail.tag_name) return null;
	const [owner, repo] = detail.repo_full_name.split("/", 2);
	if (!owner || !repo) return null;
	return {
		owner,
		repo,
		tag: detail.tag_name,
	};
}

export function buildDashboardReleaseTarget(input: {
	releaseId?: string | null;
	locator?: DashboardReleaseLocator | null;
	fromTab?: DashboardTab | null;
}): DashboardReleaseTarget {
	return {
		releaseId: normalizeReleaseId(input.releaseId) ?? null,
		locator: input.locator ?? null,
		fromTab: input.fromTab ?? "briefs",
	};
}

export function parseInternalDashboardReleaseTarget(
	href: string | undefined,
): DashboardReleaseTarget | null {
	if (!href) return null;
	try {
		const url = new URL(href, window.location.origin);
		if (url.origin !== window.location.origin) return null;
		const locator = parseDashboardReleasePathname(url.pathname);
		if (locator) {
			return {
				releaseId: null,
				locator,
				fromTab: normalizeDashboardReturnTab(url.searchParams.get("from")),
			};
		}
		if (normalizePathname(url.pathname) !== "/") return null;
		const releaseId = normalizeReleaseId(url.searchParams.get("release"));
		if (!releaseId) return null;
		return {
			releaseId,
			locator: null,
			fromTab: normalizeDashboardReturnTab(url.searchParams.get("tab")),
		};
	} catch {
		return null;
	}
}
