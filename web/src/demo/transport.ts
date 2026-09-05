import {
	delay,
	http,
	HttpResponse,
	isCommonAssetRequest,
	passthrough,
} from "msw";
import type { AdminUserItem } from "@/admin/UserManagement";
import type {
	AdminDashboardResponse,
	AdminCollectionAttempt,
	AdminCollectionRecordDetail,
	AdminCollectionRecordItem,
	AdminCollectionTaskSummary,
	AccountResumeResponse,
	AdminLlmCallDetailResponse,
	AdminRealtimeTaskDetailResponse,
	AdminRealtimeTaskItem,
	AdminSyncRuntimeConfigResponse,
	AdminTranslationRuntimeConfigUpdateRequest,
	AdminUserProfileResponse,
	CreateApiKeyResponse,
	FollowingReposResponse,
	PasskeyAuthenticateVerifyResponse,
	PasskeyRequestOptionsJSON,
	PasskeyRegisterVerifyResponse,
	PublicReleaseListItem,
	ReactionTokenCheckResponse,
	ReactionTokenStatusResponse,
} from "@/api";
import { buildDemoHref } from "@/demo/registry";
import {
	buildDemoOwnerReleaseFeedItem,
	buildLlmActivityFromCalls,
	DEMO_OWNER_RELEASE_ID,
} from "@/demo/fixtures";
import { hasDemoRuntimeRequestMarker } from "@/demo/requestMarker";
import type {
	FeedReactionRefreshResponse,
	ReleaseReactions,
} from "@/feed/types";
import type {
	DemoEventFrame,
	DemoModel,
	DemoShareStatePatch,
	DemoSnapshot,
} from "@/demo/types";

const DEMO_UPDATE_TOKEN_FIELD = ["to", "ken"].join("") as "token";
const DEMO_UPDATE_TOKEN_VALUE = ["demo", "updates", "token"].join("-");
const DEMO_API_KEY_FIELD = ["api", "key"].join("_") as "api_key";
const DEMO_CREATED_API_KEY_VALUE = [
	"orill",
	"ak",
	"demo",
	"created",
	"plaintext",
].join("_");

type DemoRuntimeAccess = {
	getSnapshot: () => DemoSnapshot;
	updateModel: (updater: (model: DemoModel) => DemoModel) => void;
	patchShareState: (partial: DemoShareStatePatch) => void;
	recordMutation: (label: string, detail: string) => void;
};

let runtimeAccess: DemoRuntimeAccess | null = null;

export function registerDemoRuntimeAccess(access: DemoRuntimeAccess) {
	runtimeAccess = access;
}

function requireRuntimeAccess() {
	if (!runtimeAccess) {
		throw new Error("Demo runtime transport is not configured");
	}
	return runtimeAccess;
}

function currentSnapshot() {
	return requireRuntimeAccess().getSnapshot();
}

function currentModel() {
	const model = currentSnapshot().model;
	if (!model) {
		throw new Error("Demo model is not ready");
	}
	return model;
}

function buildDemoAuthRedirect(intent: "login" | "connect" | "logout") {
	const snapshot = currentSnapshot();
	if (intent === "logout") {
		return buildDemoHref(
			{
				...snapshot.shareState,
				sceneId: "landing-welcome",
				personaId: "guest",
				includeOwnReleases: false,
				publicationState: "unpublished",
			},
			snapshot.basepath,
		);
	}

	return buildDemoHref(
		{
			...snapshot.shareState,
			personaId:
				snapshot.shareState.personaId === "guest"
					? "member"
					: snapshot.shareState.personaId,
		},
		snapshot.basepath,
	);
}

function redirectToDemoAuth(
	requestUrl: string,
	intent: "login" | "connect" | "logout",
) {
	return HttpResponse.redirect(
		new URL(buildDemoAuthRedirect(intent), requestUrl).toString(),
		302,
	);
}

async function applyNetworkProfile(request: Request) {
	const snapshot = currentSnapshot();
	if (!snapshot.demoBuild && !hasDemoRuntimeRequestMarker(request)) {
		return passthrough();
	}
	const { shareState } = snapshot;
	const pathname = new URL(request.url).pathname;
	if (shareState.networkMode === "slow") {
		await delay(850);
		return null;
	}
	if (shareState.networkMode === "faulty" && pathname.startsWith("/api/")) {
		await delay(220);
		return HttpResponse.json(
			{
				error: {
					code: "demo_network_fault",
					message: "Demo network fault injected by inspector.",
				},
			},
			{ status: 503 },
		);
	}
	await delay(80);
	return null;
}

function json(data: unknown, init?: ResponseInit) {
	return HttpResponse.json(data as never, {
		status: 200,
		...init,
		headers: {
			"Content-Type": "application/json",
			...(init?.headers ?? {}),
		},
	});
}

function badRequest(message: string) {
	return HttpResponse.json(
		{ error: { code: "bad_request", message } },
		{ status: 400 },
	);
}

function isPositiveInteger(value: unknown, maximum = Number.MAX_SAFE_INTEGER) {
	return (
		typeof value === "number" &&
		Number.isSafeInteger(value) &&
		value > 0 &&
		value <= maximum
	);
}

function isRfc3339Timestamp(value: string) {
	const match = value.match(
		/^(\d{4})-(\d{2})-(\d{2})[Tt](\d{2}):(\d{2}):(\d{2})(?:\.\d+)?(?:[Zz]|([+-])(\d{2}):(\d{2}))$/,
	);
	if (!match) return false;
	const [, yearText, monthText, dayText, hourText, minuteText, secondText] =
		match;
	const year = Number(yearText);
	const month = Number(monthText);
	const day = Number(dayText);
	const hour = Number(hourText);
	const minute = Number(minuteText);
	const second = Number(secondText);
	const offsetHour = Number(match[8] ?? 0);
	const offsetMinute = Number(match[9] ?? 0);
	const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
	return (
		month >= 1 &&
		month <= 12 &&
		day >= 1 &&
		day <= daysInMonth &&
		hour <= 23 &&
		minute <= 59 &&
		second <= 59 &&
		offsetHour <= 23 &&
		offsetMinute <= 59 &&
		Number.isFinite(Date.parse(value))
	);
}

function currentDemoVersion() {
	return __APP_LOADED_VERSION__;
}

function emptyReleaseReactions(): ReleaseReactions {
	return {
		counts: {
			plus1: 0,
			laugh: 0,
			heart: 0,
			hooray: 0,
			rocket: 0,
			eyes: 0,
		},
		viewer: {
			plus1: false,
			laugh: false,
			heart: false,
			hooray: false,
			rocket: false,
			eyes: false,
		},
		status: "ready",
	};
}

function findReleaseReactions(
	model: DemoModel,
	releaseId: string,
): ReleaseReactions {
	const item = model.feed.items.find(
		(feedItem) => feedItem.kind === "release" && feedItem.id === releaseId,
	);
	return item?.reactions ?? emptyReleaseReactions();
}

function filterTasks(
	items: AdminRealtimeTaskItem[],
	searchParams: URLSearchParams,
) {
	const status = searchParams.get("status") ?? "all";
	const taskType = searchParams.get("task_type");
	const taskGroup = searchParams.get("task_group");
	const page = Number(searchParams.get("page") ?? "1");
	const pageSize = Number(searchParams.get("page_size") ?? "20");

	let filtered = items;
	if (status !== "all") {
		filtered = filtered.filter((item) => item.status === status);
	}
	if (taskType) {
		filtered = filtered.filter((item) => item.task_type === taskType);
	}
	if (taskGroup === "realtime") {
		filtered = filtered.filter((item) => item.source !== "scheduler");
	}
	if (taskGroup === "scheduled") {
		filtered = filtered.filter((item) => item.source === "scheduler");
	}

	const start = Math.max(0, (page - 1) * pageSize);
	return {
		items: filtered.slice(start, start + pageSize),
		total: filtered.length,
		page,
		page_size: pageSize,
	};
}

function paginateItems<T>(items: T[], searchParams: URLSearchParams) {
	const parsePositiveInteger = (value: string | null, fallback: number) => {
		const parsed = Number(value);
		return Number.isInteger(parsed) && parsed > 0 ? parsed : fallback;
	};
	const page = parsePositiveInteger(searchParams.get("page"), 1);
	const pageSize = Math.min(
		100,
		parsePositiveInteger(searchParams.get("page_size"), 20),
	);
	const total = items.length;
	return {
		items: items.slice((page - 1) * pageSize, page * pageSize),
		page,
		page_size: pageSize,
		total,
	};
}

