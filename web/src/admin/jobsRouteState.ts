export type AdminJobsPrimaryTab =
	| "realtime"
	| "scheduled"
	| "subscriptions"
	| "llm"
	| "translations"
	| "ai_records";

export type TranslationViewTab = "queue" | "history";

export type LlmCallRouteStatus =
	| "all"
	| "queued"
	| "running"
	| "succeeded"
	| "failed";

export type LlmCallRouteFilters = {
	status: LlmCallRouteStatus;
	model: string;
	source: string;
	requestedBy: string;
	startedFrom: string;
	startedTo: string;
	finishedFrom: string;
	finishedBefore: string;
};

export type AdminJobsSearchInput = {
	from?: string;
	view?: string;
	llm_status?: string;
	llm_model?: string;
	llm_source?: string;
	llm_requested_by?: string;
	llm_started_from?: string;
	llm_started_to?: string;
	llm_finished_from?: string;
	llm_finished_before?: string;
	// Legacy single-axis time range parameters. Parsed for shared-link compatibility.
	llm_time_field?: string;
	llm_time_from?: string;
	llm_time_to?: string;
	ai_attempt?: string;
	ai_llm?: string;
};

export type TaskDrawerRoute = {
	taskId: string;
	llmCallId: string | null;
};

export type AiRecordDetailRoute = {
	kind: "release" | "announcement" | "brief";
	id: string;
	attemptId?: string | null;
	llmCallId?: string | null;
};

export type AdminJobsRouteState = {
	primaryTab: AdminJobsPrimaryTab;
	translationView: TranslationViewTab;
	taskDrawerRoute: TaskDrawerRoute | null;
	drawerFromTab: AdminJobsPrimaryTab | null;
	subscriptionDetailTaskId?: string | null;
	llmCallFilters?: LlmCallRouteFilters;
	aiRecordDetailRoute?: AiRecordDetailRoute | null;
};

export const ADMIN_JOBS_BASE_PATH = "/admin/jobs";
export const ADMIN_JOBS_SCHEDULED_PATH = `${ADMIN_JOBS_BASE_PATH}/scheduled`;
export const ADMIN_JOBS_SUBSCRIPTIONS_PATH = `${ADMIN_JOBS_BASE_PATH}/subscriptions`;
export const ADMIN_JOBS_LLM_PATH = `${ADMIN_JOBS_BASE_PATH}/llm`;
export const ADMIN_JOBS_TRANSLATIONS_PATH = `${ADMIN_JOBS_BASE_PATH}/translations`;
export const ADMIN_JOBS_AI_RECORDS_PATH = `${ADMIN_JOBS_BASE_PATH}/ai-records`;
export const ADMIN_SUBSCRIPTION_SETTINGS_AUTO_OPEN_SESSION_KEY =
	"admin.jobs.subscription-settings.auto-open";

const ADMIN_JOBS_ROUTE_QUERY_KEYS = [
	"from",
	"view",
	"llm_status",
	"llm_model",
	"llm_source",
	"llm_requested_by",
	"llm_started_from",
	"llm_started_to",
	"llm_finished_from",
	"llm_finished_before",
	"llm_time_field",
	"llm_time_from",
	"llm_time_to",
	"ai_attempt",
	"ai_llm",
] as const;
const TASK_DRAWER_ROUTE_PATTERN =
	/^\/admin\/jobs\/tasks\/([^/]+?)(?:\/llm\/([^/]+))?$/;
const SUBSCRIPTION_DETAIL_ROUTE_PATTERN =
	/^\/admin\/jobs\/subscriptions\/([^/]+?)$/;
const AI_RECORD_DETAIL_ROUTE_PATTERN =
	/^\/admin\/jobs\/ai-records\/(release|announcement|brief)\/([^/]+?)$/;

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
		value === "translations" ||
		value === "ai_records"
	);
}

function normalizeLlmRouteText(value: string | null | undefined) {
	return value?.trim() ?? "";
}

function normalizeLlmRouteTimestamp(value: string | null | undefined) {
	const normalized = normalizeLlmRouteText(value);
	if (!normalized || !/\dT\d.*(?:Z|[+-]\d{2}:\d{2})$/i.test(normalized)) {
		return "";
	}
	const timestamp = Date.parse(normalized);
	return Number.isNaN(timestamp) ? "" : new Date(timestamp).toISOString();
}

