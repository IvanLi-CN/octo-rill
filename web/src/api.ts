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

export type AdminUserProfileResponse = {
	user_id: number;
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
	requested_by: number | null;
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
	id: number;
	event_type: string;
	payload_json: string;
	created_at: string;
};

export type AdminRealtimeTaskDetailItem = AdminRealtimeTaskItem & {
	payload_json?: string | null;
	result_json?: string | null;
};

export type AdminRealtimeTaskDetailResponse = {
	task: AdminRealtimeTaskDetailItem;
	events: AdminTaskEventItem[];
};

export type AdminTaskActionResponse = {
	task_id: string;
	status: string;
};

export type AdminJobsStreamEvent = {
	event_id: number;
	task_id: string;
	task_type: string;
	status: string;
	event_type: string;
	created_at: string;
};

export type AdminLlmCallStreamEvent = {
	event_id: number;
	call_id: string;
	status: string;
	source: string;
	requested_by: number | null;
	parent_task_id: string | null;
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
	requested_by: number | null;
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
	userId: number,
): Promise<AdminUserProfileResponse> {
	return apiGet<AdminUserProfileResponse>(`/api/admin/users/${userId}/profile`);
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

export async function apiTranslateReleaseDetail(
	releaseId: string,
): Promise<ReleaseDetailTranslated> {
	return apiPostJson<ReleaseDetailTranslated>("/api/translate/release/detail", {
		release_id: releaseId,
	});
}