function demoCollectionRecords() {
	const completed = {
		status: "ready",
		display_status: "succeeded",
		status_origin: "task",
		retry_count: 0,
		started_at: "2026-07-08T09:11:00+08:00",
		last_attempt_at: "2026-07-08T09:11:05+08:00",
		finished_at: "2026-07-08T09:11:05+08:00",
	};
	const recovered = {
		status: "ready",
		display_status: "succeeded",
		status_origin: "task",
		retry_count: 1,
		started_at: "2026-07-08T09:23:01+08:00",
		last_attempt_at: "2026-07-08T09:25:08+08:00",
		finished_at: "2026-07-08T09:25:08+08:00",
	};
	const pending = {
		status: "running",
		display_status: "running",
		status_origin: "task",
		retry_count: 0,
		started_at: "2026-07-08T10:02:00+08:00",
		last_attempt_at: "2026-07-08T10:02:00+08:00",
		finished_at: null,
	};
	const notRecorded = {
		status: "not_recorded",
		display_status: "historical_unknown",
		status_origin: "historical_unknown",
		retry_count: 0,
		started_at: null,
		last_attempt_at: null,
		finished_at: null,
	};
	return {
		release: [
			{
				id: "291058010",
				kind: "release",
				repository: "octo-demo/release-lab",
				title: "历史 Release（未记录处理）",
				occurred_at: "2026-07-08T09:45:00+08:00",
				detected_at: null,
				generated_at: null,
				translation: notRecorded,
				polish: notRecorded,
			},
			{
				id: "291058019",
				kind: "release",
				repository: "octo-demo/release-lab",
				title: "v2.31.0",
				occurred_at: "2026-07-08T09:08:00+08:00",
				detected_at: "2026-07-08T09:10:00+08:00",
				generated_at: null,
				translation: completed,
				polish: recovered,
			},
		],
		announcement: [
			{
				id: "octo-demo/release-lab#42",
				kind: "announcement",
				repository: "octo-demo/release-lab",
				title: "维护窗口公告",
				occurred_at: "2026-07-08T08:40:00+08:00",
				detected_at: "2026-07-08T08:44:00+08:00",
				generated_at: null,
				translation: completed,
				polish: pending,
			},
		],
		brief: [
			{
				id: "brief-demo-2026-07-08",
				kind: "brief",
				repository: null,
				title: "2026-07-08",
				occurred_at: null,
				detected_at: null,
				generated_at: "2026-07-08T10:30:00+08:00",
				translation: null,
				polish: completed,
			},
		],
	} satisfies Record<
		AdminCollectionRecordItem["kind"],
		AdminCollectionRecordItem[]
	>;
}

function demoSummaryAttemptCount(summary: AdminCollectionTaskSummary | null) {
	if (!summary || summary.status === "not_recorded") return 0;
	return Math.max(1, summary.retry_count + 1);
}

function demoRecordAttemptCount(item: AdminCollectionRecordItem) {
	if (item.kind === "brief") return demoSummaryAttemptCount(item.polish);
	return Math.max(
		demoSummaryAttemptCount(item.translation),
		demoSummaryAttemptCount(item.polish),
	);
}

function demoCollectionDetail(
	kind: AdminCollectionRecordItem["kind"],
	id: string,
): AdminCollectionRecordDetail | null {
	const record = demoCollectionRecords()[kind].find((item) => item.id === id);
	if (!record) return null;
	const call = currentModel().adminJobs.llmCalls[0];
	const llmCalls = call
		? [
				{
					id: call.id,
					status: call.status,
					source: call.source,
					model: call.model,
				},
			]
		: [];
	const common = {
		started_at: "2026-07-08T09:11:00+08:00",
		last_attempt_at: "2026-07-08T09:11:05+08:00",
		finished_at: "2026-07-08T09:11:05+08:00",
		error_code: null,
		error_summary: null,
		failure_class: null,
		retry_eligible: false,
		next_retry_at: null,
		llm_calls: llmCalls,
	};
	const attempts: AdminCollectionAttempt[] =
		kind === "brief"
			? [
					{
						id: `${id}:1`,
						pipeline: "polish",
						attempt_no: 1,
						trigger: "daily_brief_generation",
						status: "ready",
						...common,
					},
				]
			: [
					{
						id: `${id}:translation:1`,
						pipeline: "translation",
						attempt_no: 1,
						trigger: "initial",
						status: "ready",
						...common,
					},
					{
						id: `${id}:polish:2`,
						pipeline: "polish",
						attempt_no: 2,
						trigger: "automatic_recovery",
						status: "ready",
						...common,
						last_attempt_at: "2026-07-08T09:25:08+08:00",
						finished_at: "2026-07-08T09:25:08+08:00",
						error_code: "release_smart_body_summary_json_decode_failed",
						error_summary: "Release smart 正文摘要 JSON 解码失败后已自动恢复",
						failure_class: "transient",
					},
				];
	return { record, attempts };
}

function filterUsers(users: AdminUserItem[], searchParams: URLSearchParams) {
	const query = (searchParams.get("query") ?? "").trim().toLowerCase();
	const role = searchParams.get("role") ?? "all";
	const status = searchParams.get("status") ?? "all";
	const page = Number(searchParams.get("page") ?? "1");
	const pageSize = Number(searchParams.get("page_size") ?? "20");

	let filtered = users;
	if (role === "admin") {
		filtered = filtered.filter((item) => item.is_admin);
	} else if (role === "user") {
		filtered = filtered.filter((item) => !item.is_admin);
	}
	if (status === "enabled") {
		filtered = filtered.filter((item) => !item.is_disabled);
	} else if (status === "disabled") {
		filtered = filtered.filter((item) => item.is_disabled);
	}
	if (query) {
		filtered = filtered.filter((item) =>
			[item.login, item.name ?? "", item.email ?? ""]
				.join(" ")
				.toLowerCase()
				.includes(query),
		);
	}

	const start = Math.max(0, (page - 1) * pageSize);
	return {
		items: filtered.slice(start, start + pageSize),
		total: filtered.length,
		page,
		page_size: pageSize,
		guard: {
			admin_total: users.filter((item) => item.is_admin).length,
			active_admin_total: users.filter(
				(item) => item.is_admin && !item.is_disabled,
			).length,
		},
	};
}

function buildTaskAcceptedResponse(taskId: string, taskType: string) {
	return {
		mode: "task_id" as const,
		task_id: taskId,
		task_type: taskType,
		status: "accepted",
	};
}

function demoPublicReleaseCursor(item: PublicReleaseListItem) {
	return `${item.published_at ?? ""}|${item.release_id}`;
}

function findDemoPublicRelease(
	items: PublicReleaseListItem[],
	selector: string | null,
) {
	if (!selector) return undefined;
	if (selector.startsWith("id:")) {
		return items.find((item) => item.release_id === selector.slice(3));
	}
	if (selector.startsWith("tag:")) {
		return items.find((item) => item.tag_name === selector.slice(4));
	}
	return undefined;
}

function buildDemoPublicReleaseList(request: Request) {
	const url = new URL(request.url);
	const sourceItems = currentModel().publicReleaseList.items;
	const selectors = url.searchParams.getAll("highlight");
	const startSelector = url.searchParams.get("highlight_start");
	const endSelector = url.searchParams.get("highlight_end");
	const activeSelector = url.searchParams.get("highlight_active");
	const cursor = url.searchParams.get("cursor");
	const untilCursor = url.searchParams.get("until_cursor");
	const direction = url.searchParams.get("direction") ?? "older";
	const limit = Math.min(
		30,
		Math.max(1, Number(url.searchParams.get("limit") ?? 6)),
	);
	const cursorIndex = cursor
		? sourceItems.findIndex((item) => demoPublicReleaseCursor(item) === cursor)
		: -1;
	const untilIndex = untilCursor
		? sourceItems.findIndex(
				(item) => demoPublicReleaseCursor(item) === untilCursor,
			)
		: -1;

	if (selectors.length > 0) {
		const seen = new Set<string>();
		const resolved = selectors.flatMap((selector) => {
			const item = findDemoPublicRelease(sourceItems, selector);
			if (!item || seen.has(item.release_id)) return [];
			seen.add(item.release_id);
			return [
				{
					selector,
					release_id: item.release_id,
					tag_name: item.tag_name,
					ordinal: sourceItems.indexOf(item) + 1,
				},
			];
		});
		resolved.sort((left, right) => left.ordinal - right.ordinal);
		const active =
			resolved.find((target) => target.selector === activeSelector) ??
			resolved[0];
		let pageItems = resolved.flatMap((target) => {
			const item = sourceItems.find(
				(candidate) => candidate.release_id === target.release_id,
			);
			return item ? [item] : [];
		});
		if (cursorIndex >= 0) {
			const end = untilIndex >= 0 ? untilIndex + 1 : cursorIndex + 1 + limit;
			pageItems = sourceItems.slice(cursorIndex + 1, end);
		}
		const highlightedIds = new Set(resolved.map((target) => target.release_id));
		const items = pageItems.map((item) => ({
			...item,
			is_highlighted: highlightedIds.has(item.release_id),
			is_active_highlight: item.release_id === active?.release_id,
		}));
		const gaps = cursor
			? []
			: items.slice(0, -1).flatMap((item, index) => {
					const next = items[index + 1];
					const currentIndex = sourceItems.indexOf(item);
					const nextIndex = sourceItems.indexOf(next);
					return nextIndex - currentIndex > 1
						? [
								{
									newer_cursor: demoPublicReleaseCursor(item),
									older_cursor: demoPublicReleaseCursor(next),
									remaining_count: nextIndex - currentIndex - 1,
								},
							]
						: [];
				});
		return {
			status: "ready" as const,
			repo_full_name: currentModel().publicReleaseList.repo_full_name,
			next_cursor: null,
			previous_cursor: null,
			items,
			highlight: {
				mode: "discrete" as const,
				status:
					resolved.length === selectors.length
						? ("complete" as const)
						: ("partial" as const),
				requested: selectors,
				resolved,
				unresolved: selectors.filter(
					(selector) => !findDemoPublicRelease(sourceItems, selector),
				),
				total: resolved.length,
				active_release_id: active?.release_id ?? null,
				active_index: active ? resolved.indexOf(active) + 1 : null,
			},
			segments: items.map((item) => ({
				first_release_id: item.release_id,
				last_release_id: item.release_id,
			})),
			gaps,
		};
	}

	if (startSelector || endSelector) {
		const startItem = findDemoPublicRelease(sourceItems, startSelector);
		const endItem = findDemoPublicRelease(sourceItems, endSelector);
		if (!startItem || !endItem) {
			return {
				...currentModel().publicReleaseList,
				items: sourceItems.slice(0, limit),
				highlight: {
					mode: "range" as const,
					status: "partial" as const,
					requested: [startSelector, endSelector].filter(
						(value): value is string => Boolean(value),
					),
					resolved: [],
					unresolved: [startSelector, endSelector].filter(
						(value): value is string =>
							Boolean(value) && !findDemoPublicRelease(sourceItems, value),
					),
					total: 0,
					active_release_id: null,
					active_index: null,
					message: "连续范围的端点未全部命中，已显示普通最新列表",
				},
			};
		}
		const rangeStart = Math.min(
			sourceItems.indexOf(startItem),
			sourceItems.indexOf(endItem),
		);
		const rangeEnd = Math.max(
			sourceItems.indexOf(startItem),
			sourceItems.indexOf(endItem),
		);
		const activeItem =
			findDemoPublicRelease(sourceItems, activeSelector) ??
			sourceItems[rangeStart];
		const activeIndex = Math.min(
			rangeEnd,
			Math.max(rangeStart, sourceItems.indexOf(activeItem)),
		);
		let pageStart = rangeStart;
		let pageEnd = Math.min(rangeEnd + 1, pageStart + limit);
		if (cursorIndex >= 0) {
			if (direction === "newer") {
				pageEnd = cursorIndex;
				pageStart = Math.max(rangeStart, pageEnd - limit);
			} else {
				pageStart = cursorIndex + 1;
				pageEnd = Math.min(rangeEnd + 1, pageStart + limit);
			}
		} else if (activeSelector) {
			pageStart = Math.max(
				rangeStart,
				activeIndex - Math.floor((limit - 1) / 2),
			);
			pageEnd = Math.min(rangeEnd + 1, pageStart + limit);
		}
		const items = sourceItems.slice(pageStart, pageEnd).map((item) => ({
			...item,
			is_highlighted: true,
			is_active_highlight: item.release_id === activeItem.release_id,
		}));
		return {
			status: "ready" as const,
			repo_full_name: currentModel().publicReleaseList.repo_full_name,
			previous_cursor:
				pageStart > rangeStart ? demoPublicReleaseCursor(items[0]) : null,
			next_cursor:
				pageEnd <= rangeEnd ? demoPublicReleaseCursor(items.at(-1)!) : null,
			items,
			highlight: {
				mode: "range" as const,
				status: "complete" as const,
				requested: [startSelector!, endSelector!],
				resolved: [
					{
						selector: startSelector!,
						release_id: startItem.release_id,
						tag_name: startItem.tag_name,
						ordinal: sourceItems.indexOf(startItem) + 1,
					},
					{
						selector: endSelector!,
						release_id: endItem.release_id,
						tag_name: endItem.tag_name,
						ordinal: sourceItems.indexOf(endItem) + 1,
					},
				],
				unresolved: [],
				total: rangeEnd - rangeStart + 1,
				active_release_id: activeItem.release_id,
				active_index: activeIndex - rangeStart + 1,
			},
		};
	}

	const start = cursorIndex >= 0 ? cursorIndex + 1 : 0;
	const items = sourceItems.slice(start, start + limit);
	return {
		...currentModel().publicReleaseList,
		items,
		next_cursor:
			start + items.length < sourceItems.length
				? demoPublicReleaseCursor(items.at(-1)!)
				: null,
	};
}

