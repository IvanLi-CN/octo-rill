export type AdminJobsPrimaryTab =
	| "realtime"
	| "scheduled"
	| "subscriptions"
	| "llm"
	| "translations";

export type TranslationViewTab = "queue" | "history";

export type AdminJobsSearchInput = {
	from?: string;
	view?: string;
};

export type TaskDrawerRoute = {
	taskId: string;
	llmCallId: string | null;
};

export type AdminJobsRouteState = {
	primaryTab: AdminJobsPrimaryTab;
	translationView: TranslationViewTab;
	taskDrawerRoute: TaskDrawerRoute | null;
	drawerFromTab: AdminJobsPrimaryTab | null;
	subscriptionDetailTaskId?: string | null;
};

export const ADMIN_JOBS_BASE_PATH = "/admin/jobs";
export const ADMIN_JOBS_SCHEDULED_PATH = `${ADMIN_JOBS_BASE_PATH}/scheduled`;
export const ADMIN_JOBS_SUBSCRIPTIONS_PATH = `${ADMIN_JOBS_BASE_PATH}/subscriptions`;
export const ADMIN_JOBS_LLM_PATH = `${ADMIN_JOBS_BASE_PATH}/llm`;
export const ADMIN_JOBS_TRANSLATIONS_PATH = `${ADMIN_JOBS_BASE_PATH}/translations`;

const ADMIN_JOBS_ROUTE_QUERY_KEYS = ["from", "view"] as const;
const TASK_DRAWER_ROUTE_PATTERN =
	/^\/admin\/jobs\/tasks\/([^/]+?)(?:\/llm\/([^/]+))?$/;
const SUBSCRIPTION_DETAIL_ROUTE_PATTERN =
	/^\/admin\/jobs\/subscriptions\/([^/]+?)$/;

function normalizePathname(pathname: string) {
	return pathname.replace(/\/+$/, "") || "/";
}

function isPrimaryTab(
	value: string | null | undefined,
): value is AdminJobsPrimaryTab {
	return (
		value === "realtime" ||
		value === "scheduled" ||
		value === "subscriptions" ||
		value === "llm" ||
		value === "translations"
	);
}

export function parseTranslationView(
	value: string | URLSearchParams | null | undefined,
): TranslationViewTab {
	if (value instanceof URLSearchParams) {
		return value.get("view") === "history" ? "history" : "queue";
	}

	return value === "history" ? "history" : "queue";
}

export function buildAdminJobsBasePath(primaryTab: AdminJobsPrimaryTab) {
	switch (primaryTab) {
		case "scheduled":
			return ADMIN_JOBS_SCHEDULED_PATH;
		case "subscriptions":
			return ADMIN_JOBS_SUBSCRIPTIONS_PATH;
		case "llm":
			return ADMIN_JOBS_LLM_PATH;
		case "translations":
			return ADMIN_JOBS_TRANSLATIONS_PATH;
		default:
			return ADMIN_JOBS_BASE_PATH;
	}
}

export function buildTaskDrawerPath(taskId: string, llmCallId?: string | null) {
	const base = `${ADMIN_JOBS_BASE_PATH}/tasks/${encodeURIComponent(taskId)}`;
	if (!llmCallId) return base;
	return `${base}/llm/${encodeURIComponent(llmCallId)}`;
}

export function parseTaskDrawerRoute(pathname: string): TaskDrawerRoute | null {
	const normalized = normalizePathname(pathname);
	const matched = normalized.match(TASK_DRAWER_ROUTE_PATTERN);
	if (!matched) return null;
	try {
		return {
			taskId: decodeURIComponent(matched[1] ?? ""),
			llmCallId: matched[2] ? decodeURIComponent(matched[2]) : null,
		};
	} catch {
		return null;
	}
}

