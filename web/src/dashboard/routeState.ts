import type { ReleaseDetailResponse } from "@/api";
import { normalizeReleaseId } from "@/lib/releaseId";
import type { DashboardTab } from "@/pages/DashboardControlBand";

export type DashboardReleaseLocator = {
	owner: string;
	repo: string;
	tag: string;
};

export type DashboardScope =
	| {
			kind: "repo";
			owner: string;
			repo: string;
	  }
	| {
			kind: "repos";
			items: string[];
	  }
	| {
			kind: "org";
			org: string;
	  }
	| {
			kind: "mine";
	  };

export type DashboardScopedTab = Extract<DashboardTab, "all" | "releases">;

export type DashboardReleaseTarget = {
	releaseId: string | null;
	locator: DashboardReleaseLocator | null;
	fromTab: DashboardTab;
	scope?: DashboardScope | null;
};

export type DashboardRouteState = {
	tab: DashboardTab;
	scope: DashboardScope | null;
	activeReleaseId: string | null;
	activeReleaseLocator: DashboardReleaseLocator | null;
	releaseReturnTab: DashboardTab;
};

export type DashboardWarmRouteState = {
	tab: DashboardTab;
	scopeSignature: string | null;
	activeReleaseId: string | null;
	activeReleaseLocatorKey: string | null;
	releaseReturnTab: DashboardTab;
};