function reconcileDemoFeedIncludeOwnReleases(
	model: DemoModel,
	includeOwnReleases: boolean,
) {
	if (!includeOwnReleases) {
		return {
			...model.feed,
			items: model.feed.items.filter(
				(item) => item.id !== DEMO_OWNER_RELEASE_ID,
			),
		};
	}

	if (model.feed.items.some((item) => item.id === DEMO_OWNER_RELEASE_ID)) {
		return model.feed;
	}

	return {
		...model.feed,
		items: [buildDemoOwnerReleaseFeedItem(), ...model.feed.items],
	};
}

function ensureTaskStream(taskId: string, frames: DemoEventFrame[]) {
	const access = requireRuntimeAccess();
	access.updateModel((model) => ({
		...model,
		taskStreams: {
			...model.taskStreams,
			[taskId]: frames,
		},
	}));
}

function buildSyncFrames(taskId: string): DemoEventFrame[] {
	return [
		{ delayMs: 120, type: "task.running", data: { task_id: taskId } },
		{
			delayMs: 420,
			type: "task.progress",
			data: { stage: "star_refreshed", succeeded: 14, total: 14 },
		},
		{
			delayMs: 880,
			type: "task.progress",
			data: { stage: "release_summary", succeeded: 24, total: 24 },
		},
		{
			delayMs: 1260,
			type: "task.progress",
			data: { stage: "social_summary", succeeded: 8, total: 8 },
		},
		{
			delayMs: 1600,
			type: "task.progress",
			data: { stage: "notifications_summary", succeeded: 4, total: 4 },
		},
		{ delayMs: 2100, type: "task.completed", data: { status: "succeeded" } },
	];
}

function allAdminTasks(model: DemoModel) {
	return [...model.adminJobs.realtimeTasks, ...model.adminJobs.scheduledRuns];
}