export function parseLlmCallRouteFilters(
	search: Pick<
		AdminJobsSearchInput,
		| "llm_status"
		| "llm_model"
		| "llm_source"
		| "llm_requested_by"
		| "llm_started_from"
		| "llm_started_to"
		| "llm_finished_from"
		| "llm_finished_before"
		| "llm_time_field"
		| "llm_time_from"
		| "llm_time_to"
	>,
): LlmCallRouteFilters {
	const status = search.llm_status;
	const legacyTimeFrom = normalizeLlmRouteTimestamp(search.llm_time_from);
	const legacyTimeTo = normalizeLlmRouteTimestamp(search.llm_time_to);
	const useLegacyFinishedRange = search.llm_time_field === "finished";
	return {
		status:
			status === "queued" ||
			status === "running" ||
			status === "succeeded" ||
			status === "failed"
				? status
				: "all",
		model: normalizeLlmRouteText(search.llm_model),
		source: normalizeLlmRouteText(search.llm_source),
		requestedBy: normalizeLlmRouteText(search.llm_requested_by),
		startedFrom:
			normalizeLlmRouteTimestamp(search.llm_started_from) ||
			(useLegacyFinishedRange ? "" : legacyTimeFrom),
		startedTo:
			normalizeLlmRouteTimestamp(search.llm_started_to) ||
			(useLegacyFinishedRange ? "" : legacyTimeTo),
		finishedFrom:
			normalizeLlmRouteTimestamp(search.llm_finished_from) ||
			(useLegacyFinishedRange ? legacyTimeFrom : ""),
		finishedBefore:
			normalizeLlmRouteTimestamp(search.llm_finished_before) ||
			(useLegacyFinishedRange ? legacyTimeTo : ""),
	};
}

export function llmCallRouteFiltersToSearch(
	filters: LlmCallRouteFilters,
): AdminJobsSearchInput {
	return {
		llm_status: filters.status === "all" ? undefined : filters.status,
		llm_model: filters.model || undefined,
		llm_source: filters.source || undefined,
		llm_requested_by: filters.requestedBy || undefined,
		llm_started_from: filters.startedFrom || undefined,
		llm_started_to: filters.startedTo || undefined,
		llm_finished_from: filters.finishedFrom || undefined,
		llm_finished_before: filters.finishedBefore || undefined,
		llm_time_field: undefined,
		llm_time_from: undefined,
		llm_time_to: undefined,
	};
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
		case "ai_records":
			return ADMIN_JOBS_AI_RECORDS_PATH;
		default:
			return ADMIN_JOBS_BASE_PATH;
	}
}

export function buildAiRecordDetailPath(
	kind: AiRecordDetailRoute["kind"],
	id: string,
) {
	return `${ADMIN_JOBS_AI_RECORDS_PATH}/${kind}/${encodeURIComponent(id)}`;
}

