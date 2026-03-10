export class ApiError extends Error {
	public status: number;
	public code: string;
	constructor(status: number, message: string, code = "unknown_error") {
		super(message);
		this.status = status;
		this.code = code;
	}
}
async function parseJson(res: Response) {
	const text = await res.text();
	if (!text) return null;
	try {
		return JSON.parse(text) as unknown;
	} catch {
		return text;
	}
}
function toApiError(res: Response, body: unknown) {
	if (typeof body === "object" && body && "error" in body) {
		const payload = body as {
			error?: { code?: string; message?: string };
		};
		return new ApiError(
			res.status,
			String(payload.error?.message ?? res.statusText),
			String(payload.error?.code ?? "unknown_error"),
		);
	}
	return new ApiError(res.status, res.statusText);
}
export async function apiGet<T>(path: string): Promise<T> {
	const res = await fetch(path, { credentials: "include" });
	if (!res.ok) {
		const body = await parseJson(res);
		throw toApiError(res, body);
	}
	return (await res.json()) as T;
}
export async function apiPost<T>(path: string): Promise<T> {
	return apiPostJson<T>(path);
}
export async function apiPostJson<T>(path: string, body?: unknown): Promise<T> {
	const res = await fetch(path, {
		method: "POST",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	if (!res.ok) {
		throw toApiError(res, await parseJson(res));
	}
	return (await res.json()) as T;
}
export async function apiPutJson<T>(path: string, body?: unknown): Promise<T> {
	const res = await fetch(path, {
		method: "PUT",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	if (!res.ok) {
		throw toApiError(res, await parseJson(res));
	}
	return (await res.json()) as T;
}
export async function apiPatchJson<T>(
	path: string,
	body?: unknown,
): Promise<T> {
	const res = await fetch(path, {
		method: "PATCH",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: body === undefined ? undefined : JSON.stringify(body),
	});
	if (!res.ok) {
		throw toApiError(res, await parseJson(res));
	}
	return (await res.json()) as T;
}
export type LocalUserId = string;
export type MeResponse = {
	user: {
		id: LocalUserId;
		github_user_id: number;
		login: string;
		name: string | null;
		avatar_url: string | null;
		email: string | null;
		is_admin: boolean;
	};
};
export type AdminUserProfileResponse = {
	user_id: LocalUserId;
	daily_brief_utc_time: string;
	last_active_at: string | null;
};
export type AdminJobsOverviewResponse = {
	queued: number;
	running: number;
	failed_24h: number;
	succeeded_24h: number;
	enabled_scheduled_slots: number;
	total_scheduled_slots: number;
};
export type AdminRealtimeTaskItem = {
	id: string;
	task_type: string;
	status: string;
	source: string;
	requested_by: LocalUserId | null;
	parent_task_id: string | null;
	cancel_requested: boolean;
	error_message: string | null;
	created_at: string;
	started_at: string | null;
	finished_at: string | null;
	updated_at: string;
};
export type AdminRealtimeTasksResponse = {
	items: AdminRealtimeTaskItem[];
	page: number;
	page_size: number;
	total: number;
};
export type AdminTaskEventItem = {
	id: string;
	event_type: string;
	payload_json: string;
	created_at: string;
};
export type AdminTaskEventMeta = {
	returned: number;
	total: number;
	limit: number;
	truncated: boolean;
};
export type AdminBusinessOutcome = {
	code: "ok" | "partial" | "failed" | "disabled" | "unknown";
	label: string;
	message: string;
};
export type AdminTranslateReleaseBatchDiagnostics = {
	target_user_id: LocalUserId | null;
	release_total: number;
	summary: {
		total: number;
		ready: number;
		missing: number;
		disabled: number;
		error: number;
	};
	progress: {
		processed: number;
		last_stage: string | null;
	};
	items: Array<{
		release_id: string;
		item_status: string;
		item_error: string | null;
		last_event_at: string;
	}>;
};
export type AdminBriefDailySlotDiagnostics = {
	hour_utc: number | null;
	summary: {
		total_users: number;
		progressed_users: number;
		succeeded_users: number;
		failed_users: number;
		canceled: boolean;
	};
	users: Array<{
		user_id: LocalUserId;
		key_date: string | null;
		state: "succeeded" | "failed" | "running";
		error: string | null;
		last_event_at: string;
	}>;
};
export type AdminBriefGenerateDiagnostics = {
	target_user_id: LocalUserId | null;
	content_length: number | null;
	key_date: string | null;
};
export type AdminSyncSubscriptionsDiagnostics = {
	trigger: string | null;
	schedule_key: string | null;
	skipped: boolean;
	skip_reason: string | null;
	log_available: boolean;
	log_download_path: string | null;
	star: {
		total_users: number;
		succeeded_users: number;
		failed_users: number;
		total_repos: number;
	};
	release: {
		total_repos: number;
		succeeded_repos: number;
		failed_repos: number;
		candidate_failures: number;
	};
	releases_written: number;
	critical_events: number;
	recent_events: Array<{
		id: string;
		stage: string;
		event_type: string;
		severity: string;
		recoverable: boolean;
		attempt: number;
		user_id: LocalUserId | null;
		repo_id: number | null;
		repo_full_name: string | null;
		message: string | null;
		created_at: string;
	}>;
};
export type AdminTaskDiagnostics = {
	business_outcome: AdminBusinessOutcome;
	translate_release_batch?: AdminTranslateReleaseBatchDiagnostics | null;
	brief_daily_slot?: AdminBriefDailySlotDiagnostics | null;
	brief_generate?: AdminBriefGenerateDiagnostics | null;
	sync_subscriptions?: AdminSyncSubscriptionsDiagnostics | null;
};
export type AdminRealtimeTaskDetailItem = AdminRealtimeTaskItem & {
	payload_json?: string | null;
	result_json?: string | null;
};
export type AdminRealtimeTaskDetailResponse = {
	task: AdminRealtimeTaskDetailItem;
	events: AdminTaskEventItem[];
	event_meta?: AdminTaskEventMeta | null;
	diagnostics?: AdminTaskDiagnostics | null;
};
export type AdminTaskActionResponse = {
	task_id: string;
	status: string;
};
export type AdminJobsStreamEvent = {
	event_id: string;
	task_id: string;
	task_type: string;
	status: string;
	event_type: string;
	created_at: string;
};
export type AdminLlmCallStreamEvent = {
	event_id: string;
	call_id: string;
	status: string;
	source: string;
	requested_by: LocalUserId | null;
	parent_task_id: string | null;
	event_type: string;
	created_at: string;
};
export type AdminTranslationStreamEvent = {
	event_id: string;
	resource_type: "request" | "batch" | "worker";
	resource_id: string;
	status: string;
	event_type: string;
	created_at: string;
};
export type AdminScheduledSlotItem = {
	hour_utc: number;
	enabled: boolean;
	last_dispatch_at: string | null;
	updated_at: string;
};
export type AdminScheduledSlotsResponse = {
	items: AdminScheduledSlotItem[];
};
export type AdminLlmSchedulerStatusResponse = {
	scheduler_enabled: boolean;
	request_interval_ms: number;
	waiting_calls: number;
	in_flight_calls: number;
	next_slot_in_ms: number;
	calls_24h: number;
	failed_24h: number;
	avg_wait_ms_24h: number | null;
	avg_duration_ms_24h: number | null;
	last_success_at: string | null;
	last_failure_at: string | null;
};
export type AdminLlmCallItem = {
	id: string;
	status: string;
	source: string;
	model: string;
	requested_by: LocalUserId | null;
	parent_task_id: string | null;
	parent_task_type: string | null;
	max_tokens: number;
	attempt_count: number;
	scheduler_wait_ms: number;
	first_token_wait_ms: number | null;
	duration_ms: number | null;
	input_tokens: number | null;
	output_tokens: number | null;
	cached_input_tokens: number | null;
	total_tokens: number | null;
	created_at: string;
	started_at: string | null;
	finished_at: string | null;
	updated_at: string;
};
export type AdminLlmCallDetailResponse = AdminLlmCallItem & {
	input_messages_json: string | null;
	output_messages_json: string | null;
	prompt_text: string;
	response_text: string | null;
	error_text: string | null;
};
export type AdminLlmCallsResponse = {
	items: AdminLlmCallItem[];
	page: number;
	page_size: number;
	total: number;
};
export async function apiGetAdminUserProfile(
	userId: LocalUserId,
): Promise<AdminUserProfileResponse> {
	return apiGet<AdminUserProfileResponse>(
		`/api/admin/users/${encodeURIComponent(userId)}/profile`,
	);
}
export async function apiGetAdminJobsOverview(): Promise<AdminJobsOverviewResponse> {
	return apiGet<AdminJobsOverviewResponse>("/api/admin/jobs/overview");
}
export async function apiGetAdminRealtimeTasks(
	params: URLSearchParams,
): Promise<AdminRealtimeTasksResponse> {
	return apiGet<AdminRealtimeTasksResponse>(
		`/api/admin/jobs/realtime?${params.toString()}`,
	);
}
export async function apiGetAdminRealtimeTaskDetail(
	taskId: string,
): Promise<AdminRealtimeTaskDetailResponse> {
	return apiGet<AdminRealtimeTaskDetailResponse>(
		`/api/admin/jobs/realtime/${encodeURIComponent(taskId)}`,
	);
}
export function buildAdminRealtimeTaskLogDownloadPath(taskId: string): string {
	return `/api/admin/jobs/realtime/${encodeURIComponent(taskId)}/log`;
}
export async function apiRetryAdminRealtimeTask(
	taskId: string,
): Promise<AdminTaskActionResponse> {
	return apiPostJson<AdminTaskActionResponse>(
		`/api/admin/jobs/realtime/${encodeURIComponent(taskId)}/retry`,
	);
}
export async function apiCancelAdminRealtimeTask(
	taskId: string,
): Promise<AdminTaskActionResponse> {
	return apiPostJson<AdminTaskActionResponse>(
		`/api/admin/jobs/realtime/${encodeURIComponent(taskId)}/cancel`,
	);
}
export function apiOpenAdminJobsEventsStream(): EventSource {
	return new EventSource("/api/admin/jobs/events", { withCredentials: true });
}
export async function apiGetAdminScheduledSlots(): Promise<AdminScheduledSlotsResponse> {
	return apiGet<AdminScheduledSlotsResponse>("/api/admin/jobs/scheduled");
}
export async function apiPatchAdminScheduledSlot(
	hourUtc: number,
	enabled: boolean,
): Promise<AdminScheduledSlotItem> {
	return apiPatchJson<AdminScheduledSlotItem>(
		`/api/admin/jobs/scheduled/${hourUtc}`,
		{ enabled },
	);
}
export async function apiGetAdminLlmSchedulerStatus(): Promise<AdminLlmSchedulerStatusResponse> {
	return apiGet<AdminLlmSchedulerStatusResponse>("/api/admin/jobs/llm/status");
}
export async function apiGetAdminLlmCalls(
	params: URLSearchParams,
): Promise<AdminLlmCallsResponse> {
	return apiGet<AdminLlmCallsResponse>(
		`/api/admin/jobs/llm/calls?${params.toString()}`,
	);
}
export async function apiGetAdminLlmCallDetail(
	callId: string,
): Promise<AdminLlmCallDetailResponse> {
	return apiGet<AdminLlmCallDetailResponse>(
		`/api/admin/jobs/llm/calls/${encodeURIComponent(callId)}`,
	);
}
export type ReleaseDetailTranslated = {
	lang: string;
	status: "ready" | "missing" | "disabled";
	title: string | null;
	summary: string | null;
};
export type ReleaseDetailResponse = {
	release_id: string;
	repo_full_name: string | null;
	tag_name: string;
	name: string | null;
	body: string | null;
	html_url: string;
	published_at: string | null;
	is_prerelease: number;
	is_draft: number;
	translated: ReleaseDetailTranslated | null;
};
export async function apiGetReleaseDetail(
	releaseId: string,
): Promise<ReleaseDetailResponse> {
	return apiGet<ReleaseDetailResponse>(
		`/api/releases/${encodeURIComponent(releaseId)}/detail`,
	);
}
export type TranslationSourceBlock = {
	slot: "title" | "excerpt" | "body_markdown" | "metadata";
	text: string;
};
export type TranslationRequestItemInput = {
	producer_ref: string;
	kind: "release_summary" | "release_detail" | "notification";
	variant: string;
	entity_id: string;
	target_lang: string;
	max_wait_ms: number;
	source_blocks: TranslationSourceBlock[];
	target_slots: Array<"title_zh" | "summary_md" | "body_md">;
};
export type TranslationAsyncSingleSubmitRequest = {
	mode: "async";
	item: TranslationRequestItemInput;
	items?: never;
};
export type TranslationWaitSubmitRequest = {
	mode: "wait";
	item: TranslationRequestItemInput;
	items?: never;
};
export type TranslationStreamSubmitRequest = {
	mode: "stream";
	item: TranslationRequestItemInput;
	items?: never;
};
export type TranslationSingleSubmitRequest =
	| TranslationAsyncSingleSubmitRequest
	| TranslationWaitSubmitRequest
	| TranslationStreamSubmitRequest;
export type TranslationBatchSubmitRequest = {
	mode: "async";
	items: TranslationRequestItemInput[];
	item?: never;
};
export type TranslationSubmitRequest =
	| TranslationSingleSubmitRequest
	| TranslationBatchSubmitRequest;
export type TranslationResultItem = {
	producer_ref: string;
	entity_id: string;
	kind: string;
	variant: string;
	status: "queued" | "running" | "ready" | "disabled" | "missing" | "error";
	title_zh: string | null;
	summary_md: string | null;
	body_md: string | null;
	error: string | null;
	work_item_id: string | null;
	batch_id: string | null;
};
export type TranslationRequestResponse = {
	request_id: string;
	status: "queued" | "running" | "completed" | "failed";
	result: TranslationResultItem;
};
export type TranslationBatchSubmitItemResponse = {
	request_id: string;
	status: "queued" | "running" | "completed" | "failed";
	producer_ref: string;
	entity_id: string;
	kind: string;
	variant: string;
};
export type TranslationBatchSubmitResponse = {
	requests: TranslationBatchSubmitItemResponse[];
};
export type TranslationRequestStreamEvent = {
	event: "queued" | "batched" | "running" | "completed" | "failed";
	request_id: string;
	status: "queued" | "running" | "completed" | "failed";
	batch_id?: string | null;
	result?: TranslationResultItem | null;
	error?: string | null;
};
export type AdminTranslationWorkerStatus = {
	worker_id: string;
	worker_slot: number;
	worker_kind: "general" | "user_dedicated";
	status: "idle" | "running" | "error";
	current_batch_id: string | null;
	request_count: number;
	work_item_count: number;
	trigger_reason: string | null;
	updated_at: string;
	error_text: string | null;
};
export type AdminTranslationStatusResponse = {
	scheduler_enabled: boolean;
	llm_enabled: boolean;
	scan_interval_ms: number;
	batch_token_threshold: number;
	worker_concurrency: number;
	idle_workers: number;
	busy_workers: number;
	workers: AdminTranslationWorkerStatus[];
	queued_requests: number;
	queued_work_items: number;
	running_batches: number;
	requests_24h: number;
	completed_batches_24h: number;
	failed_batches_24h: number;
	avg_wait_ms_24h: number | null;
	last_batch_finished_at: string | null;
};
export type AdminTranslationRequestListItem = {
	id: string;
	status: string;
	source: string;
	request_origin: "user" | "system" | string;
	requested_by: LocalUserId | null;
	scope_user_id: LocalUserId;
	producer_ref: string;
	kind: string;
	variant: string;
	entity_id: string;
	batch_id: string | null;
	created_at: string;
	started_at: string | null;
	finished_at: string | null;
	updated_at: string;
};
export type AdminTranslationRequestsResponse = {
	items: AdminTranslationRequestListItem[];
	page: number;
	page_size: number;
	total: number;
};
export type AdminTranslationRequestDetailResponse = {
	request: AdminTranslationRequestListItem;
	result: TranslationResultItem;
};
export type AdminTranslationBatchListItem = {
	id: string;
	status: string;
	trigger_reason: string;
	worker_slot: number;
	request_count: number;
	item_count: number;
	estimated_input_tokens: number;
	created_at: string;
	started_at: string | null;
	finished_at: string | null;
	updated_at: string;
};
export type AdminTranslationBatchesResponse = {
	items: AdminTranslationBatchListItem[];
	page: number;
	page_size: number;
	total: number;
};
export type AdminTranslationLinkedLlmCall = {
	id: string;
	status: string;
	source: string;
	model: string;
	scheduler_wait_ms: number;
	duration_ms: number | null;
	created_at: string;
};
export type AdminTranslationBatchDetailResponse = {
	batch: AdminTranslationBatchListItem;
	items: TranslationResultItem[];
	llm_calls: AdminTranslationLinkedLlmCall[];
};
export async function apiSubmitTranslationRequest(
	body: TranslationSingleSubmitRequest,
): Promise<TranslationRequestResponse>;
export async function apiSubmitTranslationRequest(
	body: TranslationBatchSubmitRequest,
): Promise<TranslationBatchSubmitResponse>;
export async function apiSubmitTranslationRequest(
	body: TranslationSubmitRequest,
): Promise<TranslationRequestResponse | TranslationBatchSubmitResponse> {
	return apiPostJson<
		TranslationRequestResponse | TranslationBatchSubmitResponse
	>("/api/translate/requests", body);
}
export async function apiOpenTranslationRequestStream(
	body: TranslationStreamSubmitRequest,
): Promise<Response> {
	const res = await fetch("/api/translate/requests", {
		method: "POST",
		credentials: "include",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(body),
	});
	if (!res.ok) {
		throw toApiError(res, await parseJson(res));
	}
	return res;
}
export async function apiGetTranslationRequest(
	requestId: string,
): Promise<TranslationRequestResponse> {
	return apiGet<TranslationRequestResponse>(
		`/api/translate/requests/${encodeURIComponent(requestId)}`,
	);
}
export async function apiGetAdminTranslationStatus(): Promise<AdminTranslationStatusResponse> {
	return apiGet<AdminTranslationStatusResponse>(
		"/api/admin/jobs/translations/status",
	);
}
export async function apiGetAdminTranslationRequests(
	params: URLSearchParams,
): Promise<AdminTranslationRequestsResponse> {
	return apiGet<AdminTranslationRequestsResponse>(
		`/api/admin/jobs/translations/requests?${params.toString()}`,
	);
}
export async function apiGetAdminTranslationRequestDetail(
	requestId: string,
): Promise<AdminTranslationRequestDetailResponse> {
	return apiGet<AdminTranslationRequestDetailResponse>(
		`/api/admin/jobs/translations/requests/${encodeURIComponent(requestId)}`,
	);
}
export async function apiGetAdminTranslationBatches(
	params: URLSearchParams,
): Promise<AdminTranslationBatchesResponse> {
	return apiGet<AdminTranslationBatchesResponse>(
		`/api/admin/jobs/translations/batches?${params.toString()}`,
	);
}
export async function apiGetAdminTranslationBatchDetail(
	batchId: string,
): Promise<AdminTranslationBatchDetailResponse> {
	return apiGet<AdminTranslationBatchDetailResponse>(
		`/api/admin/jobs/translations/batches/${encodeURIComponent(batchId)}`,
	);
}
export async function apiTranslateReleaseDetail(
	detail: ReleaseDetailResponse,
): Promise<ReleaseDetailTranslated> {
	const originalTitle = detail.name?.trim() || detail.tag_name;
	const body = detail.body?.trim();
	const metadata = [detail.repo_full_name, detail.published_at]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	const source_blocks: TranslationSourceBlock[] = [
		{ slot: "title", text: originalTitle },
		...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
		...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
	];
	const response = await apiSubmitTranslationRequest({
		mode: "wait",
		item: {
			producer_ref: `release_detail:${detail.release_id}`,
			kind: "release_detail",
			variant: "detail_card",
			entity_id: detail.release_id,
			target_lang: "zh-CN",
			max_wait_ms: 5_000,
			source_blocks,
			target_slots: ["title_zh", "body_md"],
		},
	});
	const translated = response.result;
	if (translated.status !== "ready" && translated.status !== "disabled") {
		throw new ApiError(
			500,
			translated.error ?? "translate failed",
			"translate_failed",
		);
	}
	return {
		lang: "zh-CN",
		status: translated.status === "disabled" ? "disabled" : "ready",
		title: translated.title_zh,
		summary: translated.body_md,
	};
}