export function parseAdminJobsRoute(
	pathname: string,
	search: string,
): AdminJobsRouteState {
	const searchParams = new URLSearchParams(search);
	const translationView = parseTranslationView(searchParams);
	const rawDrawerFromTab = searchParams.get("from");
	const drawerFromTab = isPrimaryTab(rawDrawerFromTab)
		? rawDrawerFromTab
		: null;
	const taskDrawerRoute = parseTaskDrawerRoute(pathname);

	if (taskDrawerRoute) {
		return {
			primaryTab: drawerFromTab ?? "realtime",
			translationView,
			taskDrawerRoute,
			drawerFromTab,
			subscriptionDetailTaskId: null,
		};
	}

	const normalizedPath = normalizePathname(pathname);
	const subscriptionDetailMatch = normalizedPath.match(
		SUBSCRIPTION_DETAIL_ROUTE_PATTERN,
	);
	if (subscriptionDetailMatch) {
		return {
			primaryTab: "subscriptions",
			translationView,
			taskDrawerRoute: null,
			drawerFromTab: null,
			subscriptionDetailTaskId: decodeURIComponent(
				subscriptionDetailMatch[1] ?? "",
			),
		};
	}

	let primaryTab: AdminJobsPrimaryTab = "realtime";
	if (normalizedPath === ADMIN_JOBS_SCHEDULED_PATH) {
		primaryTab = "scheduled";
	} else if (normalizedPath === ADMIN_JOBS_SUBSCRIPTIONS_PATH) {
		primaryTab = "subscriptions";
	} else if (normalizedPath === ADMIN_JOBS_LLM_PATH) {
		primaryTab = "llm";
	} else if (normalizedPath === ADMIN_JOBS_TRANSLATIONS_PATH) {
		primaryTab = "translations";
	}

	return {
		primaryTab,
		translationView,
		taskDrawerRoute: null,
		drawerFromTab: null,
		subscriptionDetailTaskId: null,
	};
}

export function buildAdminJobsRouteUrl(
	route: AdminJobsRouteState,
	currentSearch = "",
) {
	const searchParams = new URLSearchParams(currentSearch);
	const pathname = route.subscriptionDetailTaskId
		? `${ADMIN_JOBS_SUBSCRIPTIONS_PATH}/${encodeURIComponent(
				route.subscriptionDetailTaskId,
			)}`
		: route.taskDrawerRoute
			? buildTaskDrawerPath(
					route.taskDrawerRoute.taskId,
					route.taskDrawerRoute.llmCallId,
				)
			: buildAdminJobsBasePath(route.primaryTab);

	for (const key of ADMIN_JOBS_ROUTE_QUERY_KEYS) {
		searchParams.delete(key);
	}

	if (route.taskDrawerRoute) {
		if (route.drawerFromTab) {
			searchParams.set("from", route.drawerFromTab);
			if (route.drawerFromTab === "translations") {
				searchParams.set("view", route.translationView);
			}
		}
	} else if (route.primaryTab === "translations") {
		searchParams.set("view", route.translationView);
	}

	const query = searchParams.toString();
	return `${pathname}${query ? `?${query}` : ""}`;
}

export function buildAdminJobsRouteState(input: {
	primaryTab: AdminJobsPrimaryTab;
	search: AdminJobsSearchInput;
	taskId?: string;
	llmCallId?: string;
	subscriptionDetailTaskId?: string;
}): AdminJobsRouteState {
	const translationView = parseTranslationView(input.search.view);
	const drawerFromTab = isPrimaryTab(input.search.from)
		? input.search.from
		: null;

	if (input.taskId) {
		return {
			primaryTab: drawerFromTab ?? "realtime",
			translationView,
			taskDrawerRoute: {
				taskId: input.taskId,
				llmCallId: input.llmCallId ?? null,
			},
			drawerFromTab,
			subscriptionDetailTaskId: null,
		};
	}

	return {
		primaryTab: input.subscriptionDetailTaskId
			? "subscriptions"
			: input.primaryTab,
		translationView,
		taskDrawerRoute: null,
		drawerFromTab: null,
		subscriptionDetailTaskId: input.subscriptionDetailTaskId ?? null,
	};
}

export function validateAdminJobsSearch(search: Record<string, unknown>) {
	return {
		from: typeof search.from === "string" ? search.from : undefined,
		view: typeof search.view === "string" ? search.view : undefined,
	};
}