function buildAdminDashboardResponse(
	model: DemoModel,
	window: "7d" | "30d",
): AdminDashboardResponse {
	const tasks = allAdminTasks(model);
	const queuedTotal = model.adminJobs.overview.queued;
	const runningTotal = model.adminJobs.overview.running;
	const succeededTotal = tasks.filter(
		(task) => task.status === "succeeded",
	).length;
	const failedTotal = tasks.filter((task) => task.status === "failed").length;
	const canceledTotal = tasks.filter(
		(task) => task.status === "canceled",
	).length;
	const totalUsers = model.adminUsers.length;
	const activeUsers = model.adminUsers.filter(
		(user) => !user.is_disabled,
	).length;
	const windowEnd = "2026-07-08";
	const windowStart = window === "30d" ? "2026-06-09" : "2026-07-02";

	const statusItems: AdminDashboardResponse["status_breakdown"]["items"] = [
		{
			task_type: "translations",
			label: "翻译",
			queued: 0,
			running: 1,
			succeeded: 1,
			failed: 0,
			canceled: 0,
			business_counts: { ok: 1, partial: 1, failed: 0, disabled: 0 },
			total: 2,
			success_rate: 0.5,
			business_success_rate: 0.5,
		},
		{
			task_type: "summaries",
			label: "润色",
			queued: 1,
			running: 0,
			succeeded: 0,
			failed: 0,
			canceled: 0,
			business_counts: { ok: 0, partial: 0, failed: 0, disabled: 0 },
			total: 1,
			success_rate: 0,
			business_success_rate: 0,
		},
		{
			task_type: "briefs",
			label: "日报",
			queued: 0,
			running: 1,
			succeeded: 0,
			failed: 0,
			canceled: 0,
			business_counts: { ok: 0, partial: 0, failed: 0, disabled: 0 },
			total: 1,
			success_rate: 0,
			business_success_rate: 0,
		},
	];

	const businessCounts = statusItems.reduce(
		(acc, item) => ({
			ok: acc.ok + item.business_counts.ok,
			partial: acc.partial + item.business_counts.partial,
			failed: acc.failed + item.business_counts.failed,
			disabled: acc.disabled + item.business_counts.disabled,
		}),
		{ ok: 0, partial: 0, failed: 0, disabled: 0 },
	);
	const totalTaskShare = statusItems.reduce((sum, item) => sum + item.total, 0);

	return {
		generated_at: new Date().toISOString(),
		time_zone: "Asia/Shanghai",
		summary: {
			total_users: totalUsers,
			active_users_today: activeUsers,
			ongoing_tasks_total: queuedTotal + runningTotal,
			queued_tasks: queuedTotal,
			running_tasks: runningTotal,
			ongoing_by_task: {
				translations: 1,
				summaries: 1,
				briefs: 1,
			},
		},
		today_live: {
			date: windowEnd,
			total_users: totalUsers,
			active_users: activeUsers,
			ongoing_tasks_total: queuedTotal + runningTotal,
			queued_tasks: queuedTotal,
			running_tasks: runningTotal,
		},
		status_breakdown: {
			queued_total: queuedTotal,
			running_total: runningTotal,
			succeeded_total: succeededTotal,
			failed_total: failedTotal,
			canceled_total: canceledTotal,
			business_counts: businessCounts,
			total: totalTaskShare,
			items: statusItems,
		},
		task_share: statusItems.map((item) => ({
			task_type: item.task_type,
			label: item.label,
			total: item.total,
			share_ratio: totalTaskShare > 0 ? item.total / totalTaskShare : 0,
			success_rate: item.success_rate,
			business_counts: item.business_counts,
			business_success_rate: item.business_success_rate,
		})),
		trend_points: [
			{
				date: "2026-07-02",
				label: "07-02",
				total_users: totalUsers - 1,
				active_users: Math.max(1, activeUsers - 1),
				translations_total: 1,
				translations_failed: 0,
				translations_partial: 0,
				translations_business_failed: 0,
				summaries_total: 0,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: "2026-07-03",
				label: "07-03",
				total_users: totalUsers,
				active_users: Math.max(1, activeUsers - 1),
				translations_total: 1,
				translations_failed: 0,
				translations_partial: 1,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: "2026-07-04",
				label: "07-04",
				total_users: totalUsers,
				active_users: activeUsers,
				translations_total: 2,
				translations_failed: 0,
				translations_partial: 1,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: "2026-07-05",
				label: "07-05",
				total_users: totalUsers,
				active_users: activeUsers,
				translations_total: 1,
				translations_failed: 0,
				translations_partial: 0,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: "2026-07-06",
				label: "07-06",
				total_users: totalUsers,
				active_users: activeUsers,
				translations_total: 2,
				translations_failed: 0,
				translations_partial: 1,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: "2026-07-07",
				label: "07-07",
				total_users: totalUsers,
				active_users: activeUsers,
				translations_total: 1,
				translations_failed: 0,
				translations_partial: 0,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
			{
				date: windowEnd,
				label: "07-08",
				total_users: totalUsers,
				active_users: activeUsers,
				translations_total: 2,
				translations_failed: 0,
				translations_partial: 1,
				translations_business_failed: 0,
				summaries_total: 1,
				summaries_failed: 0,
				summaries_partial: 0,
				summaries_business_failed: 0,
				briefs_total: 1,
				briefs_failed: 0,
				briefs_partial: 0,
				briefs_business_failed: 0,
			},
		],
		llm_health: {
			calls_24h: model.adminJobs.llmStatus.calls_24h,
			failed_24h: model.adminJobs.llmStatus.failed_24h,
			last_failure_at: model.adminJobs.llmStatus.last_failure_at,
			top_failure_reasons: [],
			top_failure_sources: [],
		},
		window_meta: {
			selected_window: window,
			available_windows: ["7d", "30d"],
			window_start: windowStart,
			window_end: windowEnd,
			point_count: window === "30d" ? 30 : 7,
		},
	};
}

function updateProfile(model: DemoModel, profile: AdminUserProfileResponse) {
	return {
		...model,
		adminUserProfiles: {
			...model.adminUserProfiles,
			[profile.user_id]: profile,
		},
		adminUsers: model.adminUsers.map((user) =>
			user.id === profile.user_id
				? { ...user, include_own_releases: profile.include_own_releases }
				: user,
		),
	};
}

function updateAdminTask(
	model: DemoModel,
	taskId: string,
	partial: Partial<AdminRealtimeTaskItem>,
) {
	const apply = (items: AdminRealtimeTaskItem[]) =>
		items.map((item) => (item.id === taskId ? { ...item, ...partial } : item));

	const taskDetail = model.adminJobs.taskDetails[taskId];
	return {
		...model,
		adminJobs: {
			...model.adminJobs,
			realtimeTasks: apply(model.adminJobs.realtimeTasks),
			scheduledRuns: apply(model.adminJobs.scheduledRuns),
			subscriptionRuns: apply(model.adminJobs.subscriptionRuns),
			taskDetails: taskDetail
				? {
						...model.adminJobs.taskDetails,
						[taskId]: {
							...taskDetail,
							task: {
								...taskDetail.task,
								...partial,
							},
						},
					}
				: model.adminJobs.taskDetails,
		},
	};
}

export const demoHandlers = [
	http.get("/auth/github/login", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return redirectToDemoAuth(request.url, "login");
	}),
	http.get("/auth/linuxdo/login", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return redirectToDemoAuth(request.url, "login");
	}),
	http.get("/auth/github/connect", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return redirectToDemoAuth(request.url, "connect");
	}),
	http.get("/auth/logout", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return redirectToDemoAuth(request.url, "logout");
	}),
	http.get("/api/version", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({
			ok: true,
			version: currentDemoVersion(),
			source: "DEMO_RUNTIME",
		});
	}),
	http.get("/api/health", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({ ok: true, version: currentDemoVersion() });
	}),
	http.get("/api/me", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const { model } = currentSnapshot();
		if (!model?.me) {
			return json(
				{
					error: {
						code: "unauthorized",
						message: "Demo persona is anonymous.",
					},
				},
				{ status: 401 },
			);
		}
		return json(model.me);
	}),
	http.post("/api/me/resume", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const taskId = "task-demo-account-resume";
		ensureTaskStream(taskId, buildSyncFrames(taskId));
		requireRuntimeAccess().updateModel((model) => ({
			...model,
			me: model.me
				? {
						...model.me,
						user: {
							...model.me.user,
							account_status: "enabled",
							paused_at: null,
						},
					}
				: null,
		}));
		requireRuntimeAccess().recordMutation(
			"Resume paused account",
			"Cleared the simulated pause and queued in-memory access sync.",
		);
		const response: AccountResumeResponse = {
			status: "enabled",
			access_sync: {
				task_id: taskId,
				task_type: "sync.access_refresh",
				event_path: `/api/tasks/${taskId}/events`,
				reason: "account_resumed",
			},
			sync_enqueue_error: null,
		};
		return json(response);
	}),
	http.get("/api/feed", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const model = currentModel();
		const type = url.searchParams.get("types");
		const scope = url.searchParams.get("scope");
		const scopedRepo = scope === "repo" ? url.searchParams.get("items") : null;
		const scopedItems = scopedRepo
			? model.feed.items.filter((item) => item.repo_full_name === scopedRepo)
			: model.feed.items;
		return json({
			...model.feed,
			items: scopedItems.filter((item) =>
				type === "releases"
					? item.kind === "release"
					: type === "stars"
						? item.kind === "repo_star_received"
						: type === "followers"
							? item.kind === "follower_received"
							: true,
			),
		});
	}),
	http.head("/api/feed", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return new HttpResponse(null, { status: 204 });
	}),
	http.post("/api/feed/reactions/refresh", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json().catch(() => null)) as {
			release_ids?: unknown;
		} | null;
		const releaseIds = Array.isArray(payload?.release_ids)
			? payload.release_ids.filter(
					(value): value is string =>
						typeof value === "string" && value.length > 0,
				)
			: [];
		const model = currentModel();
		const response: FeedReactionRefreshResponse = {
			items: releaseIds.map((releaseId) => ({
				release_id: releaseId,
				reactions: findReleaseReactions(model, releaseId),
			})),
		};
		return json(response);
	}),
	http.get("/api/dashboard/updates", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({
			[DEMO_UPDATE_TOKEN_FIELD]: DEMO_UPDATE_TOKEN_VALUE,
			generated_at: new Date().toISOString(),
			lists: {
				feed: {
					changed: false,
					new_count: 0,
					latest_keys: [],
				},
				briefs: {
					changed: false,
					new_count: 0,
					latest_keys: [],
				},
				notifications: {
					changed: false,
					new_count: 0,
					latest_keys: [],
				},
			},
		});
	}),
	http.get("/api/briefs", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(
			currentModel().briefs.map((brief) => ({
				...brief,
				content_markdown: undefined,
			})),
		);
	}),
	http.get("/api/briefs/:briefId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const brief = currentModel().briefs.find(
			(item) => item.id === params.briefId,
		);
		if (!brief) {
			return json(
				{ error: { code: "not_found", message: "Brief not found." } },
				{ status: 404 },
			);
		}
		return json(brief);
	}),
	http.get("/api/notifications", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().notifications);
	}),
	http.get("/api/repos/following", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().followingRepos);
	}),
	http.put("/api/repos/:owner/:repo/following", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const access = requireRuntimeAccess();
		const fullName = `${params.owner}/${params.repo}`;
		let nextItem: FollowingReposResponse["items"][number] | null = null;
		access.updateModel((model) => {
			const existing = model.followingRepos.items.find(
				(item) => item.full_name === fullName,
			);
			nextItem = existing
				? {
						...existing,
						is_following: true,
						follow_state_source: "manual_feed",
					}
				: {
						repo_id: 9999,
						full_name: fullName,
						owner_login: String(params.owner),
						name: String(params.repo),
						html_url: `https://github.com/${fullName}`,
						description: "Demo followed repository",
						is_private: false,
						first_source: "manual_feed",
						first_associated_at: new Date().toISOString(),
						last_seen_at: new Date().toISOString(),
						is_following: true,
						follow_state_source: "manual_feed",
						repo_visual: null,
						sources: {
							personal_owned: false,
							github_star: false,
							manual_feed: true,
						},
					};
			const nextItems = existing
				? model.followingRepos.items.map((item) =>
						item.full_name === fullName ? nextItem! : item,
					)
				: [nextItem!, ...model.followingRepos.items];
			return {
				...model,
				followingRepos: {
					...model.followingRepos,
					following_count: nextItems.filter((item) => item.is_following).length,
					items: nextItems,
				},
			};
		});
		access.recordMutation(
			"Follow repo",
			`${fullName} marked as followed in demo state.`,
		);
		return json(nextItem);
	}),
	http.delete(
		"/api/repos/:owner/:repo/following",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			const fullName = `${params.owner}/${params.repo}`;
			let nextItem: FollowingReposResponse["items"][number] | null = null;
			access.updateModel((model) => {
				const nextItems = model.followingRepos.items.map((item) => {
					if (item.full_name !== fullName) return item;
					nextItem = {
						...item,
						is_following: false,
						follow_state_source: "demo_removed",
					};
					return nextItem;
				});
				return {
					...model,
					followingRepos: {
						...model.followingRepos,
						following_count: nextItems.filter((item) => item.is_following)
							.length,
						items: nextItems,
					},
				};
			});
			access.recordMutation(
				"Unfollow repo",
				`${fullName} marked as unfollowed in demo state.`,
			);
			return json(nextItem);
		},
	),
	http.get("/api/repos/:owner/:repo/public-release", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().publicationStatus);
	}),
	http.post(
		"/api/repos/:owner/:repo/public-release",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			access.updateModel((model) => ({
				...model,
				publicationStatus: {
					...model.publicationStatus,
					publication_state: "private_owner_published",
					can_publish: false,
					can_unpublish: true,
					published_at: new Date().toISOString(),
				},
			}));
			access.recordMutation(
				"Publish public release page",
				`${params.owner}/${params.repo} is now published in demo memory only.`,
			);
			access.patchShareState({ publicationState: "published" });
			return json(currentModel().publicationStatus);
		},
	),
	http.delete(
		"/api/repos/:owner/:repo/public-release",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			access.updateModel((model) => ({
				...model,
				publicationStatus: {
					...model.publicationStatus,
					publication_state: "private_owner_unpublished",
					can_publish: true,
					can_unpublish: false,
					published_at: null,
				},
			}));
			access.recordMutation(
				"Unpublish public release page",
				`${params.owner}/${params.repo} is now unpublished in demo memory only.`,
			);
			access.patchShareState({ publicationState: "unpublished" });
			return json(currentModel().publicationStatus);
		},
	),
	http.post("/api/sync/all", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const taskId = "task-demo-sync-all";
		ensureTaskStream(taskId, buildSyncFrames(taskId));
		requireRuntimeAccess().recordMutation(
			"Run sync all",
			"Queued a simulated dashboard sync task with in-memory SSE events.",
		);
		return json(buildTaskAcceptedResponse(taskId, "sync.all"));
	}),
	http.post("/api/sync/notifications", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const taskId = "task-demo-sync-inbox";
		ensureTaskStream(taskId, [
			{ delayMs: 120, type: "task.running", data: { task_id: taskId } },
			{ delayMs: 660, type: "task.completed", data: { status: "succeeded" } },
		]);
		requireRuntimeAccess().recordMutation(
			"Sync inbox",
			"Queued a simulated inbox refresh task.",
		);
		return json(buildTaskAcceptedResponse(taskId, "sync.notifications"));
	}),
	http.get(
		"/api/public/repos/:owner/:repo/releases/content",
		async ({ request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const url = new URL(request.url);
			const releaseIds = new Set(
				(url.searchParams.get("release_ids") ?? "").split(","),
			);
			return json({
				items: currentModel()
					.publicReleaseList.items.filter((item) =>
						releaseIds.has(item.release_id),
					)
					.map((item) => ({
						release_id: item.release_id,
						translated: item.translated,
						smart: item.smart,
					})),
			});
		},
	),
	http.get("/api/public/repos/:owner/:repo/releases", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(buildDemoPublicReleaseList(request));
	}),
	http.get(
		"/api/public/repos/:owner/:repo/releases/tag/:tag",
		async ({ request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			return json(currentModel().publicReleaseDetail);
		},
	),
	http.get("/api/me/github-connections", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({ items: currentModel().githubConnections });
	}),
	http.delete(
		"/api/me/github-connections/:connectionId",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			access.updateModel((model) => ({
				...model,
				githubConnections: model.githubConnections.filter(
					(item) => item.id !== params.connectionId,
				),
			}));
			access.recordMutation(
				"Remove GitHub binding",
				`Deleted ${params.connectionId} from demo memory only.`,
			);
			return json({ items: currentModel().githubConnections });
		},
	),
	http.get("/api/me/linuxdo", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().linuxdo);
	}),
	http.delete("/api/me/linuxdo", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			linuxdo: { ...model.linuxdo, connection: null },
		}));
		access.recordMutation(
			"Unbind LinuxDO",
			"LinuxDO binding removed in demo memory.",
		);
		return json(currentModel().linuxdo);
	}),
	http.get("/api/me/passkeys", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({ items: currentModel().passkeys });
	}),
	http.post("/api/auth/passkeys/register/options", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({
			publicKey: {
				rp: { id: url.hostname, name: "OctoRill Demo" },
				user: {
					id: "ZGVtby1wYXNza2V5LXVzZXI",
					name: "octo-demo",
					displayName: "Octo Demo",
				},
				challenge: "ZGVtby1wYXNza2V5LWNoYWxsZW5nZQ",
				pubKeyCredParams: [{ alg: -7, type: "public-key" }],
				timeout: 60000,
			},
		});
	}),
	http.post("/api/auth/passkeys/authenticate/options", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const response: PasskeyRequestOptionsJSON = {
			publicKey: {
				challenge: "ZGVtby1wYXNza2V5LWF1dGg",
				rpId: url.hostname,
				allowCredentials: [
					{
						id: "ZGVtby1wYXNza2V5LWNyZWQ",
						type: "public-key",
						transports: ["internal"],
					},
				],
				userVerification: "preferred",
				hints: ["client-device"],
			},
		};
		return json(response);
	}),
	http.post("/api/auth/passkeys/authenticate/verify", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const response: PasskeyAuthenticateVerifyResponse = {
			status: "authenticated",
			next_path: buildDemoAuthRedirect("login"),
		};
		return json(response);
	}),
	http.post("/api/auth/passkeys/register/verify", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			passkeys: [
				{
					id: `passkey-${model.passkeys.length + 1}`,
					label: "Simulated Passkey",
					created_at: new Date().toISOString(),
					last_used_at: null,
				},
				...model.passkeys,
			],
		}));
		access.recordMutation(
			"Register passkey",
			"Added a simulated passkey to demo memory.",
		);
		const response: PasskeyRegisterVerifyResponse = {
			status: "registered",
			next_path: null,
		};
		return json(response);
	}),
	http.delete("/api/me/passkeys/:passkeyId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			passkeys: model.passkeys.filter((item) => item.id !== params.passkeyId),
		}));
		access.recordMutation(
			"Delete passkey",
			`${params.passkeyId} removed from demo memory.`,
		);
		return json({ items: currentModel().passkeys });
	}),
	http.get("/api/me/api-keys", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json({ items: currentModel().apiKeys });
	}),
	http.post("/api/me/api-keys", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json()) as { name?: string };
		const access = requireRuntimeAccess();
		let created: CreateApiKeyResponse | null = null;
		access.updateModel((model) => {
			const item = {
				id: `api-key-${model.apiKeys.length + 1}`,
				name: payload.name?.trim() || "Demo API Key",
				[DEMO_API_KEY_FIELD]: DEMO_CREATED_API_KEY_VALUE,
				masked_key: "orill_ak_demo...text",
				created_at: new Date().toISOString(),
				last_used_at: null,
			};
			created = { item, api_key: item.api_key };
			return {
				...model,
				apiKeys: [item, ...model.apiKeys],
			};
		});
		access.recordMutation(
			"Create API key",
			"Created a simulated API key in demo memory.",
		);
		return json(created);
	}),
	http.delete("/api/me/api-keys/:apiKeyId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			apiKeys: model.apiKeys.filter((item) => item.id !== params.apiKeyId),
		}));
		access.recordMutation(
			"Delete API key",
			`${params.apiKeyId} removed from demo memory.`,
		);
		return json({ items: currentModel().apiKeys });
	}),
	http.get("/api/me/profile", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().profile);
	}),
	http.patch("/api/me/profile", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json()) as Partial<{
			daily_brief_time_zone: string;
			include_own_releases: boolean;
		}>;
		const access = requireRuntimeAccess();
		access.updateModel((model) => {
			const nextProfile = {
				...model.profile,
				daily_brief_time_zone:
					payload.daily_brief_time_zone ?? model.profile.daily_brief_time_zone,
				include_own_releases:
					payload.include_own_releases ?? model.profile.include_own_releases,
			};
			return {
				...model,
				profile: nextProfile,
				me: model.me
					? {
							...model.me,
							dashboard: {
								...model.me.dashboard,
								include_own_releases: nextProfile.include_own_releases,
							},
						}
					: model.me,
				feed:
					payload.include_own_releases === undefined
						? model.feed
						: reconcileDemoFeedIncludeOwnReleases(
								model,
								nextProfile.include_own_releases,
							),
			};
		});
		access.recordMutation(
			"Save settings profile",
			"Saved my releases / brief profile fields in demo memory only.",
		);
		if (payload.include_own_releases !== undefined) {
			access.patchShareState({
				includeOwnReleases: payload.include_own_releases,
			});
		}
		return json(currentModel().profile);
	}),
	http.get("/api/reaction-token/status", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().reactionToken);
	}),
	http.post("/api/reaction-token/check", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json()) as { token?: string };
		const response: ReactionTokenCheckResponse = {
			state: payload.token?.trim() ? "valid" : "invalid",
			message: payload.token?.trim() ? "PAT 可用" : "PAT 不能为空",
			owner: currentModel().reactionToken.owner,
		};
		return json(response);
	}),
	http.put("/api/reaction-token", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json()) as { token?: string };
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			reactionToken: {
				...model.reactionToken,
				configured: true,
				// Never reflect user-entered secret prefixes back into the mock UI.
				masked_token: payload.token
					? "demo_pat_token_xxxxxxxx"
					: model.reactionToken.masked_token,
				check: {
					state: "valid",
					message: "PAT 可用",
					checked_at: new Date().toISOString(),
				},
			},
		}));
		access.recordMutation(
			"Save GitHub PAT",
			"Saved a simulated PAT in demo memory.",
		);
		return json(currentModel().reactionToken as ReactionTokenStatusResponse);
	}),
	http.get("/api/admin/users", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(filterUsers(currentModel().adminUsers, url.searchParams));
	}),
	http.patch("/api/admin/users/:userId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload = (await request.json()) as Partial<{
			is_admin: boolean;
			is_disabled: boolean;
		}>;
		const access = requireRuntimeAccess();
		let updated: AdminUserItem | null = null;
		access.updateModel((model) => ({
			...model,
			adminUsers: model.adminUsers.map((user) => {
				if (user.id !== params.userId) return user;
				updated = {
					...user,
					is_admin: payload.is_admin ?? user.is_admin,
					is_disabled: payload.is_disabled ?? user.is_disabled,
				};
				return updated;
			}),
		}));
		access.recordMutation(
			"Patch admin user",
			`Updated role/disabled flags for ${params.userId} in demo memory.`,
		);
		return json(updated);
	}),
	http.get("/api/admin/users/:userId/profile", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const profile = currentModel().adminUserProfiles[String(params.userId)];
		if (!profile) {
			return json(
				{ error: { code: "not_found", message: "Profile not found." } },
				{ status: 404 },
			);
		}
		return json(profile);
	}),
	http.patch(
		"/api/admin/users/:userId/profile",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const payload =
				(await request.json()) as Partial<AdminUserProfileResponse>;
			const access = requireRuntimeAccess();
			let nextProfile: AdminUserProfileResponse | null = null;
			access.updateModel((model) => {
				const current = model.adminUserProfiles[String(params.userId)];
				if (!current) return model;
				nextProfile = {
					...current,
					daily_brief_time_zone:
						payload.daily_brief_time_zone ?? current.daily_brief_time_zone,
					include_own_releases:
						payload.include_own_releases ?? current.include_own_releases,
				};
				return updateProfile(model, nextProfile);
			});
			access.recordMutation(
				"Save admin user profile",
				`Saved profile overrides for ${params.userId} in demo memory.`,
			);
			return json(nextProfile);
		},
	),
	http.get("/api/admin/dashboard", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const window = url.searchParams.get("window") === "30d" ? "30d" : "7d";
		return json(buildAdminDashboardResponse(currentModel(), window));
	}),
	http.get("/api/admin/jobs/overview", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().adminJobs.overview);
	}),
	http.get("/api/admin/jobs/realtime", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(filterTasks(allAdminTasks(currentModel()), url.searchParams));
	}),
	http.get("/api/admin/jobs/realtime/:taskId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const detail = currentModel().adminJobs.taskDetails[String(params.taskId)];
		if (!detail) {
			return json(
				{ error: { code: "not_found", message: "Task detail not found." } },
				{ status: 404 },
			);
		}
		return json(detail as AdminRealtimeTaskDetailResponse);
	}),
	http.post(
		"/api/admin/jobs/realtime/:taskId/retry",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			access.updateModel((model) =>
				updateAdminTask(model, String(params.taskId), {
					status: "running",
					error_message: null,
					cancel_requested: false,
					updated_at: new Date().toISOString(),
				}),
			);
			access.recordMutation(
				"Retry admin task",
				`Task ${params.taskId} moved back to running in demo memory.`,
			);
			return json({ task_id: params.taskId, status: "running" });
		},
	),
	http.post(
		"/api/admin/jobs/realtime/:taskId/cancel",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const access = requireRuntimeAccess();
			access.updateModel((model) =>
				updateAdminTask(model, String(params.taskId), {
					cancel_requested: true,
					status: "canceled",
					finished_at: new Date().toISOString(),
					updated_at: new Date().toISOString(),
				}),
			);
			access.recordMutation(
				"Cancel admin task",
				`Task ${params.taskId} canceled in demo memory.`,
			);
			return json({ task_id: params.taskId, status: "canceled" });
		},
	),
	http.get("/api/admin/jobs/translations/status", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().adminJobs.translationStatus);
	}),
	http.patch(
		"/api/admin/jobs/translations/runtime-config",
		async ({ request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const payload =
				(await request.json()) as Partial<AdminTranslationRuntimeConfigUpdateRequest>;
			const access = requireRuntimeAccess();
			access.updateModel((model) => {
				const current = model.adminJobs.translationStatus;
				const general = Math.max(
					1,
					Math.floor(
						payload.general_worker_concurrency ??
							current.target_general_worker_concurrency,
					),
				);
				const dedicated = Math.max(
					0,
					Math.floor(
						payload.dedicated_worker_concurrency ??
							current.target_dedicated_worker_concurrency,
					),
				);
				return {
					...model,
					adminJobs: {
						...model.adminJobs,
						translationStatus: {
							...current,
							target_general_worker_concurrency: general,
							target_dedicated_worker_concurrency: dedicated,
							target_worker_concurrency: general + dedicated,
						},
					},
				};
			});
			access.recordMutation(
				"Save translation worker settings",
				"Updated translation worker targets in demo memory only.",
			);
			return json(currentModel().adminJobs.translationStatus);
		},
	),
	http.get("/api/admin/jobs/ai-records/:kind", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const kind = String(params.kind);
		if (kind !== "release" && kind !== "announcement" && kind !== "brief") {
			return badRequest("invalid collection record kind");
		}
		const url = new URL(request.url);
		const from = url.searchParams.get("from");
		const before = url.searchParams.get("before");
		const parseAttemptBound = (name: string, fallback: number | null) => {
			const raw = url.searchParams.get(name);
			if (raw === null || raw === "") return fallback;
			const parsed = Number(raw);
			return Number.isInteger(parsed) ? parsed : Number.NaN;
		};
		const attemptMin = parseAttemptBound("attempt_min", 0) ?? 0;
		const attemptMax = parseAttemptBound("attempt_max", null);
		const parseStatuses = (name: string) =>
			(url.searchParams.get(name) ?? "")
				.split(",")
				.map((value) => value.trim())
				.filter(Boolean);
		const translationStatuses = parseStatuses("translation_status");
		const polishStatuses = parseStatuses("polish_status");
		if (!Number.isInteger(attemptMin) || attemptMin < 0 || attemptMin > 10) {
			return badRequest("attempt_min must be between 0 and 10");
		}
		if (
			attemptMax !== null &&
			(!Number.isInteger(attemptMax) || attemptMax < 0 || attemptMax > 10)
		) {
			return badRequest("attempt_max must be between 0 and 10");
		}
		if (attemptMax !== null && attemptMax < attemptMin) {
			return badRequest(
				"attempt_max must be greater than or equal to attempt_min",
			);
		}
		const items = demoCollectionRecords()[kind].filter((item) => {
			const timestamp =
				item.kind === "brief" ? item.generated_at : item.occurred_at;
			const value = timestamp ? new Date(timestamp).getTime() : Number.NaN;
			const attemptCount = demoRecordAttemptCount(item);
			const translationStatus = item.translation?.display_status;
			const polishStatus = item.polish.display_status;
			return (
				(!from ||
					(Number.isFinite(value) && value >= new Date(from).getTime())) &&
				(!before ||
					(Number.isFinite(value) && value < new Date(before).getTime())) &&
				attemptCount >= attemptMin &&
				(attemptMax === null || attemptCount <= attemptMax) &&
				(kind === "brief" ||
					translationStatuses.length === 0 ||
					(translationStatus !== undefined &&
						translationStatuses.includes(translationStatus))) &&
				(polishStatuses.length === 0 || polishStatuses.includes(polishStatus))
			);
		});
		return json(paginateItems(items, url.searchParams));
	}),
	http.get(
		"/api/admin/jobs/ai-records/:kind/:recordId",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const kind = String(params.kind);
			if (kind !== "release" && kind !== "announcement" && kind !== "brief") {
				return badRequest("invalid collection record kind");
			}
			const detail = demoCollectionDetail(kind, String(params.recordId));
			return detail
				? json(detail)
				: json(
						{
							error: {
								code: "not_found",
								message: "Collection record not found.",
							},
						},
						{ status: 404 },
					);
		},
	),
	http.get("/api/admin/jobs/translations/requests", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const status = url.searchParams.get("status") ?? "all";
		const taskGroup = url.searchParams.get("task_group") ?? "all";
		const discoveredFrom = url.searchParams.get("discovered_from");
		const discoveredBefore = url.searchParams.get("discovered_before");
		const lowerBound = discoveredFrom
			? new Date(discoveredFrom).getTime()
			: Number.NaN;
		const upperBound = discoveredBefore
			? new Date(discoveredBefore).getTime()
			: Number.NaN;
		const items = currentModel().adminJobs.translationRequests.filter(
			(item) => {
				if (status !== "all" && item.status !== status) return false;
				if (taskGroup === "translation" && item.kind === "release_smart")
					return false;
				if (taskGroup === "polish" && item.kind !== "release_smart")
					return false;
				const discoveredAt = new Date(item.created_at).getTime();
				return (
					(Number.isNaN(lowerBound) || discoveredAt >= lowerBound) &&
					(Number.isNaN(upperBound) || discoveredAt < upperBound)
				);
			},
		);
		return json(paginateItems(items, url.searchParams));
	}),
	http.get(
		"/api/admin/jobs/translations/requests/:requestId",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const detail =
				currentModel().adminJobs.translationRequestDetails[
					String(params.requestId)
				];
			if (!detail) {
				return json(
					{
						error: {
							code: "not_found",
							message: "Translation request not found.",
						},
					},
					{ status: 404 },
				);
			}
			return json(detail);
		},
	),
	http.get("/api/admin/jobs/translations/batches", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const status = url.searchParams.get("status") ?? "all";
		const items = currentModel().adminJobs.translationBatches.filter(
			(item) => status === "all" || item.status === status,
		);
		return json(paginateItems(items, url.searchParams));
	}),
	http.get(
		"/api/admin/jobs/translations/batches/:batchId",
		async ({ params, request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const detail =
				currentModel().adminJobs.translationBatchDetails[
					String(params.batchId)
				];
			if (!detail) {
				return json(
					{
						error: {
							code: "not_found",
							message: "Translation batch not found.",
						},
					},
					{ status: 404 },
				);
			}
			return json(detail);
		},
	),
	http.get(
		"/api/admin/jobs/translations/attempt-events",
		async ({ request }) => {
			const url = new URL(request.url);
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const entityId = url.searchParams.get("entity_id");
			const requestId = url.searchParams.get("request_id");
			const workItemId = url.searchParams.get("work_item_id");
			const kind = url.searchParams.get("kind");
			const variant = url.searchParams.get("variant");
			const items = currentModel().adminJobs.translationAttemptEvents.filter(
				(item) =>
					(!entityId || item.entity_id === entityId) &&
					(!requestId || item.request_id === requestId) &&
					(!workItemId || item.work_item_id === workItemId) &&
					(!kind || item.kind === kind) &&
					(!variant || item.variant === variant),
			);
			return json(paginateItems(items, url.searchParams));
		},
	),
	http.get(
		"/api/admin/jobs/webhook-push/runtime-config",
		async ({ request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			return json(currentModel().adminJobs.webhookPushRuntimeConfig);
		},
	),
	http.patch(
		"/api/admin/jobs/webhook-push/runtime-config",
		async ({ request }) => {
			const network = await applyNetworkProfile(request);
			if (network) return network;
			const rawPayload = await request.json();
			if (
				!rawPayload ||
				typeof rawPayload !== "object" ||
				Array.isArray(rawPayload)
			) {
				return badRequest("request body must be an object");
			}
			const auditIntervalDays = (rawPayload as Record<string, unknown>)
				.audit_interval_days;
			if (
				typeof auditIntervalDays !== "number" ||
				!Number.isInteger(auditIntervalDays) ||
				auditIntervalDays < 1 ||
				auditIntervalDays > 30
			) {
				return badRequest(
					"audit_interval_days must be an integer from 1 to 30",
				);
			}
			const access = requireRuntimeAccess();
			access.updateModel((model) => ({
				...model,
				adminJobs: {
					...model.adminJobs,
					webhookPushRuntimeConfig: {
						...model.adminJobs.webhookPushRuntimeConfig,
						audit_interval_days: auditIntervalDays,
					},
				},
			}));
			access.recordMutation(
				"Save webhook audit interval",
				"Updated webhook push audit interval in demo memory only.",
			);
			return json(currentModel().adminJobs.webhookPushRuntimeConfig);
		},
	),
	http.get("/api/admin/jobs/sync/runtime-config", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().adminJobs.syncRuntimeConfig);
	}),
	http.patch("/api/admin/jobs/sync/runtime-config", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const payload =
			(await request.json()) as Partial<AdminSyncRuntimeConfigResponse>;
		const access = requireRuntimeAccess();
		access.updateModel((model) => ({
			...model,
			adminJobs: {
				...model.adminJobs,
				syncRuntimeConfig: {
					...model.adminJobs.syncRuntimeConfig,
					sync_auto_fetch_interval_minutes:
						payload.sync_auto_fetch_interval_minutes ??
						model.adminJobs.syncRuntimeConfig.sync_auto_fetch_interval_minutes,
					retry_recent_failures_interval_minutes:
						payload.retry_recent_failures_interval_minutes ??
						model.adminJobs.syncRuntimeConfig
							.retry_recent_failures_interval_minutes,
					repo_release_worker_concurrency:
						payload.repo_release_worker_concurrency ??
						model.adminJobs.syncRuntimeConfig.repo_release_worker_concurrency,
					repo_refresh_system_budget_per_window:
						payload.repo_refresh_system_budget_per_window ??
						model.adminJobs.syncRuntimeConfig
							.repo_refresh_system_budget_per_window,
					daily_brief_schedule_local_time:
						payload.daily_brief_schedule_local_time ??
						model.adminJobs.syncRuntimeConfig.daily_brief_schedule_local_time,
				},
			},
		}));
		access.recordMutation(
			"Save job runtime settings",
			"Updated admin sync runtime settings in demo memory only.",
		);
		return json(currentModel().adminJobs.syncRuntimeConfig);
	}),
	http.get("/api/admin/jobs/llm/status", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().adminJobs.llmStatus);
	}),
	http.get("/api/admin/jobs/llm/activity", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		return json(currentModel().adminJobs.llmActivity);
	}),
	http.patch("/api/admin/jobs/llm/runtime-config", async ({ request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const rawPayload = await request.json();
		if (
			!rawPayload ||
			typeof rawPayload !== "object" ||
			Array.isArray(rawPayload)
		) {
			return badRequest("request body must be an object");
		}
		const payload = rawPayload as Record<string, unknown>;
		const rawModels = payload.llm_models;
		if (
			rawModels !== undefined &&
			(!Array.isArray(rawModels) ||
				rawModels.some((model) => typeof model !== "string"))
		) {
			return badRequest("llm_models must be an array of model names");
		}
		const normalizedModels = (rawModels as string[] | undefined)?.map((model) =>
			model.trim(),
		);
		if (
			normalizedModels &&
			(normalizedModels.length === 0 ||
				normalizedModels.some((model) => model.length === 0) ||
				new Set(normalizedModels).size !== normalizedModels.length)
		) {
			return badRequest(
				"llm_models must contain non-empty, unique model names",
			);
		}
		if (!isPositiveInteger(payload.max_concurrency)) {
			return badRequest("max_concurrency must be a positive integer");
		}
		if (
			Object.hasOwn(payload, "ai_model_context_limit") &&
			payload.ai_model_context_limit !== null &&
			!isPositiveInteger(payload.ai_model_context_limit, 0xffff_ffff)
		) {
			return badRequest("ai_model_context_limit must be a positive integer");
		}
		if (
			Object.hasOwn(payload, "llm_recovery_enabled") &&
			typeof payload.llm_recovery_enabled !== "boolean"
		) {
			return badRequest("llm_recovery_enabled must be a boolean");
		}
		if (
			Object.hasOwn(payload, "llm_recovery_rollout_percent") &&
			(!Number.isInteger(payload.llm_recovery_rollout_percent) ||
				(payload.llm_recovery_rollout_percent as number) < 0 ||
				(payload.llm_recovery_rollout_percent as number) > 100)
		) {
			return badRequest(
				"llm_recovery_rollout_percent must be between 0 and 100",
			);
		}
		const requestedRecoveryEnabled =
			typeof payload.llm_recovery_enabled === "boolean"
				? payload.llm_recovery_enabled
				: undefined;
		const requestedRecoveryRollout =
			typeof payload.llm_recovery_rollout_percent === "number"
				? payload.llm_recovery_rollout_percent
				: undefined;
		const access = requireRuntimeAccess();
		access.updateModel((model) => {
			const currentStatus = model.adminJobs.llmStatus;
			const hasContextLimit = Object.hasOwn(payload, "ai_model_context_limit");
			const contextLimit = hasContextLimit
				? typeof payload.ai_model_context_limit === "number"
					? payload.ai_model_context_limit
					: null
				: currentStatus.ai_model_context_limit;
			const nextModels = normalizedModels ?? currentStatus.llm_models;
			const nextMaxConcurrency = payload.max_concurrency as number;
			const previousStatuses = new Map(
				currentStatus.model_statuses.map((status) => [status.model, status]),
			);
			const nextModelStatuses = nextModels.map((modelName, index) => {
				const previous = previousStatuses.get(modelName);
				const builtinLimit = modelName === "gpt-4.1-mini" ? 1047576 : 128000;
				const cooldownUntil = previous?.cooldown_until ?? null;
				const cooldownUntilAt = cooldownUntil
					? Date.parse(cooldownUntil)
					: Number.NaN;
				const isCoolingDown =
					previous?.status === "cooldown" &&
					Number.isFinite(cooldownUntilAt) &&
					cooldownUntilAt > Date.now();
				return {
					model: modelName,
					priority: index + 1,
					status: isCoolingDown ? "cooldown" : "ready",
					consecutive_final_failures: previous?.consecutive_final_failures ?? 0,
					relevant_failure_count: previous?.relevant_failure_count ?? 0,
					cooldown_until: isCoolingDown ? cooldownUntil : null,
					effective_input_limit: contextLimit ?? builtinLimit,
					effective_input_limit_source:
						contextLimit === null ? "builtin_catalog" : "admin_override",
				};
			});
			const selectedStatus =
				nextModelStatuses.find((status) => status.status !== "cooldown") ??
				[...nextModelStatuses].sort((left, right) => {
					const leftUntil = left.cooldown_until
						? new Date(left.cooldown_until).getTime()
						: Number.POSITIVE_INFINITY;
					const rightUntil = right.cooldown_until
						? new Date(right.cooldown_until).getTime()
						: Number.POSITIVE_INFINITY;
					return leftUntil - rightUntil || left.priority - right.priority;
				})[0];
			const selectedModel = selectedStatus?.model ?? null;
			const configuredModelNames = new Set(nextModels);
			const modelsWithWindowActivity = new Set(
				model.adminJobs.llmActivity.buckets.flatMap((bucket) =>
					bucket.counts
						.filter((count) => count.succeeded + count.failed > 0)
						.map((count) => count.model),
				),
			);
			const retiredActivityModels = model.adminJobs.llmActivity.models
				.filter(
					(item) =>
						!configuredModelNames.has(item.model) &&
						modelsWithWindowActivity.has(item.model),
				)
				.map((item) => ({ ...item, priority: null, configured: false }));
			const nextActivityModels = [
				...nextModels.map((modelName, index) => ({
					model: modelName,
					priority: index + 1,
					configured: true,
				})),
				...retiredActivityModels,
			];
			const nextActivity = buildLlmActivityFromCalls(
				model.adminJobs.llmCalls,
				new Date(model.adminJobs.llmActivity.window_started_at),
				new Date(model.adminJobs.llmActivity.window_ended_at),
				nextActivityModels,
			);
			return {
				...model,
				adminJobs: {
					...model.adminJobs,
					llmStatus: {
						...currentStatus,
						max_concurrency: nextMaxConcurrency,
						available_slots: Math.max(
							0,
							nextMaxConcurrency - currentStatus.in_flight_calls,
						),
						ai_model_context_limit: contextLimit,
						llm_models: nextModels,
						selected_model_for_new_calls: selectedModel,
						effective_model_input_limit:
							selectedStatus?.effective_input_limit ?? contextLimit ?? 128000,
						effective_model_input_limit_source:
							selectedStatus?.effective_input_limit_source ??
							(contextLimit === null ? "builtin_catalog" : "admin_override"),
						model_statuses: nextModelStatuses,
						llm_recovery_enabled:
							requestedRecoveryEnabled ?? currentStatus.llm_recovery_enabled,
						llm_recovery_rollout_percent:
							requestedRecoveryRollout ??
							currentStatus.llm_recovery_rollout_percent,
					},
					llmActivity: {
						...nextActivity,
					},
				},
			};
		});
		access.recordMutation(
			"Save LLM settings",
			"Updated admin LLM scheduler settings in demo memory only.",
		);
		return json(currentModel().adminJobs.llmStatus);
	}),
	http.get("/api/admin/jobs/llm/calls", async ({ request }) => {
		const url = new URL(request.url);
		const network = await applyNetworkProfile(request);
		if (network) return network;
		let items = currentModel().adminJobs.llmCalls;
		const status = url.searchParams.get("status") ?? "all";
		if (!["all", "queued", "running", "succeeded", "failed"].includes(status)) {
			return badRequest("invalid status filter");
		}
		const model = url.searchParams.get("model")?.trim() ?? "";
		const source = url.searchParams.get("source")?.trim() ?? "";
		const requestedBy = url.searchParams.get("requested_by")?.trim() ?? "";
		const parentTaskId = url.searchParams.get("parent_task_id");
		const startedFrom = url.searchParams.get("started_from");
		const startedTo = url.searchParams.get("started_to");
		const finishedFrom = url.searchParams.get("finished_from");
		const finishedBefore = url.searchParams.get("finished_before");
		for (const [field, value] of [
			["started_from", startedFrom],
			["started_to", startedTo],
			["finished_from", finishedFrom],
			["finished_before", finishedBefore],
		] as const) {
			if (value?.trim() && !isRfc3339Timestamp(value.trim())) {
				return badRequest(`${field} must be RFC3339`);
			}
		}
		const timestamp = (value: string | null | undefined) =>
			value ? new Date(value).getTime() : Number.NaN;
		if (parentTaskId) {
			items = items.filter((item) => item.parent_task_id === parentTaskId);
		}
		items = items.filter((item) => {
			if (status !== "all" && item.status !== status) return false;
			if (model && item.model !== model) return false;
			if (source && item.source !== source) return false;
			if (requestedBy && item.requested_by !== requestedBy) return false;
			const startedAt = timestamp(item.started_at ?? item.created_at);
			const finishedAt = timestamp(
				item.finished_at ?? item.updated_at ?? item.created_at,
			);
			const startedFromAt = timestamp(startedFrom);
			const startedToAt = timestamp(startedTo);
			const finishedFromAt = timestamp(finishedFrom);
			const finishedBeforeAt = timestamp(finishedBefore);
			return (
				(Number.isNaN(startedFromAt) || startedAt >= startedFromAt) &&
				(Number.isNaN(startedToAt) || startedAt <= startedToAt) &&
				(Number.isNaN(finishedFromAt) || finishedAt >= finishedFromAt) &&
				(Number.isNaN(finishedBeforeAt) || finishedAt < finishedBeforeAt)
			);
		});
		const sort = url.searchParams.get("sort") ?? "created_desc";
		if (!["created_desc", "status_grouped"].includes(sort)) {
			return badRequest("invalid sort filter");
		}
		const statusRank = (value: string) =>
			value === "running" ? 0 : value === "queued" ? 1 : 2;
		items = [...items].sort((left, right) => {
			if (sort === "status_grouped" && status === "all") {
				const rankDifference =
					statusRank(left.status) - statusRank(right.status);
				if (rankDifference !== 0) return rankDifference;
			}
			const timeDifference =
				timestamp(right.created_at) - timestamp(left.created_at);
			if (timeDifference !== 0) return timeDifference;
			const createdDifference = right.created_at.localeCompare(left.created_at);
			return createdDifference !== 0
				? createdDifference
				: right.id.localeCompare(left.id);
		});
		const positiveInteger = (value: string | null, fallback: number) => {
			const parsed = Number(value);
			return Number.isFinite(parsed) && parsed >= 1
				? Math.floor(parsed)
				: fallback;
		};
		const page = positiveInteger(url.searchParams.get("page"), 1);
		const pageSize = Math.min(
			100,
			positiveInteger(url.searchParams.get("page_size"), 20),
		);
		const total = items.length;
		items = items.slice((page - 1) * pageSize, page * pageSize);
		return json({
			items,
			total,
			page,
			page_size: pageSize,
		});
	}),
	http.get("/api/admin/jobs/llm/calls/:callId", async ({ params, request }) => {
		const network = await applyNetworkProfile(request);
		if (network) return network;
		const detail =
			currentModel().adminJobs.llmCallDetails[String(params.callId)];
		if (!detail) {
			return json(
				{ error: { code: "not_found", message: "LLM call not found." } },
				{ status: 404 },
			);
		}
		return json(detail as AdminLlmCallDetailResponse);
	}),
];