function parseAiRecordDetailRoute(
	pathname: string,
	searchParams: URLSearchParams,
): AiRecordDetailRoute | null {
	const matched = normalizePathname(pathname).match(
		AI_RECORD_DETAIL_ROUTE_PATTERN,
	);
	if (!matched) return null;
	try {
		return {
			kind: matched[1] as AiRecordDetailRoute["kind"],
			id: decodeURIComponent(matched[2] ?? ""),
			attemptId: searchParams.get("ai_attempt") || null,
			llmCallId: searchParams.get("ai_llm") || null,
		};
	} catch {
		return null;
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
	const llmCallFilters = parseLlmCallRouteFilters({
		llm_status: searchParams.get("llm_status") ?? undefined,
		llm_model: searchParams.get("llm_model") ?? undefined,
		llm_source: searchParams.get("llm_source") ?? undefined,
		llm_requested_by: searchParams.get("llm_requested_by") ?? undefined,
		llm_started_from: searchParams.get("llm_started_from") ?? undefined,
		llm_started_to: searchParams.get("llm_started_to") ?? undefined,
		llm_finished_from: searchParams.get("llm_finished_from") ?? undefined,
		llm_finished_before: searchParams.get("llm_finished_before") ?? undefined,
		llm_time_field: searchParams.get("llm_time_field") ?? undefined,
		llm_time_from: searchParams.get("llm_time_from") ?? undefined,
		llm_time_to: searchParams.get("llm_time_to") ?? undefined,
	});
	const translationView = parseTranslationView(searchParams);
	const rawDrawerFromTab = searchParams.get("from");
	const drawerFromTab = isPrimaryTab(rawDrawerFromTab)
		? rawDrawerFromTab
		: null;
	const taskDrawerRoute = parseTaskDrawerRoute(pathname);
	const aiRecordDetailRoute = parseAiRecordDetailRoute(pathname, searchParams);

	if (taskDrawerRoute) {
		return {
			primaryTab: drawerFromTab ?? "realtime",
			translationView,
			taskDrawerRoute,
			drawerFromTab,
			subscriptionDetailTaskId: null,
			llmCallFilters,
		};
	}
	if (aiRecordDetailRoute) {
		return {
			primaryTab: "ai_records",
			translationView,
			taskDrawerRoute: null,
			drawerFromTab: null,
			subscriptionDetailTaskId: null,
			llmCallFilters,
			aiRecordDetailRoute,
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
			llmCallFilters,
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
	} else if (normalizedPath === ADMIN_JOBS_AI_RECORDS_PATH) {
		primaryTab = "ai_records";
	}

	return {
		primaryTab,
		translationView,
		taskDrawerRoute: null,
		drawerFromTab: null,
		subscriptionDetailTaskId: null,
		llmCallFilters,
		aiRecordDetailRoute: null,
	};
}

export function buildAdminJobsRouteUrl(
	route: AdminJobsRouteState,
	currentSearch = "",
) {
	const searchParams = new URLSearchParams(currentSearch);
	const pathname = route.aiRecordDetailRoute
		? buildAiRecordDetailPath(
				route.aiRecordDetailRoute.kind,
				route.aiRecordDetailRoute.id,
			)
		: route.subscriptionDetailTaskId
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
	} else if (route.primaryTab === "llm") {
		const filters = llmCallRouteFiltersToSearch(
			route.llmCallFilters ?? parseLlmCallRouteFilters({}),
		);
		for (const [key, value] of Object.entries(filters)) {
			if (value) searchParams.set(key, value);
		}
	} else if (route.aiRecordDetailRoute) {
		if (route.aiRecordDetailRoute.attemptId) {
			searchParams.set("ai_attempt", route.aiRecordDetailRoute.attemptId);
		}
		if (route.aiRecordDetailRoute.llmCallId) {
			searchParams.set("ai_llm", route.aiRecordDetailRoute.llmCallId);
		}
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
	aiRecordKind?: AiRecordDetailRoute["kind"];
	aiRecordId?: string;
	aiRecordAttemptId?: string;
	aiRecordLlmCallId?: string;
}): AdminJobsRouteState {
	const translationView = parseTranslationView(input.search.view);
	const drawerFromTab = isPrimaryTab(input.search.from)
		? input.search.from
		: null;
	const llmCallFilters = parseLlmCallRouteFilters(input.search);

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
			llmCallFilters,
		};
	}
	if (input.aiRecordKind && input.aiRecordId) {
		return {
			primaryTab: "ai_records",
			translationView,
			taskDrawerRoute: null,
			drawerFromTab: null,
			subscriptionDetailTaskId: null,
			llmCallFilters,
			aiRecordDetailRoute: {
				kind: input.aiRecordKind,
				id: input.aiRecordId,
				attemptId: input.aiRecordAttemptId ?? null,
				llmCallId: input.aiRecordLlmCallId ?? null,
			},
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
		llmCallFilters,
	};
}

export function validateAdminJobsSearch(search: Record<string, unknown>) {
	return {
		from: typeof search.from === "string" ? search.from : undefined,
		view: typeof search.view === "string" ? search.view : undefined,
		llm_status:
			typeof search.llm_status === "string" ? search.llm_status : undefined,
		llm_model:
			typeof search.llm_model === "string" ? search.llm_model : undefined,
		llm_source:
			typeof search.llm_source === "string" ? search.llm_source : undefined,
		llm_requested_by:
			typeof search.llm_requested_by === "string"
				? search.llm_requested_by
				: undefined,
		llm_started_from:
			typeof search.llm_started_from === "string"
				? search.llm_started_from
				: undefined,
		llm_started_to:
			typeof search.llm_started_to === "string"
				? search.llm_started_to
				: undefined,
		llm_finished_from:
			typeof search.llm_finished_from === "string"
				? search.llm_finished_from
				: undefined,
		llm_finished_before:
			typeof search.llm_finished_before === "string"
				? search.llm_finished_before
				: undefined,
		llm_time_field:
			typeof search.llm_time_field === "string"
				? search.llm_time_field
				: undefined,
		llm_time_from:
			typeof search.llm_time_from === "string"
				? search.llm_time_from
				: undefined,
		llm_time_to:
			typeof search.llm_time_to === "string" ? search.llm_time_to : undefined,
		ai_attempt:
			typeof search.ai_attempt === "string" ? search.ai_attempt : undefined,
		ai_llm: typeof search.ai_llm === "string" ? search.ai_llm : undefined,
	};
}