export type DashboardRouteNavigation = {
	to: string;
	params?: Record<string, string>;
	search?: Record<string, string>;
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

function isScopedOnlyTab(tab: DashboardTab) {
	return tab === "all" || tab === "releases";
}

function normalizeRepoNamePart(value: string | null | undefined) {
	return value?.trim().replace(/^\/+|\/+$/g, "") ?? "";
}

function normalizeRepoFullName(value: string | null | undefined) {
	const [ownerRaw = "", repoRaw = ""] = (value ?? "").split("/", 2);
	const owner = normalizeRepoNamePart(ownerRaw);
	const repo = normalizeRepoNamePart(repoRaw);
	if (!owner || !repo) return null;
	return {
		owner,
		repo,
		fullName: `${owner}/${repo}`,
	};
}

function normalizeRepoItems(
	value: string | string[] | null | undefined,
): string[] {
	const rawItems = Array.isArray(value)
		? value
		: (value ?? "")
				.split(",")
				.map((item) => item.trim())
				.filter(Boolean);
	const seen = new Set<string>();
	const items: string[] = [];
	for (const raw of rawItems) {
		const normalized = normalizeRepoFullName(raw);
		if (!normalized) continue;
		const key = normalized.fullName.toLowerCase();
		if (seen.has(key)) continue;
		seen.add(key);
		items.push(normalized.fullName);
	}
	return items;
}

function normalizeScopeOrg(value: string | null | undefined) {
	const org = normalizeRepoNamePart(value);
	return org || null;
}

function encodeSegment(value: string) {
	return encodeURIComponent(value);
}

function decodeSegment(value: string) {
	try {
		return decodeURIComponent(value);
	} catch {
		return value;
	}
}

function scopeDefaultTab(scope: DashboardScope | null) {
	return scope ? "all" : "briefs";
}

export function normalizeDashboardTab(
	value: string | null | undefined,
	scope: DashboardScope | null = null,
): DashboardTab {
	const normalized = DASHBOARD_TAB_ROUTE_KEYS.has(value as DashboardTab)
		? (value as DashboardTab)
		: "all";
	if (!scope) return normalized;
	return isScopedOnlyTab(normalized) ? normalized : "all";
}

export function normalizeDashboardReturnTab(
	value: string | null | undefined,
	scope: DashboardScope | null = null,
): DashboardTab {
	const fallback = scopeDefaultTab(scope);
	if (value == null) return fallback;
	const normalized = DASHBOARD_TAB_QUERY_KEYS.has(value as DashboardTab)
		? (value as DashboardTab)
		: fallback;
	if (!scope) return normalized;
	return isScopedOnlyTab(normalized) ? normalized : fallback;
}

export function isScopedDashboardRouteState(routeState: DashboardRouteState) {
	return routeState.scope !== null;
}

export function buildDashboardScopeSignature(scope: DashboardScope | null) {
	if (!scope) return null;
	switch (scope.kind) {
		case "repo":
			return `repo:${scope.owner.toLowerCase()}/${scope.repo.toLowerCase()}`;
		case "repos":
			return `repos:${scope.items.map((item) => item.toLowerCase()).join(",")}`;
		case "org":
			return `org:${scope.org.toLowerCase()}`;
		case "mine":
			return "mine";
	}
}

export function buildDashboardTabPath(tab: DashboardTab) {
	return DASHBOARD_TAB_PATHS[tab];
}

export function buildDashboardReleasePath(locator: DashboardReleaseLocator) {
	return `/${encodeSegment(locator.owner)}/${encodeSegment(locator.repo)}/releases/tag/${encodeSegment(locator.tag)}`;
}

function buildDashboardScopeQueryParams(scope: DashboardScope | null) {
	const params = new URLSearchParams();
	if (!scope) return params;
	params.set("scope", scope.kind);
	switch (scope.kind) {
		case "repo":
			params.set("items", `${scope.owner}/${scope.repo}`);
			break;
		case "repos":
			if (scope.items.length > 0) {
				params.set("items", scope.items.join(","));
			}
			break;
		case "org":
			params.set("org", scope.org);
			break;
		default:
			break;
	}
	return params;
}

function buildDashboardScopePageQueryParams(scope: DashboardScope | null) {
	const params = new URLSearchParams();
	if (!scope) return params;
	if (scope.kind === "repos" && scope.items.length > 0) {
		params.set("items", scope.items.join(","));
	}
	return params;
}

export function buildDashboardScopePath(
	scope: DashboardScope,
	tab: DashboardScopedTab = "all",
) {
	switch (scope.kind) {
		case "repo": {
			const base = `/focus/repo/${encodeSegment(scope.owner)}/${encodeSegment(scope.repo)}`;
			return tab === "releases" ? `${base}/releases` : base;
		}
		case "repos":
			return tab === "releases" ? "/focus/repos/releases" : "/focus/repos";
		case "org": {
			const base = `/focus/org/${encodeSegment(scope.org)}`;
			return tab === "releases" ? `${base}/releases` : base;
		}
		case "mine":
			return tab === "releases" ? "/focus/mine/releases" : "/focus/mine";
	}
}

export function buildDashboardScopeHref(
	scope: DashboardScope,
	tab: DashboardScopedTab = "all",
) {
	const params = buildDashboardScopePageQueryParams(scope);
	const query = params.toString();
	const path = buildDashboardScopePath(scope, tab);
	return query ? `${path}?${query}` : path;
}

export function buildDashboardReleaseHref(
	locator: DashboardReleaseLocator,
	fromTab: DashboardTab = "briefs",
	options?: {
		scope?: DashboardScope | null;
	},
) {
	const params = buildDashboardScopeQueryParams(options?.scope ?? null);
	params.set(
		"from",
		normalizeDashboardReturnTab(fromTab, options?.scope ?? null),
	);
	const query = params.toString();
	return `${buildDashboardReleasePath(locator)}${query ? `?${query}` : ""}`;
}

export function buildDashboardRouteUrl(routeState: DashboardRouteState) {
	if (routeState.activeReleaseLocator) {
		return buildDashboardReleaseHref(
			routeState.activeReleaseLocator,
			routeState.releaseReturnTab,
			{ scope: routeState.scope },
		);
	}
	const basePath = routeState.scope
		? buildDashboardScopePath(
				routeState.scope,
				normalizeDashboardTab(
					routeState.tab,
					routeState.scope,
				) as DashboardScopedTab,
			)
		: buildDashboardTabPath(routeState.tab);
	const params = routeState.scope
		? buildDashboardScopePageQueryParams(routeState.scope)
		: new URLSearchParams();
	if (routeState.activeReleaseId) {
		params.set(
			routeState.scope ? "from" : "tab",
			normalizeDashboardReturnTab(
				routeState.releaseReturnTab,
				routeState.scope,
			),
		);
		params.set("release", routeState.activeReleaseId);
	}
	const query = params.toString();
	return query ? `${basePath}?${query}` : basePath;
}

function searchObjectFromParams(params: URLSearchParams) {
	const entries = Array.from(params.entries());
	return entries.length > 0 ? Object.fromEntries(entries) : undefined;
}

export function buildDashboardRouteNavigation(
	routeState: DashboardRouteState,
): DashboardRouteNavigation {
	if (routeState.activeReleaseLocator) {
		const params = buildDashboardScopeQueryParams(routeState.scope);
		params.set(
			"from",
			normalizeDashboardReturnTab(
				routeState.releaseReturnTab,
				routeState.scope,
			),
		);
		return {
			to: "/$owner/$repo/releases/tag/$tag",
			params: {
				owner: routeState.activeReleaseLocator.owner,
				repo: routeState.activeReleaseLocator.repo,
				tag: routeState.activeReleaseLocator.tag,
			},
			search: searchObjectFromParams(params),
		};
	}

	if (routeState.scope) {
		const tab = normalizeDashboardTab(routeState.tab, routeState.scope);
		const searchParams = buildDashboardScopePageQueryParams(routeState.scope);
		if (routeState.activeReleaseId) {
			searchParams.set(
				"from",
				normalizeDashboardReturnTab(
					routeState.releaseReturnTab,
					routeState.scope,
				),
			);
			searchParams.set("release", routeState.activeReleaseId);
		}
		switch (routeState.scope.kind) {
			case "repo":
				return {
					to:
						tab === "releases"
							? "/focus/repo/$owner/$repo/releases"
							: "/focus/repo/$owner/$repo",
					params: {
						owner: routeState.scope.owner,
						repo: routeState.scope.repo,
					},
					search: searchObjectFromParams(searchParams),
				};
			case "repos":
				return {
					to: tab === "releases" ? "/focus/repos/releases" : "/focus/repos",
					search: searchObjectFromParams(searchParams),
				};
			case "org":
				return {
					to:
						tab === "releases" ? "/focus/org/$org/releases" : "/focus/org/$org",
					params: { org: routeState.scope.org },
					search: searchObjectFromParams(searchParams),
				};
			case "mine":
				return {
					to: tab === "releases" ? "/focus/mine/releases" : "/focus/mine",
					search: searchObjectFromParams(searchParams),
				};
		}
	}

	const searchParams = new URLSearchParams();
	if (routeState.activeReleaseId) {
		searchParams.set(
			"tab",
			normalizeDashboardReturnTab(routeState.releaseReturnTab, null),
		);
		searchParams.set("release", routeState.activeReleaseId);
	}
	return {
		to: buildDashboardTabPath(routeState.tab),
		search: searchObjectFromParams(searchParams),
	};
}

export function buildDashboardWarmRouteState(
	routeState: DashboardRouteState,
): DashboardWarmRouteState {
	return {
		tab: routeState.tab,
		scopeSignature: buildDashboardScopeSignature(routeState.scope),
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

function parseDashboardScopePathname(
	pathname: string,
	searchParams: URLSearchParams,
): { scope: DashboardScope; tab: DashboardScopedTab } | null {
	const segments = normalizePathname(pathname).split("/").filter(Boolean);
	if (segments[0] !== "focus") return null;
	if (segments[1] === "repo" && segments.length >= 4) {
		const owner = decodeSegment(segments[2] ?? "").trim();
		const repo = decodeSegment(segments[3] ?? "").trim();
		if (!owner || !repo) return null;
		return {
			scope: { kind: "repo", owner, repo },
			tab: segments[4] === "releases" ? "releases" : "all",
		};
	}
	if (segments[1] === "repos") {
		return {
			scope: {
				kind: "repos",
				items: normalizeRepoItems(searchParams.get("items")),
			},
			tab: segments[2] === "releases" ? "releases" : "all",
		};
	}
	if (segments[1] === "org" && segments.length >= 3) {
		const org = decodeSegment(segments[2] ?? "").trim();
		if (!org) return null;
		return {
			scope: { kind: "org", org },
			tab: segments[3] === "releases" ? "releases" : "all",
		};
	}
	if (segments[1] === "mine") {
		return {
			scope: { kind: "mine" },
			tab: segments[2] === "releases" ? "releases" : "all",
		};
	}
	return null;
}

function searchParamsFromInput(
	input:
		| string
		| URLSearchParams
		| {
				tab?: string | null;
				release?: string | null;
				from?: string | null;
				scope?: string | null;
				items?: string | null;
				org?: string | null;
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
	if (input?.scope) params.set("scope", input.scope);
	if (input?.items) params.set("items", input.items);
	if (input?.org) params.set("org", input.org);
	return params;
}

function parseDashboardScopeFromSearch(
	searchParams: URLSearchParams,
	releaseLocator: DashboardReleaseLocator | null,
): DashboardScope | null {
	const scopeKind = searchParams.get("scope")?.trim();
	switch (scopeKind) {
		case "repo":
			return releaseLocator
				? {
						kind: "repo",
						owner: releaseLocator.owner,
						repo: releaseLocator.repo,
					}
				: null;
		case "repos":
			return {
				kind: "repos",
				items: normalizeRepoItems(searchParams.get("items")),
			};
		case "org": {
			const org = normalizeScopeOrg(searchParams.get("org"));
			return org ? { kind: "org", org } : null;
		}
		case "mine":
			return { kind: "mine" };
		default:
			return null;
	}
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
			scope: null,
			activeReleaseId: releaseId,
			activeReleaseLocator: null,
			releaseReturnTab: normalizeDashboardReturnTab(input.tab),
		};
	}

	return {
		tab: normalizeDashboardTab(input.tab),
		scope: null,
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
				scope?: string | null;
				items?: string | null;
				org?: string | null;
		  }
		| null;
	tab?: string | null;
	release?: string | null;
	from?: string | null;
	scope?: DashboardScope | null;
	owner?: string | null;
	repo?: string | null;
	tag?: string | null;
}): DashboardRouteState {
	const pathname = normalizePathname(input.pathname ?? "/");
	const searchParams = searchParamsFromInput(
		input.search ?? {
			tab: input.tab,
			release: input.release,
			from: input.from,
		},
	);
	const parsedScopePath = parseDashboardScopePathname(pathname, searchParams);
	const explicitScope = input.scope ?? parsedScopePath?.scope ?? null;

	if (input.owner && input.repo && input.tag) {
		const locator = {
			owner: input.owner,
			repo: input.repo,
			tag: input.tag,
		};
		const detailScope =
			explicitScope ?? parseDashboardScopeFromSearch(searchParams, locator);
		const fromTab = normalizeDashboardReturnTab(
			input.from ?? searchParams.get("from"),
			detailScope,
		);
		return {
			tab: fromTab,
			scope: detailScope,
			activeReleaseId: null,
			activeReleaseLocator: locator,
			releaseReturnTab: fromTab,
		};
	}

	const releaseLocator = parseDashboardReleasePathname(pathname);
	if (releaseLocator) {
		const detailScope =
			explicitScope ??
			parseDashboardScopeFromSearch(searchParams, releaseLocator);
		const fromTab = normalizeDashboardReturnTab(
			searchParams.get("from"),
			detailScope,
		);
		return {
			tab: fromTab,
			scope: detailScope,
			activeReleaseId: null,
			activeReleaseLocator: releaseLocator,
			releaseReturnTab: fromTab,
		};
	}

	if (parsedScopePath) {
		const scope = explicitScope ?? parsedScopePath.scope;
		const releaseId = normalizeReleaseId(searchParams.get("release"));
		const fromTab = normalizeDashboardReturnTab(
			searchParams.get("from"),
			scope,
		);
		const tab = normalizeDashboardTab(parsedScopePath.tab, scope);
		if (releaseId) {
			return {
				tab: fromTab,
				scope,
				activeReleaseId: releaseId,
				activeReleaseLocator: null,
				releaseReturnTab: fromTab,
			};
		}
		return {
			tab,
			scope,
			activeReleaseId: null,
			activeReleaseLocator: null,
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
		scope: null,
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
		scope: typeof search.scope === "string" ? search.scope : undefined,
		items: typeof search.items === "string" ? search.items : undefined,
		org: typeof search.org === "string" ? search.org : undefined,
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
		parseDashboardReleasePathname(normalized) !== null ||
		normalized === "/focus/repos" ||
		normalized === "/focus/repos/releases" ||
		normalized === "/focus/mine" ||
		normalized === "/focus/mine/releases" ||
		normalized.startsWith("/focus/repo/") ||
		normalized.startsWith("/focus/org/")
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
	scope?: DashboardScope | null;
}): DashboardReleaseTarget {
	return {
		releaseId: normalizeReleaseId(input.releaseId) ?? null,
		locator: input.locator ?? null,
		fromTab: normalizeDashboardReturnTab(input.fromTab, input.scope ?? null),
		scope: input.scope ?? null,
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
			const scope = parseDashboardScopeFromSearch(url.searchParams, locator);
			return {
				releaseId: null,
				locator,
				fromTab: normalizeDashboardReturnTab(
					url.searchParams.get("from"),
					scope,
				),
				scope,
			};
		}
		const scoped = parseDashboardScopePathname(url.pathname, url.searchParams);
		if (scoped) {
			const releaseId = normalizeReleaseId(url.searchParams.get("release"));
			if (!releaseId) return null;
			return {
				releaseId,
				locator: null,
				fromTab: normalizeDashboardReturnTab(
					url.searchParams.get("from"),
					scoped.scope,
				),
				scope: scoped.scope,
			};
		}
		if (normalizePathname(url.pathname) !== "/") return null;
		const releaseId = normalizeReleaseId(url.searchParams.get("release"));
		if (!releaseId) return null;
		return {
			releaseId,
			locator: null,
			fromTab: normalizeDashboardReturnTab(url.searchParams.get("tab")),
			scope: null,
		};
	} catch {
		return null;
	}
}