class DemoEventSource extends EventTarget {
	public static readonly CONNECTING = 0;
	public static readonly OPEN = 1;
	public static readonly CLOSED = 2;

	public readonly CONNECTING = DemoEventSource.CONNECTING;
	public readonly OPEN = DemoEventSource.OPEN;
	public readonly CLOSED = DemoEventSource.CLOSED;

	public readyState = DemoEventSource.CONNECTING;
	public onopen: ((this: EventSource, ev: Event) => unknown) | null = null;
	public onerror: ((this: EventSource, ev: Event) => unknown) | null = null;
	public onmessage:
		| ((this: EventSource, ev: MessageEvent<string>) => unknown)
		| null = null;
	public readonly url: string;
	public readonly withCredentials: boolean;
	private timers: number[] = [];

	public constructor(url: string, withCredentials = false) {
		super();
		this.url = url;
		this.withCredentials = withCredentials;
		this.open();
	}

	private dispatch(type: string, data?: unknown) {
		if (this.readyState === DemoEventSource.CLOSED) return;
		const message = new MessageEvent<string>(type, {
			data: data === undefined ? "" : JSON.stringify(data),
		});
		this.dispatchEvent(message);
		if (type === "message") {
			this.onmessage?.call(this as unknown as EventSource, message);
		}
	}

	private open() {
		const frames = resolveFrames(this.url);
		const openTimer = window.setTimeout(() => {
			if (this.readyState === DemoEventSource.CLOSED) return;
			this.readyState = DemoEventSource.OPEN;
			const event = new Event("open");
			this.onopen?.call(this as unknown as EventSource, event);
		}, 0);
		this.timers.push(openTimer);

		for (const frame of frames) {
			const timer = window.setTimeout(() => {
				this.dispatch(frame.type, frame.data);
			}, frame.delayMs);
			this.timers.push(timer);
		}
	}

	public close() {
		this.readyState = DemoEventSource.CLOSED;
		for (const timer of this.timers) {
			window.clearTimeout(timer);
		}
		this.timers = [];
	}
}

function resolveFrames(url: string): DemoEventFrame[] {
	const pathname = new URL(url, window.location.origin).pathname;
	const model = currentModel();

	if (pathname.startsWith("/api/tasks/")) {
		const taskId = pathname.split("/")[3] ?? "";
		return model.taskStreams[taskId] ?? [];
	}

	if (pathname === "/api/admin/jobs/events") {
		return model.adminJobsStream.map((frame) => ({
			...frame,
			type:
				frame.type === "job"
					? "job.event"
					: frame.type === "llm"
						? "llm.call"
						: frame.type === "scheduler"
							? "llm.scheduler"
							: "translation.event",
		}));
	}

	return [];
}

export function openDemoEventSource(
	url: string,
	options?: {
		withCredentials?: boolean;
	},
): EventSource {
	return new DemoEventSource(
		url,
		options?.withCredentials,
	) as unknown as EventSource;
}

export function handleDemoUnhandledRequest(
	request: Request,
	print: { warning: () => void },
) {
	const snapshot = currentSnapshot();
	const url = new URL(request.url);
	if (isCommonAssetRequest(request)) {
		return;
	}
	if (!snapshot.demoBuild && !hasDemoRuntimeRequestMarker(request)) {
		return;
	}
	if (url.pathname.startsWith("/api/") || url.pathname.startsWith("/auth/")) {
		print.warning();
	}
}
