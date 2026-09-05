import { type Page, type Route, expect, test } from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

type MockRequestRule = {
	pathname: string;
	search?: string;
	afterCount?: number;
	times?: number;
};

type MockDelayRule = MockRequestRule & {
	delayMs: number;
};

type MockFailureRule = MockRequestRule & {
	status?: number;
	message?: string;
};

const CURRENT_USER_ID = "2f4k7m9p3x6c8v2a";
const RECENT_EVENT_USER_ID = "4h6p9s3t5z8e2x4c";
const LONG_ADMIN_LOGIN = "storybook-admin-with-a-very-long-login-name";

type AdminJobsMockOptions = {
	responseDelayMs?: number;
	delayedPaths?: string[];
	delayRules?: MockDelayRule[];
	failureRules?: MockFailureRule[];
	emitStreamEvents?: boolean;
	emitLlmSchedulerEvents?: boolean;
	emitTranslationEvents?: boolean;
	emitSubscriptionProgressEvents?: boolean;
	currentUserLogin?: string;
	activityRefreshOnSecondRequest?: boolean;
};

function sleep(ms: number) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function installAdminJobsMocks(
	page: Page,
	options: AdminJobsMockOptions = {},
) {
	const currentUserLogin = options.currentUserLogin ?? "octo-admin";
	const tasks = [
		{
			id: "task-failed-1",
			task_type: "brief.daily_slot",
			status: "failed",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: "mock failed",
			created_at: "2026-02-26T00:00:00Z",
			started_at: "2026-02-26T00:00:10Z",
			finished_at: "2026-02-26T00:01:00Z",
			updated_at: "2026-02-26T00:01:00Z",
		},
		{
			id: "task-running-1",
			task_type: "sync.releases",
			status: "running",
			source: "api.sync_releases",
			requested_by: CURRENT_USER_ID,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T01:00:00Z",
			started_at: "2026-02-26T01:00:05Z",
			finished_at: null,
			updated_at: "2026-02-26T01:00:20Z",
		},
		{
			id: "task-translate-batch-1",
			task_type: "translate.release.batch",
			status: "succeeded",
			source: "api.translate_releases_batch_stream",
			requested_by: CURRENT_USER_ID,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T01:10:00Z",
			started_at: "2026-02-26T01:10:02Z",
			finished_at: "2026-02-26T01:10:40Z",
			updated_at: "2026-02-26T01:10:40Z",
		},
		{
			id: "task-subscriptions-1",
			task_type: "sync.subscriptions",
			status: "succeeded",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T14:30:00Z",
			started_at: "2026-02-26T14:30:04Z",
			finished_at: "2026-02-26T14:38:10Z",
			updated_at: "2026-02-26T14:38:10Z",
		},
		{
			id: "task-subscriptions-skipped",
			task_type: "sync.subscriptions",
			status: "succeeded",
			source: "scheduler",
			requested_by: null,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T14:40:00Z",
			started_at: "2026-02-26T14:40:01Z",
			finished_at: "2026-02-26T14:40:01Z",
			updated_at: "2026-02-26T14:40:01Z",
		},
	];

	const llmCalls = [
		{
			id: "llm-call-1",
			status: "failed",
			source: "job.api.translate_release",
			model: "gpt-4o-mini",
			requested_by: CURRENT_USER_ID,
			parent_task_id: "task-running-1",
			parent_task_type: "sync.releases",
			max_tokens: 900,
			attempt_count: 3,
			scheduler_wait_ms: 1200,
			first_token_wait_ms: 860,
			duration_ms: 2200,
			input_tokens: 1230,
			output_tokens: 0,
			cached_input_tokens: 640,
			total_tokens: 1230,
			input_messages_json: JSON.stringify([
				{ role: "system", content: "You are a release translator." },
				{ role: "user", content: "translate notes to Chinese" },
				{ role: "assistant", content: "收到，我将输出三条重点。" },
				{ role: "user", content: "请强调排障价值" },
			]),
			output_messages_json: null,
			prompt_text: "prompt 1",
			response_text: null,
			error_text: "mock llm failed",
			failure_class: "transient",
			final_model: "gpt-4o-mini",
			fallback_count: 0,
			retry_scheduled_at: "2026-02-26T02:05:00Z",
			recovery_attempt_count: 0,
			attempt_history: [
				{
					event_type: "attempt.failed",
					status: "failed",
					model: "gpt-4o-mini",
					attempt: 1,
					failure_class: "transient",
					retry_after_ms: 300000,
					from_model: "gpt-4o-mini",
					to_model: null,
					fallback_count: 0,
					created_at: "2026-02-26T02:00:03Z",
				},
			],
			created_at: "2026-02-26T02:00:00Z",
			started_at: "2026-02-26T02:00:01Z",
			finished_at: "2026-02-26T02:00:03Z",
			updated_at: "2026-02-26T02:00:03Z",
		},
		{
			id: "llm-call-2",
			status: "running",
			source: "api.translate_releases_batch",
			model: "gpt-4o-mini",
			requested_by: CURRENT_USER_ID,
			parent_task_id: "task-translate-batch-1",
			parent_task_type: "translate.release.batch",
			max_tokens: 900,
			attempt_count: 1,
			scheduler_wait_ms: 80,
			first_token_wait_ms: null,
			duration_ms: null,
			input_tokens: 780,
			output_tokens: null,
			cached_input_tokens: 320,
			total_tokens: null,
			input_messages_json: JSON.stringify([
				{ role: "system", content: "You are a summary assistant." },
				{ role: "user", content: "summarize changes" },
				{ role: "assistant", content: "收到，我将输出三条重点。" },
				{ role: "user", content: "请强调排障价值" },
			]),
			output_messages_json: null,
			prompt_text: "prompt 2",
			response_text: null,
			error_text: null,
			failure_class: null,
			final_model: null,
			fallback_count: 0,
			retry_scheduled_at: null,
			recovery_attempt_count: 0,
			attempt_history: [],
			created_at: "2026-02-26T03:00:00Z",
			started_at: "2026-02-26T03:00:00Z",
			finished_at: null,
			updated_at: "2026-02-26T03:00:00Z",
		},
		{
			id: "llm-call-3",
			status: "queued",
			source: "translation.scheduler.deadline",
			model: "gpt-4o-mini",
			requested_by: null,
			parent_task_id: null,
			parent_task_type: null,
			max_tokens: 900,
			attempt_count: 0,
			scheduler_wait_ms: 0,
			first_token_wait_ms: null,
			duration_ms: null,
			input_tokens: null,
			output_tokens: null,
			cached_input_tokens: null,
			total_tokens: null,
			input_messages_json: JSON.stringify([
				{ role: "system", content: "You are a queue placeholder." },
				{ role: "user", content: "queued request" },
			]),
			output_messages_json: null,
			prompt_text: "prompt 3",
			response_text: null,
			error_text: null,
			failure_class: null,
			final_model: null,
			fallback_count: 0,
			retry_scheduled_at: null,
			recovery_attempt_count: 0,
			attempt_history: [],
			created_at: "2026-02-26T04:00:00Z",
			started_at: null,
			finished_at: null,
			updated_at: "2026-02-26T04:00:00Z",
		},
	];
	let llmSchedulerStatus = {
		scheduler_enabled: true,
		llm_models: ["gpt-4o-mini", "gpt-4.1-mini"],
		selected_model_for_new_calls: "gpt-4.1-mini",
		max_concurrency: 2,
		ai_model_context_limit: null as number | null,
		effective_model_input_limit: 1047576,
		effective_model_input_limit_source: "builtin_catalog",
		model_statuses: [
			{
				model: "gpt-4o-mini",
				priority: 1,
				status: "cooldown",
				consecutive_final_failures: 3,
				cooldown_until: "2026-02-26T04:10:00Z",
				effective_input_limit: 128000,
				effective_input_limit_source: "builtin_catalog",
			},
			{
				model: "gpt-4.1-mini",
				priority: 2,
				status: "ready",
				consecutive_final_failures: 0,
				cooldown_until: null,
				effective_input_limit: 1047576,
				effective_input_limit_source: "builtin_catalog",
			},
		],
		available_slots: 1,
		waiting_calls: 1,
		in_flight_calls: 1,
		calls_24h: llmCalls.length,
		failed_24h: llmCalls.filter((item) => item.status === "failed").length,
		avg_wait_ms_24h: 640,
		avg_duration_ms_24h: 1300,
		last_success_at: "2026-02-26T03:00:01Z",
		last_failure_at: "2026-02-26T02:00:03Z",
	};
	const llmActivityStart = new Date("2026-02-24T01:00:00Z");
	const llmActivity = {
		bucket_minutes: 60,
		bucket_count: 50,
		window_started_at: llmActivityStart.toISOString(),
		window_ended_at: new Date(
			llmActivityStart.getTime() + 50 * 3_600_000,
		).toISOString(),
		models: [
			{ model: "gpt-4o-mini", priority: 1, configured: true },
			{ model: "gpt-4.1-mini", priority: 2, configured: true },
		],
		buckets: Array.from({ length: 50 }, (_, index) => {
			const startedAt = new Date(
				llmActivityStart.getTime() + index * 3_600_000,
			);
			return {
				started_at: startedAt.toISOString(),
				ended_at: new Date(startedAt.getTime() + 3_600_000).toISOString(),
				counts: [
					{
						model: "gpt-4o-mini",
						succeeded: index === 49 ? 8 : index % 4 === 0 ? 3 : 0,
						failed: index === 49 ? 2 : 0,
					},
					{
						model: "gpt-4.1-mini",
						succeeded: index === 49 ? 2 : index % 3 === 0 ? 1 : 0,
						failed: index === 49 ? 1 : 0,
					},
				],
			};
		}),
	};
	const shiftedLlmActivity = (hours: number) => {
		const buckets = Array.from(
			{ length: llmActivity.buckets.length },
			(_, index) => {
				const source =
					llmActivity.buckets[
						Math.min(index + hours, llmActivity.buckets.length - 1)
					];
				const startedAt = new Date(
					llmActivityStart.getTime() + (index + hours) * 3_600_000,
				);
				return {
					...source,
					started_at: startedAt.toISOString(),
					ended_at: new Date(startedAt.getTime() + 3_600_000).toISOString(),
				};
			},
		);
		return {
			...llmActivity,
			window_started_at: buckets[0].started_at,
			window_ended_at: buckets.at(-1)?.ended_at ?? llmActivity.window_ended_at,
			buckets,
		};
	};
	const syncSubscriptionChainFinishedAt: Record<string, string> = {
		"task-subscriptions-1": "2026-02-26T14:50:30Z",
	};
	let syncRuntimeConfig = {
		sync_auto_fetch_interval_minutes: 10,
		retry_recent_failures_interval_minutes: 10,
		recent_sync_tasks: tasks
			.filter((task) => task.task_type === "sync.subscriptions")
			.filter((task) => task.id !== "task-subscriptions-skipped")
			.slice(0, 3)
			.map((task) => ({
				id: task.id,
				status: task.status,
				source: task.source,
				skipped: task.id === "task-subscriptions-skipped",
				duration_ms: syncSubscriptionChainFinishedAt[task.id]
					? new Date(syncSubscriptionChainFinishedAt[task.id]).getTime() -
						new Date(task.created_at).getTime()
					: null,
				created_at: task.created_at,
				started_at: task.started_at,
				finished_at: syncSubscriptionChainFinishedAt[task.id] ?? null,
			})),
	};

	const recentRunningWorkerUpdatedAt = new Date(
		Date.now() - 75_000,
	).toISOString();

	const completedTranslationRequest = {
		id: "req-translation-1",
		status: "completed",
		source: "feed.auto_translate",
		request_origin: "user",
		requested_by: CURRENT_USER_ID,
		scope_user_id: CURRENT_USER_ID,
		producer_ref: "feed.auto_translate:release:290978079",
		kind: "release_detail",
		variant: "feed_body",
		entity_id: "290978079",
		batch_id: "batch-translation-1",
		created_at: "2026-02-26T04:00:00Z",
		started_at: "2026-02-26T04:00:01Z",
		finished_at: "2026-02-26T04:00:03Z",
		updated_at: "2026-02-26T04:00:03Z",
	};

	const completedTranslationWorkers = [
		{
			worker_id: "translation-worker-1",
			worker_slot: 1,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:03Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-2",
			worker_slot: 2,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:03Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-3",
			worker_slot: 3,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:03Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-4",
			worker_slot: 4,
			worker_kind: "user_dedicated",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:03Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
	];

	const pendingTranslationWorkers = [
		{
			worker_id: "translation-worker-1",
			worker_slot: 1,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:00Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-2",
			worker_slot: 2,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:00Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-3",
			worker_slot: 3,
			worker_kind: "general",
			status: "idle",
			current_batch_id: null,
			request_count: 0,
			work_item_count: 0,
			trigger_reason: null,
			updated_at: "2026-02-26T04:00:00Z",
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
		{
			worker_id: "translation-worker-4",
			worker_slot: 4,
			worker_kind: "user_dedicated",
			status: "running",
			current_batch_id: "batch-translation-1",
			request_count: 1,
			work_item_count: 1,
			trigger_reason: "deadline",
			updated_at: recentRunningWorkerUpdatedAt,
			error_text: null,
			error_code: null,
			error_summary: null,
			error_detail: null,
		},
	];
	let translationRuntimeOverride: {
		general_worker_concurrency: number;
		dedicated_worker_concurrency: number;
		workers: typeof completedTranslationWorkers;
	} | null = null;

	function buildIdleTranslationWorkers(
		generalWorkerConcurrency: number,
		dedicatedWorkerConcurrency: number,
	) {
		return [
			...Array.from({ length: generalWorkerConcurrency }, (_, index) => ({
				worker_id: `translation-worker-general-${index + 1}`,
				worker_slot: index + 1,
				worker_kind: "general" as const,
				status: "idle" as const,
				current_batch_id: null,
				request_count: 0,
				work_item_count: 0,
				trigger_reason: null,
				updated_at: "2026-02-26T04:00:03Z",
				error_text: null,
				error_code: null,
				error_summary: null,
				error_detail: null,
			})),
			...Array.from({ length: dedicatedWorkerConcurrency }, (_, index) => ({
				worker_id: `translation-worker-user-dedicated-${index + 1}`,
				worker_slot: generalWorkerConcurrency + index + 1,
				worker_kind: "user_dedicated" as const,
				status: "idle" as const,
				current_batch_id: null,
				request_count: 0,
				work_item_count: 0,
				trigger_reason: null,
				updated_at: "2026-02-26T04:00:03Z",
				error_text: null,
				error_code: null,
				error_summary: null,
				error_detail: null,
			})),
		];
	}

	const completedTranslationRequestItem = {
		producer_ref: "feed.auto_translate:release:290978079",
		entity_id: "290978079",
		kind: "release_detail",
		variant: "feed_body",
		status: "ready",
		title_zh: "发布说明 290978079",
		summary_md: null,
		body_md: "- 修复了调度窗口\n- 保持单请求语义",
		error: null,
		error_code: null,
		error_summary: null,
		error_detail: null,
		work_item_id: "work-translation-1",
		batch_id: "batch-translation-1",
	};

	const completedTranslationBatch = {
		id: "batch-translation-1",
		status: "completed",
		trigger_reason: "deadline",
		worker_slot: 4,
		request_count: 1,
		item_count: 1,
		estimated_input_tokens: 512,
		created_at: "2026-02-26T04:00:01Z",
		started_at: "2026-02-26T04:00:01Z",
		finished_at: "2026-02-26T04:00:03Z",
		updated_at: "2026-02-26T04:00:03Z",
		result_summary: {
			ready: 1,
			error: 0,
			missing: 0,
			disabled: 0,
			queued: 0,
			running: 0,
		},
		business_outcome: {
			code: "ok",
			label: "业务成功",
			message: "批次与条目均已成功完成。",
		},
	};

	const completedTranslationBatchDetail = {
		batch: completedTranslationBatch,
		items: [completedTranslationRequestItem],
		llm_calls: [
			{
				id: "llm-translation-1",
				status: "succeeded",
				source: "translation.scheduler.deadline",
				model: "gpt-4o-mini",
				scheduler_wait_ms: 240,
				duration_ms: 820,
				created_at: "2026-02-26T04:00:01Z",
				failure_class: null,
				final_model: "gpt-4o-mini",
				fallback_count: 0,
				retry_scheduled_at: null,
				recovery_attempt_count: 0,
				attempt_history: [],
			},
		],
	};

	const emitTranslationEvents = options.emitTranslationEvents ?? false;
	let translationEventDelivered = !emitTranslationEvents;
	const emitLlmSchedulerEvents = options.emitLlmSchedulerEvents ?? false;
	let llmSchedulerEventDelivered = !emitLlmSchedulerEvents;
	const emitSubscriptionProgressEvents =
		options.emitSubscriptionProgressEvents ?? false;
	let subscriptionProgressEventDelivered = !emitSubscriptionProgressEvents;
	const translationViewResponseCounts = new Map<string, number>();

	function shouldServeCompletedTranslationView(key: string) {
		const nextCount = (translationViewResponseCounts.get(key) ?? 0) + 1;
		translationViewResponseCounts.set(key, nextCount);
		if (!emitTranslationEvents) {
			return true;
		}
		return translationEventDelivered && nextCount > 1;
	}

	function buildTranslationStatus(completed: boolean) {
		if (translationRuntimeOverride) {
			const busyWorkers = translationRuntimeOverride.workers.filter(
				(worker) => worker.status === "running",
			).length;
			const idleWorkers = translationRuntimeOverride.workers.filter(
				(worker) => worker.status === "idle",
			).length;
			return {
				scheduler_enabled: true,
				llm_enabled: true,
				scan_interval_ms: 250,
				batch_token_threshold: 1800,
				ai_model_context_limit: llmSchedulerStatus.ai_model_context_limit,
				effective_model_input_limit:
					llmSchedulerStatus.effective_model_input_limit,
				effective_model_input_limit_source:
					llmSchedulerStatus.effective_model_input_limit_source,
				general_worker_concurrency:
					translationRuntimeOverride.general_worker_concurrency,
				dedicated_worker_concurrency:
					translationRuntimeOverride.dedicated_worker_concurrency,
				worker_concurrency:
					translationRuntimeOverride.general_worker_concurrency +
					translationRuntimeOverride.dedicated_worker_concurrency,
				target_general_worker_concurrency:
					translationRuntimeOverride.general_worker_concurrency,
				target_dedicated_worker_concurrency:
					translationRuntimeOverride.dedicated_worker_concurrency,
				target_worker_concurrency:
					translationRuntimeOverride.general_worker_concurrency +
					translationRuntimeOverride.dedicated_worker_concurrency,
				idle_workers: idleWorkers,
				busy_workers: busyWorkers,
				workers: translationRuntimeOverride.workers,
				queued_requests: 0,
				queued_work_items: 0,
				running_batches: 0,
				requests_24h: 1,
				completed_batches_24h: 1,
				clean_completed_batches_24h: 1,
				completed_with_issues_batches_24h: 0,
				failed_batches_24h: 0,
				error_work_items_24h: 0,
				missing_work_items_24h: 0,
				avg_wait_ms_24h: 320,
				last_batch_finished_at: "2026-02-26T04:00:03Z",
			};
		}
		if (completed) {
			return {
				scheduler_enabled: true,
				llm_enabled: true,
				scan_interval_ms: 250,
				batch_token_threshold: 1800,
				ai_model_context_limit: llmSchedulerStatus.ai_model_context_limit,
				effective_model_input_limit:
					llmSchedulerStatus.effective_model_input_limit,
				effective_model_input_limit_source:
					llmSchedulerStatus.effective_model_input_limit_source,
				general_worker_concurrency: 3,
				dedicated_worker_concurrency: 1,
				worker_concurrency: 4,
				target_general_worker_concurrency: 3,
				target_dedicated_worker_concurrency: 1,
				target_worker_concurrency: 4,
				idle_workers: 4,
				busy_workers: 0,
				workers: completedTranslationWorkers,
				queued_requests: 0,
				queued_work_items: 0,
				running_batches: 0,
				requests_24h: 1,
				completed_batches_24h: 1,
				clean_completed_batches_24h: 1,
				completed_with_issues_batches_24h: 0,
				failed_batches_24h: 0,
				error_work_items_24h: 0,
				missing_work_items_24h: 0,
				avg_wait_ms_24h: 320,
				last_batch_finished_at: "2026-02-26T04:00:03Z",
			};
		}

		return {
			scheduler_enabled: true,
			llm_enabled: true,
			scan_interval_ms: 250,
			batch_token_threshold: 1800,
			ai_model_context_limit: llmSchedulerStatus.ai_model_context_limit,
			effective_model_input_limit:
				llmSchedulerStatus.effective_model_input_limit,
			effective_model_input_limit_source:
				llmSchedulerStatus.effective_model_input_limit_source,
			general_worker_concurrency: 3,
			dedicated_worker_concurrency: 1,
			worker_concurrency: 4,
			target_general_worker_concurrency: 3,
			target_dedicated_worker_concurrency: 1,
			target_worker_concurrency: 4,
			idle_workers: 3,
			busy_workers: 1,
			workers: pendingTranslationWorkers,
			queued_requests: 1,
			queued_work_items: 1,
			running_batches: 1,
			requests_24h: 1,
			completed_batches_24h: 0,
			clean_completed_batches_24h: 0,
			completed_with_issues_batches_24h: 0,
			failed_batches_24h: 0,
			error_work_items_24h: 0,
			missing_work_items_24h: 0,
			avg_wait_ms_24h: null,
			last_batch_finished_at: null,
		};
	}

	function buildTranslationRequests(completed: boolean) {
		if (completed) {
			return [completedTranslationRequest];
		}

		return [
			{
				...completedTranslationRequest,
				status: "queued",
				batch_id: null,
				started_at: null,
				finished_at: null,
				updated_at: "2026-02-26T04:00:00Z",
			},
		];
	}

	function buildTranslationRequestDetail(completed: boolean) {
		const [request] = buildTranslationRequests(completed);
		return {
			request,
			result: completed
				? completedTranslationRequestItem
				: {
						...completedTranslationRequestItem,
						status: "queued",
						title_zh: null,
						summary_md: null,
						batch_id: null,
					},
		};
	}

	function buildTranslationBatches(completed: boolean) {
		return completed ? [completedTranslationBatch] : [];
	}

	function buildTranslationBatchDetail() {
		return completedTranslationBatchDetail;
	}

	function buildTranslationAttemptEvents(entityId: string | null) {
		if (entityId !== "290978079") return [];
		return [
			{
				id: 1,
				work_item_id: "work-translation-1",
				request_id: "req-translation-1",
				batch_id: null,
				scope_user_id: CURRENT_USER_ID,
				kind: "release_detail",
				variant: "feed_body",
				entity_id: "290978079",
				target_lang: "zh-CN",
				attempt_no: 1,
				trigger: "initial",
				event_type: "attempt_queued",
				result_status: null,
				error_code: null,
				error_summary: null,
				failure_class: null,
				retry_eligible: false,
				next_retry_at: null,
				llm_call_ids: [],
				created_at: "2026-04-15T03:23:00Z",
			},
			{
				id: 2,
				work_item_id: "work-translation-1",
				request_id: null,
				batch_id: "batch-translation-1",
				scope_user_id: CURRENT_USER_ID,
				kind: "release_detail",
				variant: "feed_body",
				entity_id: "290978079",
				target_lang: "zh-CN",
				attempt_no: 1,
				trigger: "initial",
				event_type: "attempt_completed",
				result_status: "error",
				error_code: "markdown_structure_mismatch",
				error_summary: "Markdown 结构校验失败",
				failure_class: "transient",
				retry_eligible: true,
				next_retry_at: "2026-04-15T03:25:00Z",
				llm_call_ids: ["llm-call-1"],
				created_at: "2026-04-15T03:23:11Z",
			},
			{
				id: 3,
				work_item_id: "work-translation-1",
				request_id: null,
				batch_id: "batch-translation-1",
				scope_user_id: CURRENT_USER_ID,
				kind: "release_detail",
				variant: "feed_body",
				entity_id: "290978079",
				target_lang: "zh-CN",
				attempt_no: 2,
				trigger: "automatic_recovery",
				event_type: "retry_scheduled",
				result_status: "error",
				error_code: "markdown_structure_mismatch",
				error_summary: "Markdown 结构校验失败",
				failure_class: "transient",
				retry_eligible: true,
				next_retry_at: "2026-04-15T03:25:00Z",
				llm_call_ids: ["llm-call-1"],
				created_at: "2026-04-15T03:23:11Z",
			},
			{
				id: 4,
				work_item_id: "work-translation-1",
				request_id: null,
				batch_id: null,
				scope_user_id: CURRENT_USER_ID,
				kind: "release_detail",
				variant: "feed_body",
				entity_id: "290978079",
				target_lang: "zh-CN",
				attempt_no: 2,
				trigger: "automatic_recovery",
				event_type: "attempt_queued",
				result_status: null,
				error_code: null,
				error_summary: null,
				failure_class: null,
				retry_eligible: false,
				next_retry_at: null,
				llm_call_ids: [],
				created_at: "2026-04-15T03:25:00Z",
			},
		];
	}

	const slots = Array.from({ length: 24 }, (_, hour) => ({
		hour_utc: hour,
		enabled: hour % 2 === 0,
		last_dispatch_at: "2026-02-26T00:00:00Z",
		updated_at: "2026-02-26T00:00:00Z",
	}));

	const delayedPathSet = new Set(options.delayedPaths ?? []);
	const requestCounts = new Map<string, number>();
	const delayRuleCounts = new Map<number, number>();
	const failureRuleCounts = new Map<number, number>();
	const emitStreamEvents = options.emitStreamEvents ?? true;

	function matchesRule(rule: MockRequestRule, url: URL) {
		if (rule.pathname !== url.pathname) {
			return false;
		}
		if (!rule.search) {
			return true;
		}
		return url.search.includes(rule.search);
	}

	function ruleApplies(
		rule: MockRequestRule,
		url: URL,
		ruleCounts: Map<number, number>,
		index: number,
	) {
		if (!matchesRule(rule, url)) {
			return false;
		}
		const nextCount = (ruleCounts.get(index) ?? 0) + 1;
		ruleCounts.set(index, nextCount);
		const afterCount = rule.afterCount ?? 0;
		const times = rule.times ?? Number.POSITIVE_INFINITY;
		return nextCount > afterCount && nextCount <= afterCount + times;
	}

	async function maybeDelay(url: URL) {
		const pathname = url.pathname;
		const nextCount = (requestCounts.get(pathname) ?? 0) + 1;
		requestCounts.set(pathname, nextCount);
		if (
			options.responseDelayMs &&
			options.responseDelayMs > 0 &&
			delayedPathSet.has(pathname) &&
			nextCount > 1
		) {
			await sleep(options.responseDelayMs);
		}

		for (const [index, rule] of (options.delayRules ?? []).entries()) {
			if (ruleApplies(rule, url, delayRuleCounts, index)) {
				await sleep(rule.delayMs);
			}
		}
	}

	function nextFailure(url: URL) {
		for (const [index, rule] of (options.failureRules ?? []).entries()) {
			if (ruleApplies(rule, url, failureRuleCounts, index)) {
				return {
					status: rule.status ?? 500,
					message: rule.message ?? `mock failure for ${url.pathname}`,
				};
			}
		}
		return null;
	}

	let llmActivityRequestCount = 0;
	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		await maybeDelay(url);

		const failure = nextFailure(url);
		if (failure) {
			return json(
				route,
				{
					error: {
						message: failure.message,
					},
				},
				failure.status,
			);
		}

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: CURRENT_USER_ID,
					github_user_id: 10,
					login: currentUserLogin,
					name: "Octo Admin",
					avatar_url: null,
					email: "admin@example.com",
					is_admin: true,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/overview") {
			return json(route, {
				queued: 2,
				running: 1,
				failed_24h: 1,
				succeeded_24h: 3,
				enabled_scheduled_slots: slots.filter((slot) => slot.enabled).length,
				total_scheduled_slots: slots.length,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/realtime") {
			const status = url.searchParams.get("status") ?? "all";
			const taskType = url.searchParams.get("task_type") ?? "";
			const excludeTaskType = url.searchParams.get("exclude_task_type") ?? "";
			const taskGroup = url.searchParams.get("task_group") ?? "all";
			const filtered = tasks.filter((task) => {
				if (status !== "all" && task.status !== status) return false;
				if (taskType && task.task_type !== taskType) return false;
				if (excludeTaskType && task.task_type === excludeTaskType) return false;
				if (taskGroup === "scheduled") {
					return ["brief.daily_slot", "sync.subscriptions"].includes(
						task.task_type,
					);
				}
				if (taskGroup === "realtime") {
					return !["brief.daily_slot", "sync.subscriptions"].includes(
						task.task_type,
					);
				}
				return true;
			});
			return json(route, {
				items: filtered.map((task) => ({
					...task,
					skipped: task.id === "task-subscriptions-skipped",
				})),
				page: 1,
				page_size: 20,
				total: filtered.length,
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/events") {
			if (!emitStreamEvents) {
				return route.fulfill({
					status: 200,
					contentType: "text/event-stream",
					body: "",
				});
			}

			const call = llmCalls.find((item) => item.id === "llm-call-2");
			if (call && call.status === "running") {
				call.status = "succeeded";
				call.first_token_wait_ms = 140;
				call.duration_ms = 400;
				call.output_tokens = 160;
				call.total_tokens = 940;
				call.output_messages_json = JSON.stringify([
					{
						role: "assistant",
						content: "- added scheduler status endpoint\n- added call logging",
					},
				]);
				call.response_text = "ok";
				call.finished_at = "2026-02-26T03:00:01Z";
				call.updated_at = "2026-02-26T03:00:01Z";
			}

			const streamBody = [
				"event: job.event",
				`data: ${JSON.stringify({
					event_id: "evt-stream-9001",
					task_id: "task-running-1",
					task_type: "sync.releases",
					status: "running",
					event_type: "task.running",
					created_at: "2026-02-26T01:00:05Z",
				})}`,
				"",
				"event: llm.call",
				`data: ${JSON.stringify({
					event_id: "evt-stream-9101",
					call_id: "llm-call-2",
					status: "succeeded",
					source: "api.translate_releases_batch",
					requested_by: CURRENT_USER_ID,
					parent_task_id: null,
					event_type: "llm.succeeded",
					created_at: "2026-02-26T03:00:01Z",
				})}`,
				"",
			];

			if (emitLlmSchedulerEvents && !llmSchedulerEventDelivered) {
				await sleep(200);
				llmSchedulerEventDelivered = true;
				llmSchedulerStatus = {
					...llmSchedulerStatus,
					max_concurrency: 5,
					available_slots: Math.max(0, 5 - llmSchedulerStatus.in_flight_calls),
				};
				streamBody.push(
					"event: llm.scheduler",
					`data: ${JSON.stringify({
						event_id: "scheduler:2026-02-26T03:00:02Z:5:4:1:1",
						max_concurrency: 5,
						available_slots: 4,
						waiting_calls: llmSchedulerStatus.waiting_calls,
						in_flight_calls: llmSchedulerStatus.in_flight_calls,
						event_type: "llm.scheduler.updated",
						created_at: "2026-02-26T03:00:02Z",
					})}`,
					"",
				);
			}

			if (emitTranslationEvents && !translationEventDelivered) {
				await sleep(200);
				translationEventDelivered = true;
				streamBody.push(
					"event: translation.event",
					`data: ${JSON.stringify({
						event_id: "worker:2026-02-26T04:00:03Z:translation-worker-4",
						resource_type: "worker",
						resource_id: "translation-worker-4",
						status: "idle",
						event_type: "translation.worker.updated",
						created_at: "2026-02-26T04:00:03Z",
					})}`,
					"",
					"event: translation.event",
					`data: ${JSON.stringify({
						event_id: "request:2026-02-26T04:00:03Z:req-translation-1",
						resource_type: "request",
						resource_id: "req-translation-1",
						status: "completed",
						event_type: "translation.request.updated",
						created_at: "2026-02-26T04:00:03Z",
					})}`,
					"",
					"event: translation.event",
					`data: ${JSON.stringify({
						event_id: "batch:2026-02-26T04:00:03Z:batch-translation-1",
						resource_type: "batch",
						resource_id: "batch-translation-1",
						status: "completed",
						event_type: "translation.batch.updated",
						created_at: "2026-02-26T04:00:03Z",
					})}`,
					"",
				);
			}

			if (
				emitSubscriptionProgressEvents &&
				!subscriptionProgressEventDelivered
			) {
				await sleep(200);
				subscriptionProgressEventDelivered = true;
				const subscription = tasks.find(
					(task) => task.id === "task-subscriptions-1",
				);
				if (subscription) {
					subscription.status = "running";
					subscription.finished_at = null;
					subscription.updated_at = "2026-02-26T14:37:30Z";
				}
				streamBody.push(
					"event: job.event",
					`data: ${JSON.stringify({
						event_id: "evt-stream-subscription-progress-1",
						task_id: "task-subscriptions-1",
						task_type: "sync.subscriptions",
						status: "running",
						event_type: "task.progress",
						created_at: "2026-02-26T14:37:30Z",
					})}`,
					"",
				);
			}

			streamBody.push("");
			return route.fulfill({
				status: 200,
				contentType: "text/event-stream",
				body: streamBody.join("\n"),
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/log")
		) {
			return route.fulfill({
				status: 200,
				contentType: "application/x-ndjson",
				body: '{"line":1}\n{"line":2}\n',
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			!pathname.endsWith("/log")
		) {
			const taskId = pathname.split("/").at(-1) ?? "";
			const task = tasks.find((item) => item.id === taskId);
			if (!task) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "task not found" },
					},
					404,
				);
			}
			if (taskId === "task-translate-batch-1") {
				return json(route, {
					task: {
						...task,
						payload_json: JSON.stringify({
							user_id: CURRENT_USER_ID,
							release_ids: ["290978079", "290980132"],
						}),
						result_json: JSON.stringify({
							total: 2,
							ready: 0,
							missing: 0,
							disabled: 0,
							error: 2,
						}),
					},
					event_meta: {
						returned: 2,
						total: 4,
						limit: 2,
						truncated: true,
					},
					diagnostics: {
						business_outcome: {
							code: "failed",
							label: "业务失败",
							message: "任务运行完成，但全部翻译项失败。",
						},
						translate_release_batch: {
							target_user_id: CURRENT_USER_ID,
							release_total: 2,
							summary: {
								total: 2,
								ready: 0,
								missing: 0,
								disabled: 0,
								error: 2,
							},
							progress: {
								processed: 2,
								last_stage: "release",
							},
							items: [
								{
									release_id: "290978079",
									item_status: "error",
									item_error: "translation failed",
									last_event_at: "2026-02-26T01:10:30Z",
								},
								{
									release_id: "290980132",
									item_status: "error",
									item_error: "translation failed",
									last_event_at: "2026-02-26T01:10:32Z",
								},
							],
						},
					},
					events: [
						{
							id: "evt-task-20",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "release",
								release_id: "290978079",
								item_status: "error",
								item_error: "translation failed",
							}),
							created_at: "2026-02-26T01:10:30Z",
						},
						{
							id: "evt-task-21",
							event_type: "task.completed",
							payload_json: JSON.stringify({
								status: "succeeded",
							}),
							created_at: "2026-02-26T01:10:40Z",
						},
					],
				});
			}

			if (taskId === "task-subscriptions-1") {
				const liveSubscriptionProgress = subscriptionProgressEventDelivered;
				return json(route, {
					task: {
						...task,
						status: liveSubscriptionProgress ? "running" : task.status,
						finished_at: liveSubscriptionProgress ? null : task.finished_at,
						updated_at: liveSubscriptionProgress
							? "2026-02-26T14:37:30Z"
							: task.updated_at,
						payload_json: JSON.stringify({
							trigger: "schedule",
							schedule_key: "2026-02-26T14:30",
						}),
						result_json: liveSubscriptionProgress
							? null
							: JSON.stringify({
									skipped: false,
									skip_reason: null,
									star: {
										total_users: 12,
										succeeded_users: 11,
										failed_users: 1,
										total_repos: 340,
									},
									release: {
										total_repos: 128,
										succeeded_repos: 123,
										failed_repos: 5,
										candidate_failures: 7,
									},
									social: {
										total_users: 11,
										succeeded_users: 9,
										failed_users: 2,
										repo_stars: 48,
										followers: 19,
										events: 67,
									},
									notifications: {
										total_users: 11,
										succeeded_users: 10,
										failed_users: 1,
										notifications: 192,
									},
									releases_written: 1840,
									critical_events: 6,
								}),
					},
					event_meta: {
						returned: 4,
						total: 4,
						limit: 200,
						truncated: false,
					},
					diagnostics: {
						business_outcome: {
							code: "partial",
							label: "部分成功",
							message: "任务已完成，但存在失败或关键告警，请查看最近关键事件。",
						},
						sync_subscriptions: {
							trigger: "schedule",
							schedule_key: "2026-02-26T14:30",
							skipped: false,
							skip_reason: null,
							log_available: true,
							log_download_path:
								"/api/admin/jobs/realtime/task-subscriptions-1/log",
							star: {
								total_users: 12,
								succeeded_users: 11,
								failed_users: 1,
								total_repos: 340,
							},
							release: {
								total_repos: 128,
								succeeded_repos: liveSubscriptionProgress ? 79 : 123,
								failed_repos: liveSubscriptionProgress ? 4 : 5,
								candidate_failures: liveSubscriptionProgress ? 5 : 7,
								fetched_count: liveSubscriptionProgress ? 924 : 1840,
								inserted_count: liveSubscriptionProgress ? 148 : 320,
								updated_count: liveSubscriptionProgress ? 41 : 96,
								unchanged_count: liveSubscriptionProgress ? 735 : 1424,
								pages_fetched: liveSubscriptionProgress ? 79 : 123,
							},
							social: {
								total_users: 11,
								succeeded_users: 9,
								failed_users: 2,
								repo_stars: 48,
								followers: 19,
								events: 67,
							},
							notifications: {
								total_users: 11,
								succeeded_users: 10,
								failed_users: 1,
								notifications: 192,
							},
							releases_written: liveSubscriptionProgress ? 924 : 1840,
							critical_events: 6,
							recent_events: [
								{
									id: "evt-sync-42",
									stage: "release",
									event_type: "repo_inaccessible",
									severity: "error",
									recoverable: false,
									attempt: 1,
									user_id: RECENT_EVENT_USER_ID,
									repo_id: 9001,
									repo_full_name: "octo/private-repo",
									message:
										"release sync candidate failed for octo/private-repo with user #4h6p9s3t5z8e2x4c",
									created_at: "2026-02-26T14:31:40Z",
								},
								{
									id: "evt-sync-41",
									stage: "social",
									event_type: "social_sync_failed",
									severity: "error",
									recoverable: false,
									attempt: 1,
									user_id: RECENT_EVENT_USER_ID,
									repo_id: null,
									repo_full_name: null,
									message:
										"failed to refresh social activity for user #4h6p9s3t5z8e2x4c",
									created_at: "2026-02-26T14:32:10Z",
								},
								{
									id: "evt-sync-40",
									stage: "notifications",
									event_type: "notifications_sync_failed",
									severity: "error",
									recoverable: false,
									attempt: 1,
									user_id: RECENT_EVENT_USER_ID,
									repo_id: null,
									repo_full_name: null,
									message:
										"failed to refresh inbox notifications for user #4h6p9s3t5z8e2x4c",
									created_at: "2026-02-26T14:32:40Z",
								},
							],
						},
					},
					events: [
						{
							id: "evt-task-31",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "collect",
								total_users: 12,
							}),
							created_at: "2026-02-26T14:30:05Z",
						},
						{
							id: "evt-task-32",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "star_summary",
								total_users: 12,
								succeeded_users: 11,
								failed_users: 1,
							}),
							created_at: "2026-02-26T14:31:02Z",
						},
						{
							id: "evt-task-33",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: liveSubscriptionProgress
									? "release_progress"
									: "release_summary",
								total_repos: 128,
								succeeded_repos: liveSubscriptionProgress ? 79 : 123,
								failed_repos: liveSubscriptionProgress ? 4 : 5,
								pending_repos: liveSubscriptionProgress ? 45 : 0,
								releases_written: liveSubscriptionProgress ? 924 : 1840,
								fetched_count: liveSubscriptionProgress ? 924 : 1840,
								inserted_count: liveSubscriptionProgress ? 148 : 320,
								updated_count: liveSubscriptionProgress ? 41 : 96,
								unchanged_count: liveSubscriptionProgress ? 735 : 1424,
								pages_fetched: liveSubscriptionProgress ? 79 : 123,
							}),
							created_at: liveSubscriptionProgress
								? "2026-02-26T14:37:30Z"
								: "2026-02-26T14:37:59Z",
						},
						{
							id: "evt-task-34",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "social_summary",
								total_users: 11,
								succeeded_users: 9,
								failed_users: 2,
								repo_stars: 48,
								followers: 19,
								events: 67,
							}),
							created_at: "2026-02-26T14:38:02Z",
						},
						{
							id: "evt-task-35",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "notifications_summary",
								total_users: 11,
								succeeded_users: 10,
								failed_users: 1,
								notifications: 192,
							}),
							created_at: "2026-02-26T14:38:06Z",
						},
						{
							id: "evt-task-36",
							event_type: "task.completed",
							payload_json: JSON.stringify({ status: "succeeded" }),
							created_at: "2026-02-26T14:38:10Z",
						},
					],
				});
			}

			if (taskId === "task-subscriptions-skipped") {
				return json(route, {
					task: {
						...task,
						payload_json: JSON.stringify({
							trigger: "schedule",
							schedule_key: "2026-02-26T14:40",
						}),
						result_json: JSON.stringify({
							skipped: true,
							skip_reason: "previous_run_active",
							star: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								total_repos: 0,
							},
							release: {
								total_repos: 0,
								succeeded_repos: 0,
								failed_repos: 0,
								candidate_failures: 0,
							},
							social: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								repo_stars: 0,
								followers: 0,
								events: 0,
							},
							notifications: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								notifications: 0,
							},
							releases_written: 0,
							critical_events: 0,
						}),
					},
					event_meta: {
						returned: 2,
						total: 2,
						limit: 200,
						truncated: false,
					},
					diagnostics: {
						business_outcome: {
							code: "disabled",
							label: "已跳过",
							message: "上一轮订阅同步仍在执行，本轮仅记录跳过结果。",
						},
						sync_subscriptions: {
							trigger: "schedule",
							schedule_key: "2026-02-26T14:40",
							skipped: true,
							skip_reason: "previous_run_active",
							log_available: false,
							log_download_path: null,
							star: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								total_repos: 0,
							},
							release: {
								total_repos: 0,
								succeeded_repos: 0,
								failed_repos: 0,
								candidate_failures: 0,
							},
							social: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								repo_stars: 0,
								followers: 0,
								events: 0,
							},
							notifications: {
								total_users: 0,
								succeeded_users: 0,
								failed_users: 0,
								notifications: 0,
							},
							releases_written: 0,
							critical_events: 0,
							recent_events: [],
						},
					},
					events: [
						{
							id: "evt-skipped-1",
							event_type: "task.progress",
							payload_json: JSON.stringify({
								stage: "skipped",
								skip_reason: "previous_run_active",
							}),
							created_at: "2026-02-26T14:40:01Z",
						},
						{
							id: "evt-skipped-2",
							event_type: "task.completed",
							payload_json: JSON.stringify({
								status: "succeeded",
								skipped: true,
							}),
							created_at: "2026-02-26T14:40:01Z",
						},
					],
				});
			}

			return json(route, {
				task: {
					...task,
					payload_json: JSON.stringify({
						task_id: task.id,
						status: task.status,
					}),
					result_json:
						task.status === "failed" ? null : JSON.stringify({ ok: true }),
				},
				event_meta: {
					returned: 1,
					total: 1,
					limit: 200,
					truncated: false,
				},
				events: [
					{
						id: "evt-task-1",
						event_type: "task.created",
						payload_json: JSON.stringify({
							task_id: task.id,
							status: task.status,
						}),
						created_at: "2026-02-26T00:00:00Z",
					},
				],
			});
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/status") {
			return json(route, llmSchedulerStatus);
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/activity") {
			llmActivityRequestCount += 1;
			return json(
				route,
				options.activityRefreshOnSecondRequest && llmActivityRequestCount > 1
					? shiftedLlmActivity(llmActivityRequestCount - 1)
					: llmActivity,
			);
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/sync/runtime-config"
		) {
			return json(route, syncRuntimeConfig);
		}

		if (
			req.method() === "PATCH" &&
			pathname === "/api/admin/jobs/sync/runtime-config"
		) {
			const body = (req.postDataJSON() ?? {}) as {
				sync_auto_fetch_interval_minutes?: number;
				retry_recent_failures_interval_minutes?: number;
			};
			syncRuntimeConfig = {
				...syncRuntimeConfig,
				sync_auto_fetch_interval_minutes: Number(
					body.sync_auto_fetch_interval_minutes ?? 60,
				),
				retry_recent_failures_interval_minutes: Number(
					body.retry_recent_failures_interval_minutes ??
						syncRuntimeConfig.retry_recent_failures_interval_minutes,
				),
			};
			return json(route, syncRuntimeConfig);
		}

		if (
			req.method() === "PATCH" &&
			pathname === "/api/admin/jobs/llm/runtime-config"
		) {
			const body = (req.postDataJSON() ?? {}) as {
				max_concurrency?: number;
				ai_model_context_limit?: number | null;
				llm_models?: string[];
			};
			const maxConcurrency = Number(body.max_concurrency ?? 1);
			const hasModelContextLimit = Object.hasOwn(
				body,
				"ai_model_context_limit",
			);
			const aiModelContextLimit = hasModelContextLimit
				? typeof body.ai_model_context_limit === "number"
					? Number(body.ai_model_context_limit)
					: null
				: llmSchedulerStatus.ai_model_context_limit;
			const nextModels =
				body.llm_models && body.llm_models.length > 0
					? body.llm_models
					: llmSchedulerStatus.llm_models;
			llmSchedulerStatus = {
				...llmSchedulerStatus,
				llm_models: nextModels,
				selected_model_for_new_calls: nextModels[0] ?? null,
				max_concurrency: maxConcurrency,
				ai_model_context_limit: aiModelContextLimit,
				effective_model_input_limit: aiModelContextLimit ?? 128000,
				effective_model_input_limit_source:
					aiModelContextLimit === null ? "builtin_catalog" : "admin_override",
				model_statuses: nextModels.map((model, index) => ({
					model,
					priority: index + 1,
					status: "ready",
					consecutive_final_failures: 0,
					cooldown_until: null,
					effective_input_limit:
						aiModelContextLimit ??
						(model === "gpt-4.1-mini" ? 1047576 : 128000),
					effective_input_limit_source:
						aiModelContextLimit === null ? "builtin_catalog" : "admin_override",
				})),
				available_slots: Math.max(
					0,
					maxConcurrency - llmSchedulerStatus.in_flight_calls,
				),
			};
			return json(route, llmSchedulerStatus);
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/llm/calls") {
			const status = url.searchParams.get("status") ?? "all";
			const model = url.searchParams.get("model") ?? "";
			const source = url.searchParams.get("source") ?? "";
			const requestedBy = url.searchParams.get("requested_by");
			const parentTaskId = url.searchParams.get("parent_task_id") ?? "";
			const startedFrom = url.searchParams.get("started_from");
			const startedTo = url.searchParams.get("started_to");
			const finishedFrom = url.searchParams.get("finished_from");
			const finishedBefore = url.searchParams.get("finished_before");
			const sort = url.searchParams.get("sort") ?? "created_desc";
			const timestamp = (value: string | null | undefined) =>
				value ? new Date(value).getTime() : Number.NaN;
			const filtered = llmCalls.filter((item) => {
				if (status !== "all" && item.status !== status) return false;
				if (model && item.model !== model) return false;
				if (source && item.source !== source) return false;
				if (
					requestedBy &&
					String(item.requested_by ?? "") !== String(requestedBy)
				) {
					return false;
				}
				if (parentTaskId && item.parent_task_id !== parentTaskId) {
					return false;
				}
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
			const statusRank = (value: string) =>
				value === "running" ? 0 : value === "queued" ? 1 : 2;
			const sorted = [...filtered].sort((left, right) => {
				if (sort === "status_grouped" && status === "all") {
					const rankDifference =
						statusRank(left.status) - statusRank(right.status);
					if (rankDifference !== 0) return rankDifference;
				}
				return (
					new Date(right.created_at).getTime() -
						new Date(left.created_at).getTime() ||
					right.created_at.localeCompare(left.created_at) ||
					right.id.localeCompare(left.id)
				);
			});
			const pageNumber = Math.max(
				1,
				Number(url.searchParams.get("page") ?? "1"),
			);
			const pageSize = Math.min(
				100,
				Math.max(1, Number(url.searchParams.get("page_size") ?? "20")),
			);
			const pageItems = sorted.slice(
				(pageNumber - 1) * pageSize,
				pageNumber * pageSize,
			);
			return json(route, {
				items: pageItems.map(
					({
						prompt_text: _p,
						response_text: _r,
						error_text: _e,
						input_messages_json: _im,
						output_messages_json: _om,
						...rest
					}) => rest,
				),
				page: pageNumber,
				page_size: pageSize,
				total: filtered.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/llm/calls/")
		) {
			const callId = pathname.split("/").at(-1) ?? "";
			if (callId === "llm-translation-1") {
				return json(route, {
					id: "llm-translation-1",
					status: "succeeded",
					source: "translation.scheduler.deadline",
					model: "gpt-4o-mini",
					requested_by: null,
					parent_task_id: null,
					parent_task_type: null,
					max_tokens: 900,
					attempt_count: 1,
					scheduler_wait_ms: 240,
					first_token_wait_ms: 120,
					duration_ms: 820,
					input_tokens: 420,
					output_tokens: 110,
					cached_input_tokens: 0,
					total_tokens: 530,
					input_messages_json: JSON.stringify([
						{ role: "user", content: "translate grouped items" },
					]),
					output_messages_json: JSON.stringify([
						{ role: "assistant", content: "- grouped result" },
					]),
					prompt_text: "translate grouped items",
					response_text: "- grouped result",
					error_text: null,
					failure_class: null,
					final_model: "gpt-4o-mini",
					fallback_count: 0,
					retry_scheduled_at: null,
					recovery_attempt_count: 0,
					attempt_history: [],
					created_at: "2026-02-26T04:00:01Z",
					started_at: "2026-02-26T04:00:01Z",
					finished_at: "2026-02-26T04:00:02Z",
					updated_at: "2026-02-26T04:00:02Z",
				});
			}
			const item = llmCalls.find((call) => call.id === callId);
			if (!item) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "llm call not found" },
					},
					404,
				);
			}
			return json(route, item);
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/retry")
		) {
			return json(route, { task_id: "retry-1", status: "queued" });
		}

		if (
			req.method() === "POST" &&
			pathname.startsWith("/api/admin/jobs/realtime/") &&
			pathname.endsWith("/cancel")
		) {
			return json(route, {
				task_id: pathname.split("/").at(-2),
				status: "running",
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/status"
		) {
			return json(
				route,
				buildTranslationStatus(shouldServeCompletedTranslationView("status")),
			);
		}

		if (
			req.method() === "PATCH" &&
			pathname === "/api/admin/jobs/translations/runtime-config"
		) {
			const body = (req.postDataJSON() ?? {}) as {
				general_worker_concurrency?: number;
				dedicated_worker_concurrency?: number;
			};
			translationRuntimeOverride = {
				general_worker_concurrency: Number(
					body.general_worker_concurrency ?? 1,
				),
				dedicated_worker_concurrency: Number(
					body.dedicated_worker_concurrency ?? 1,
				),
				workers: buildIdleTranslationWorkers(
					Number(body.general_worker_concurrency ?? 1),
					Number(body.dedicated_worker_concurrency ?? 1),
				),
			};
			return json(route, buildTranslationStatus(true));
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/requests"
		) {
			const requests = buildTranslationRequests(
				shouldServeCompletedTranslationView("requests"),
			);
			return json(route, {
				items: requests,
				page: 1,
				page_size: 20,
				total: requests.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/attempt-events"
		) {
			const page = Math.max(1, Number(url.searchParams.get("page") ?? "1"));
			const pageSize = Math.max(
				1,
				Number(url.searchParams.get("page_size") ?? "20"),
			);
			const items =
				url.searchParams.get("kind") === "release_detail" &&
				url.searchParams.get("variant") === "feed_body"
					? buildTranslationAttemptEvents(url.searchParams.get("entity_id"))
					: [];
			const offset = (page - 1) * pageSize;
			return json(route, {
				items: items.slice(offset, offset + pageSize),
				page,
				page_size: pageSize,
				total: items.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/translations/requests/")
		) {
			return json(
				route,
				buildTranslationRequestDetail(
					shouldServeCompletedTranslationView("request_detail"),
				),
			);
		}

		if (
			req.method() === "GET" &&
			pathname === "/api/admin/jobs/translations/batches"
		) {
			const batches = buildTranslationBatches(
				shouldServeCompletedTranslationView("batches"),
			);
			return json(route, {
				items: batches,
				page: 1,
				page_size: 20,
				total: batches.length,
			});
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/admin/jobs/translations/batches/")
		) {
			return json(route, buildTranslationBatchDetail());
		}

		if (req.method() === "GET" && pathname === "/api/admin/jobs/scheduled") {
			return json(route, { items: slots });
		}

		if (
			req.method() === "PATCH" &&
			pathname.startsWith("/api/admin/jobs/scheduled/")
		) {
			const hour = Number(pathname.split("/").at(-1));
			const body = req.postDataJSON() as { enabled: boolean };
			const target = slots.find((slot) => slot.hour_utc === hour);
			if (!target) {
				return json(
					route,
					{
						ok: false,
						error: { code: "not_found", message: "slot not found" },
					},
					404,
				);
			}
			target.enabled = body.enabled;
			return json(route, target);
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});
}

test("admin can manage jobs center", async ({ page }) => {
	test.slow();
	await installAdminJobsMocks(page, { emitSubscriptionProgressEvents: true });
	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });

	const realtimeTab = page.getByRole("tab", { name: "实时异步任务" });
	const scheduledTab = page.getByRole("tab", { name: "定时任务" });
	const subscriptionsTab = page.getByRole("tab", { name: "订阅同步" });
	const llmTab = page.getByRole("tab", { name: "LLM调度" });

	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await expect(
		page.getByRole("link", { name: "OctoRill 管理后台" }),
	).toBeVisible();
	await expect(
		page.getByRole("navigation", { name: "管理员导航" }),
	).toBeVisible();
	await expect(page.getByRole("heading", { name: "任务总览" })).toBeVisible();
	await expect(realtimeTab).toHaveAttribute("aria-selected", "true");
	await expect(
		page.getByRole("combobox", { name: "实时异步任务状态筛选" }),
	).toBeVisible();

	const realtimeHelp = page.getByRole("button", { name: "实时异步任务说明" });
	await realtimeHelp.hover();
	await expect(page.getByRole("tooltip")).toContainText(
		"监控系统内部任务，并支持重试与取消。",
	);

	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("brief.daily_slot")).toHaveCount(0);
	await expect(page.getByText("sync.subscriptions")).toHaveCount(0);
	await page.getByRole("button", { name: "详情" }).first().click();
	const taskSheet = page.getByRole("dialog", { name: "任务详情" });
	await expect(taskSheet).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-running-1\?from=realtime$/,
	);
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(taskSheet).toHaveCount(0);
	await expect(page).toHaveURL(/\/admin\/jobs$/);

	const translateTaskCard = page
		.getByText("ID: task-translate-batch-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await translateTaskCard.getByRole("button", { name: "详情" }).click();
	await expect(taskSheet).toBeVisible();
	await expect(
		page.getByText("task-translate-batch-1", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("业务结果：业务失败")).toBeVisible();
	await expect(
		page.getByText("仅展示最近 2 条事件（已加载 2/4）。"),
	).toBeVisible();
	await expect(
		page.getByText("Release #290978079 · error · translation failed"),
	).toBeVisible();
	const taskLlmDetailButton = page.getByRole("button", {
		name: "查看 LLM 详情",
	});
	await taskLlmDetailButton.scrollIntoViewIfNeeded();
	await taskLlmDetailButton.click();
	const taskLlmSheet = page.getByRole("dialog", {
		name: "任务详情 · LLM 调用详情",
	});
	await expect(taskLlmSheet).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-translate-batch-1\/llm\/llm-call-2\?from=realtime$/,
	);
	await expect(
		page.getByText("来源：api.translate_releases_batch"),
	).toBeVisible();
	await page.getByRole("button", { name: "返回任务详情" }).click();
	await expect(taskSheet).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-translate-batch-1\?from=realtime$/,
	);
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(taskSheet).toHaveCount(0);
	await expect(page).toHaveURL(/\/admin\/jobs$/);

	await scheduledTab.click();
	await expect(page).toHaveURL(/\/admin\/jobs\/scheduled$/);
	await expect(scheduledTab).toHaveAttribute("aria-selected", "true");
	await expect(
		page.getByRole("combobox", { name: "定时任务状态筛选" }),
	).toBeVisible();
	await expect(page.getByRole("heading", { name: "定时任务" })).toBeVisible();
	await expect(page.getByText("定时日报")).toBeVisible();
	await expect(
		page.getByText("sync.subscriptions", { exact: true }).first(),
	).toBeVisible();
	await expect(
		page.getByText("brief.daily_slot", { exact: true }).first(),
	).toBeVisible();
	const subscriptionTaskCard = page
		.getByText("ID: task-subscriptions-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await expect(subscriptionTaskCard.getByText("订阅同步")).toBeVisible();
	await subscriptionTaskCard.getByRole("button", { name: "详情" }).click();
	await expect(
		page.getByText("task-subscriptions-1", { exact: true }),
	).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-subscriptions-1\?from=scheduled$/,
	);
	await expect(page.getByText("Social 成功/总计")).toBeVisible();
	await expect(page.getByText("9/11", { exact: true })).toBeVisible();
	await expect(page.getByText("Inbox 成功/总计")).toBeVisible();
	await expect(page.getByText("10/11", { exact: true })).toBeVisible();
	await expect(page.getByText("最近关键事件", { exact: true })).toBeVisible();
	await expect(page.getByRole("link", { name: "下载日志" })).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(page).toHaveURL(/\/admin\/jobs\/scheduled$/);
	await expect(page.getByText("执行时间配置（24小时槽）")).toHaveCount(0);

	await subscriptionsTab.click();
	await expect(page).toHaveURL(/\/admin\/jobs\/subscriptions$/);
	await expect(subscriptionsTab).toHaveAttribute("aria-selected", "true");
	const subscriptionWorkflowCard = page
		.getByText("ID: task-subscriptions-1")
		.locator("xpath=ancestor::a[@data-testid='subscription-workflow-card'][1]");
	await expect(subscriptionWorkflowCard).toBeVisible();
	await expect(subscriptionWorkflowCard).toHaveAttribute(
		"href",
		"/admin/jobs/subscriptions/task-subscriptions-1",
	);
	await subscriptionWorkflowCard.click();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/subscriptions\/task-subscriptions-1$/,
	);
	await expect(
		page.getByRole("heading", { name: "订阅同步工作流详情" }),
	).toBeVisible();
	await expect(
		page.locator("#subscription-stage-release").getByText("Release Queue"),
	).toBeVisible();
	await expect(page.getByText("79")).toBeVisible();
	await expect(page.getByText("924")).toBeVisible();
	await expect(
		page.getByText("octo/private-repo · attempt 1 · terminal"),
	).toBeVisible();
	await expect(
		page.getByRole("button", { name: "配置订阅同步设置" }),
	).toBeVisible();
	await page.getByRole("button", { name: "配置订阅同步设置" }).click();
	const syncSettingsDialog = page.getByRole("dialog", {
		name: "订阅同步设置",
	});
	await expect(syncSettingsDialog).toBeVisible();
	await expect(
		syncSettingsDialog.getByRole("slider", { name: "Release 抓取并发" }),
	).toBeVisible();
	const freshnessHelp = syncSettingsDialog.getByRole("button", {
		name: "Dashboard 手动刷新新鲜度策略说明",
	});
	await freshnessHelp.hover();
	await expect(page.getByRole("tooltip")).toContainText(
		"仅作用于 Dashboard 全量同步，单仓窗口始终限制在 1–30 分钟。",
	);
	await expect(page.getByRole("tooltip")).toContainText(
		"压力只会延长复用窗口，不会跳过有效关注仓库。",
	);
	await expect(
		syncSettingsDialog.getByText("task-subscriptions-skipped"),
	).toHaveCount(0);
	await syncSettingsDialog.getByRole("button", { name: "取消" }).click();
	await expect(syncSettingsDialog).toHaveCount(0);
	await page.getByRole("button", { name: "返回订阅同步列表" }).click();
	await expect(page).toHaveURL(/\/admin\/jobs\/subscriptions$/);
	await expect(
		page.getByRole("heading", { name: "订阅同步工作流详情" }),
	).toHaveCount(0);
	await expect(page.getByRole("heading", { name: "订阅同步" })).toBeVisible();

	await llmTab.click();
	await expect(page).toHaveURL(/\/admin\/jobs\/llm$/);
	await expect(llmTab).toHaveAttribute("aria-selected", "true");
	await expect(page.getByRole("heading", { name: "LLM 调度" })).toBeVisible();
	await expect(
		page.getByRole("combobox", { name: "LLM 调用状态筛选" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 调用来源筛选" }),
	).toBeVisible();
	const startedRangeTrigger = page.getByRole("button", {
		name: "LLM 开始时间范围",
	});
	const finishedRangeTrigger = page.getByRole("button", {
		name: "LLM 结束时间范围",
	});
	await expect(startedRangeTrigger).toBeVisible();
	await expect(finishedRangeTrigger).toBeVisible();
	await startedRangeTrigger.click();
	await expect(
		page.getByRole("group", { name: "LLM 开始时间后日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("group", { name: "LLM 开始时间前日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 开始时间后" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 开始时间前" }),
	).toBeVisible();
	await page.keyboard.press("Escape");
	await finishedRangeTrigger.click();
	await expect(
		page.getByRole("group", { name: "LLM 结束时间后日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("group", { name: "LLM 结束时间前（不含）日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 结束时间后" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 结束时间前（不含）" }),
	).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(page.getByText("调度器状态")).toHaveCount(0);
	await expect(page.getByText("等待 / 进行中")).toHaveCount(0);
	await expect(page.getByText("近24h 调用 / 失败")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await page
		.getByRole("textbox", { name: "LLM 调用来源筛选" })
		.fill("job.api.translate_release");
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);

	const llmCallCard = page
		.getByText("ID: llm-call-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await llmCallCard.getByRole("button", { name: "详情" }).click();
	const llmSheet = page.getByRole("dialog", { name: "LLM 调用详情" });
	await expect(llmSheet).toBeVisible();
	await expect(page.getByText("Conversation Timeline")).toBeVisible();
	await expect(page.getByText("Input Messages")).toHaveCount(0);
	await expect(page.getByText("耗时 / 重试")).toBeVisible();
	await expect(page.getByText("等待 / 首字 / 耗时 / 重试")).toHaveCount(0);
	await expect(
		page.getByText("等待 1.20s · 首字 860ms", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("Token（输入 / 输出 / 缓存）")).toBeVisible();
	await page.getByRole("button", { name: "查看父任务" }).click();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/tasks\/task-running-1\?from=llm$/,
	);
	await expect(taskSheet).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(page).toHaveURL(/\/admin\/jobs\/llm$/);

	const llmCallCardAgain = page
		.getByText("ID: llm-call-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await llmCallCardAgain.getByRole("button", { name: "详情" }).click();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(llmSheet).not.toBeVisible();
});

test("subscription workflow cards keep a neutral frame without extra left-edge decoration", async ({
	page,
}) => {
	await installAdminJobsMocks(page);

	await page.goto("/admin/jobs/subscriptions");

	const taskIds = ["task-subscriptions-1", "task-subscriptions-skipped"];
	const cards = taskIds.map((taskId) =>
		page.locator(
			`[data-testid="subscription-workflow-card"][data-task-id="${taskId}"]`,
		),
	);
	for (const card of cards) {
		await expect(card).toBeVisible();
	}

	const frameStyles = await Promise.all(
		cards.map((card) =>
			card.evaluate((element) => {
				const style = getComputedStyle(element);
				return {
					borderLeftColor: style.borderLeftColor,
					borderLeftWidth: style.borderLeftWidth,
					borderRightColor: style.borderRightColor,
					borderRightWidth: style.borderRightWidth,
					borderTopColor: style.borderTopColor,
					borderTopWidth: style.borderTopWidth,
					borderRadius: style.borderRadius,
				};
			}),
		),
	);
	expect(
		new Set(frameStyles.map((styles) => styles.borderLeftColor)).size,
	).toBe(1);
	for (const styles of frameStyles) {
		expect(styles.borderTopColor).toBe(styles.borderLeftColor);
		expect(styles.borderRightColor).toBe(styles.borderLeftColor);
		expect(styles.borderTopWidth).toBe("0px");
		expect(styles.borderLeftWidth).toBe("0px");
		expect(styles.borderRightWidth).toBe("0px");
		expect(styles.borderRadius).toBe("0px");
	}
	for (const card of cards) {
		await expect(
			card.getByTestId("subscription-workflow-card-accent"),
		).toHaveCount(0);
		const stageGrid = card.getByTestId("subscription-workflow-stage-grid");
		if ((await stageGrid.count()) > 0) {
			const stageGridStyles = await stageGrid.evaluate((element) => {
				const style = getComputedStyle(element);
				return {
					borderLeftWidth: style.borderLeftWidth,
					borderRightWidth: style.borderRightWidth,
					borderTopWidth: style.borderTopWidth,
					borderRadius: style.borderRadius,
				};
			});
			expect(stageGridStyles.borderLeftWidth).toBe("0px");
			expect(stageGridStyles.borderRightWidth).toBe("0px");
			expect(stageGridStyles.borderTopWidth).toBe("0px");
			expect(stageGridStyles.borderRadius).toBe("0px");
		}
		const issueSummaryBorderWidths = await card.evaluate((element) => {
			return Array.from(element.querySelectorAll("p"))
				.filter((node) => node.textContent?.includes("异常焦点"))
				.map((node) => {
					const style = getComputedStyle(node);
					return {
						borderLeftWidth: style.borderLeftWidth,
						borderTopWidth: style.borderTopWidth,
					};
				});
		});
		expect(issueSummaryBorderWidths.length).toBeGreaterThan(0);
		expect(
			issueSummaryBorderWidths.every(
				(widths) =>
					widths.borderLeftWidth === "0px" && widths.borderTopWidth === "0px",
			),
		).toBe(true);
	}
});

test("admin jobs tabs are URL-driven and support deep links plus history", async ({
	page,
}) => {
	await installAdminJobsMocks(page);

	await page.goto("/admin/jobs/scheduled", { waitUntil: "domcontentloaded" });
	await expect(page).toHaveURL(/\/admin\/jobs\/scheduled$/);
	await expect(page.getByRole("tab", { name: "定时任务" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(page.getByRole("heading", { name: "定时任务" })).toBeVisible();

	await page.goto("/admin/jobs/translations", {
		waitUntil: "domcontentloaded",
	});
	await expect(page).toHaveURL(/\/admin\/jobs\/translations\?view=queue$/);
	await expect(page.getByRole("heading", { name: "需求队列" })).toBeVisible();
	await page.getByRole("tab", { name: "任务记录" }).click();
	await expect(page).toHaveURL(/\/admin\/jobs\/translations\?view=history$/);
	await expect(page.getByRole("heading", { name: "任务记录" })).toBeVisible();
	await page.goBack({ waitUntil: "commit" });
	await expect(page).toHaveURL(/\/admin\/jobs\/translations\?view=queue$/);
	await expect(page.getByRole("heading", { name: "需求队列" })).toBeVisible();
	await page.goForward({ waitUntil: "commit" });
	await expect(page).toHaveURL(/\/admin\/jobs\/translations\?view=history$/);
	await expect(page.getByRole("heading", { name: "任务记录" })).toBeVisible();

	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });
	await expect(page).toHaveURL(/\/admin\/jobs\/llm$/);
	await expect(page.getByRole("tab", { name: "LLM调度" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await page.getByRole("tab", { name: "实时异步任务" }).click();
	await expect(page).toHaveURL(/\/admin\/jobs$/);
	await page.goBack();
	await expect(page).toHaveURL(/\/admin\/jobs\/llm$/);
	await expect(page.getByRole("heading", { name: "LLM 调度" })).toBeVisible();

	await page.goto("/admin/jobs/tasks/task-running-1", {
		waitUntil: "domcontentloaded",
	});
	await expect(page).toHaveURL(/\/admin\/jobs\/tasks\/task-running-1$/);
	await expect(page.getByRole("dialog", { name: "任务详情" })).toBeVisible();
	await page.getByRole("button", { name: "关闭", exact: true }).click();
	await expect(page).toHaveURL(/\/admin\/jobs$/);
});

test("content processing audit shows retry state, model, error, and call detail", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1440, height: 900 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	const listRequests: URL[] = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (url.pathname === "/api/admin/jobs/ai-records/release") {
			listRequests.push(url);
		}
	});

	const record = {
		id: "release-audit-1",
		kind: "release" as const,
		repository: "octo-demo/release-lab",
		title: "v2.31.0",
		occurred_at: "2026-04-15T03:20:00Z",
		detected_at: "2026-04-15T03:21:00Z",
		generated_at: null,
		translation: {
			status: "queued",
			retry_count: 1,
			started_at: "2026-04-15T03:23:00Z",
			last_attempt_at: "2026-04-15T03:25:00Z",
			finished_at: null,
		},
		polish: {
			status: "ready",
			retry_count: 0,
			started_at: "2026-04-15T03:22:00Z",
			last_attempt_at: "2026-04-15T03:22:01Z",
			finished_at: "2026-04-15T03:22:01Z",
		},
	};
	const llmCall = {
		id: "llm-call-1",
		status: "failed",
		source: "job.api.translate_release",
		model: "gpt-4o-mini",
	};
	const detail = {
		record,
		attempts: [
			{
				id: "work-translation-1:1",
				pipeline: "translation",
				attempt_no: 1,
				trigger: "automatic_recovery",
				status: "retry_scheduled",
				started_at: "2026-04-15T03:23:00Z",
				last_attempt_at: "2026-04-15T03:23:11Z",
				finished_at: "2026-04-15T03:23:11Z",
				error_code: "markdown_structure_mismatch",
				error_summary: "Markdown 结构校验失败",
				failure_class: "transient",
				retry_eligible: true,
				next_retry_at: "2026-04-15T03:25:00Z",
				llm_calls: [llmCall],
			},
			{
				id: "work-translation-1:2",
				pipeline: "translation",
				attempt_no: 2,
				trigger: "automatic_recovery",
				status: "queued",
				started_at: null,
				last_attempt_at: "2026-04-15T03:25:00Z",
				finished_at: null,
				error_code: null,
				error_summary: null,
				failure_class: null,
				retry_eligible: false,
				next_retry_at: null,
				llm_calls: [],
			},
		],
	};
	await page.route("**/api/admin/jobs/ai-records/**", async (route) => {
		const pathname = new URL(route.request().url()).pathname;
		if (pathname === "/api/admin/jobs/ai-records/release") {
			return json(route, { items: [record], page: 1, page_size: 20, total: 1 });
		}
		if (pathname === "/api/admin/jobs/ai-records/release/release-audit-1") {
			return json(route, detail);
		}
		return route.fallback();
	});

	await page.goto("/admin/jobs/ai-records", { waitUntil: "domcontentloaded" });
	await expect(page.getByRole("tab", { name: "内容处理" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	const recordsTable = page.getByRole("table").filter({ hasText: "v2.31.0" });
	await expect(
		recordsTable.getByText("v2.31.0", { exact: true }),
	).toBeVisible();
	await page.getByRole("button", { name: "翻译筛选" }).click();
	await page.getByRole("checkbox", { name: "排队中" }).check();
	await expect
		.poll(() => listRequests.at(-1)?.searchParams.get("translation_status"))
		.toBe("queued");
	await page.getByRole("checkbox", { name: "排队中" }).uncheck();
	const recordTypeTabs = page
		.getByRole("tabpanel", { name: "内容处理" })
		.getByRole("tablist");
	const recordTypeTabDimensions = await recordTypeTabs.evaluate((element) => ({
		width: element.getBoundingClientRect().width,
		parentWidth: element.parentElement?.getBoundingClientRect().width ?? 0,
	}));
	expect(recordTypeTabDimensions.width).toBeLessThan(
		recordTypeTabDimensions.parentWidth / 2,
	);
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth <= window.innerWidth,
		),
	).toBe(true);
	const minAttempts = page.getByRole("slider", { name: "最小总尝试次数" });
	const maxAttempts = page.getByRole("slider", { name: "最大总尝试次数" });
	await minAttempts.press("ArrowRight");
	await minAttempts.press("ArrowRight");
	await maxAttempts.press("Home");
	await maxAttempts.press("ArrowRight");
	await maxAttempts.press("ArrowRight");
	await maxAttempts.press("ArrowRight");
	await expect
		.poll(() => listRequests.at(-1)?.searchParams.get("attempt_min"))
		.toBe("2");
	await expect
		.poll(() => listRequests.at(-1)?.searchParams.get("attempt_max"))
		.toBe("5");
	await expect
		.poll(() => listRequests.at(-1)?.searchParams.get("page"))
		.toBe("1");
	await page.getByRole("button", { name: "重置尝试次数筛选" }).click();
	await expect
		.poll(() => listRequests.at(-1)?.searchParams.get("attempt_min") ?? null)
		.toBeNull();

	await page.getByRole("button", { name: "查看 v2.31.0 详情" }).click();
	const recordSheet = page.getByRole("dialog", { name: "记录详情" });
	await expect(recordSheet).toBeVisible();
	await expect(
		recordSheet.getByText("gpt-4o-mini", { exact: true }),
	).toBeVisible();
	await expect(recordSheet.getByText("Markdown 结构校验失败")).toBeVisible();
	await expect(
		recordSheet
			.getByRole("button", { name: "查看翻译第 2 次尝试详情" })
			.getByText("排队中", { exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "查看翻译第 1 次尝试详情" }).click();
	await expect(page.getByRole("dialog", { name: "尝试详情" })).toBeVisible();
	const llmButton = page
		.getByText("gpt-4o-mini", { exact: true })
		.locator("xpath=ancestor::button[1]");
	await llmButton.click();
	await expect(
		page.getByRole("dialog", { name: "模型调用详情" }),
	).toBeVisible();
	await expect(page.getByText("mock llm failed")).toBeVisible();
	await expect(page).toHaveURL(
		/\/admin\/jobs\/ai-records\/release\/release-audit-1\?ai_attempt=work-translation-1%3A1&ai_llm=llm-call-1$/,
	);
});

test("content processing attempt filter stays full-width without mobile overflow", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	const record = {
		id: "release-mobile-1",
		kind: "release" as const,
		repository: "octo-demo/release-lab",
		title: "移动端来源记录",
		occurred_at: "2026-04-15T03:20:00Z",
		detected_at: null,
		generated_at: null,
		translation: null,
		polish: {
			status: "not_recorded",
			retry_count: 0,
			started_at: null,
			last_attempt_at: null,
			finished_at: null,
		},
	};
	await page.route("**/api/admin/jobs/ai-records/**", async (route) => {
		const pathname = new URL(route.request().url()).pathname;
		if (pathname === "/api/admin/jobs/ai-records/release") {
			return json(route, { items: [record], page: 1, page_size: 20, total: 1 });
		}
		return route.fallback();
	});

	await page.goto("/admin/jobs/ai-records", { waitUntil: "domcontentloaded" });
	await expect(
		page.getByRole("button", { name: /移动端来源记录/ }),
	).toBeVisible();
	const filter = page.getByRole("group", { name: "尝试次数筛选" });
	const filterBox = await filter.boundingBox();
	expect(filterBox).not.toBeNull();
	expect(filterBox?.width ?? 0).toBeLessThanOrEqual(358);
	expect(
		await page.evaluate(
			() => document.documentElement.scrollWidth <= window.innerWidth,
		),
	).toBe(true);
});

test("admin requests grouped LLM call ordering and renders the response", async ({
	page,
}) => {
	const callRequests: URL[] = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (url.pathname === "/api/admin/jobs/llm/calls") callRequests.push(url);
	});
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByRole("heading", { name: "LLM 调度" })).toBeVisible();
	await expect(page.getByText("调度器状态")).toHaveCount(0);
	await expect(page.getByText("等待 / 进行中")).toHaveCount(0);

	const llmCallIds = await page
		.locator("p")
		.filter({ hasText: /^ID: llm-call-/ })
		.allTextContents();
	expect(llmCallIds).toEqual([
		"ID: llm-call-2",
		"ID: llm-call-3",
		"ID: llm-call-1",
	]);
	expect(
		callRequests.some(
			(request) =>
				request.searchParams.get("sort") === "status_grouped" &&
				request.searchParams.get("page") === "1" &&
				request.searchParams.get("page_size") === "20",
		),
	).toBe(true);
});

test("admin keeps rapid LLM filter edits in the shareable URL", async ({
	page,
}) => {
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	await page
		.getByRole("textbox", { name: "LLM 调用来源筛选" })
		.fill("job.api.translate_release");
	await page
		.getByRole("textbox", { name: "LLM 调用模型筛选" })
		.pressSequentially("gpt-4o-mini");

	await expect
		.poll(() => {
			const search = new URL(page.url()).searchParams;
			return {
				model: search.get("llm_model"),
				source: search.get("llm_source"),
			};
		})
		.toEqual({
			model: "gpt-4o-mini",
			source: "job.api.translate_release",
		});
});

test("admin llm activity defaults to chart and keeps list filters independent", async ({
	page,
}) => {
	const activityRequests: URL[] = [];
	page.on("request", (request) => {
		const url = new URL(request.url());
		if (url.pathname === "/api/admin/jobs/llm/activity") {
			activityRequests.push(url);
		}
	});
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	const grid = page.getByTestId("llm-activity-grid");
	await expect(grid).toBeVisible({ timeout: 10_000 });
	const filterHeights = await Promise.all(
		[
			page.getByRole("combobox", { name: "LLM 调用状态筛选" }),
			page.getByRole("textbox", { name: "LLM 调用模型筛选" }),
			page.getByRole("button", { name: "LLM 开始时间范围" }),
			page.getByRole("button", { name: "LLM 结束时间范围" }),
		].map((filter) =>
			filter.evaluate((element) =>
				Math.round(element.getBoundingClientRect().height),
			),
		),
	);
	expect(filterHeights).toEqual([36, 36, 36, 36]);
	const activityViewButton = page.getByRole("button", {
		name: "显示模型活动图",
	});
	const cardsViewButton = page.getByRole("button", {
		name: "显示模型状态卡片",
	});
	await expect(activityViewButton).toHaveAttribute("aria-pressed", "true");
	await expect(activityViewButton).toHaveClass(/bg-primary/);
	await expect(cardsViewButton).toHaveAttribute("aria-pressed", "false");
	await expect(cardsViewButton).not.toHaveClass(/bg-primary/);
	const latestCell = grid.getByRole("button", {
		name: /gpt-4o-mini，成功 8，失败 2/,
	});
	await latestCell.click();
	const summary = page.getByTestId("llm-activity-summary");
	await expect(
		page.getByRole("dialog", { name: /gpt-4o-mini.*调用摘要/ }),
	).toBeVisible();
	await expect(summary).toContainText("80%");
	await expect(summary).toContainText("67%");
	await latestCell.press("ArrowLeft");
	await expect(summary).toBeVisible();
	await page.keyboard.press("Escape");
	await expect(summary).toHaveCount(0);

	await page.getByRole("combobox", { name: "LLM 调用状态筛选" }).click();
	await page.getByRole("option", { name: "状态：失败" }).click();
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	expect(activityRequests.length).toBeGreaterThan(0);
	expect(activityRequests.every((request) => request.search === "")).toBe(true);

	await cardsViewButton.click();
	await expect(grid).toHaveCount(0);
	await expect(cardsViewButton).toHaveAttribute("aria-pressed", "true");
	await expect(cardsViewButton).toHaveClass(/bg-primary/);
	await expect(activityViewButton).not.toHaveClass(/bg-primary/);
	await expect(page.getByText("冷却中")).toBeVisible();
	await page.reload({ waitUntil: "domcontentloaded" });
	await expect(page.getByTestId("llm-activity-grid")).toBeVisible();
});

test("admin drills from LLM activity and model cards into shareable call filters", async ({
	page,
}) => {
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto(
		"/admin/jobs/llm?llm_status=succeeded&llm_source=obsolete-source&llm_requested_by=obsolete-user&llm_time_field=started&llm_time_from=2026-02-26T00%3A00%3A00Z",
		{ waitUntil: "domcontentloaded" },
	);

	const grid = page.getByTestId("llm-activity-grid");
	const latestCell = grid.getByRole("button", {
		name: /gpt-4o-mini，成功 8，失败 2/,
	});
	await expect(latestCell).toBeVisible({ timeout: 10_000 });
	await latestCell.focus();
	await page.keyboard.press("ContextMenu");
	await expect(
		page.getByRole("menuitem", { name: "查看失败调用" }),
	).toBeVisible();
	await page.keyboard.press("Escape");

	await latestCell.click({ button: "right" });
	await page.getByRole("menuitem", { name: "查看失败调用" }).click();
	await expect
		.poll(() => {
			const search = new URL(page.url()).searchParams;
			return {
				status: search.get("llm_status"),
				model: search.get("llm_model"),
				source: search.get("llm_source"),
				requestedBy: search.get("llm_requested_by"),
				startedFrom: search.get("llm_started_from"),
				startedTo: search.get("llm_started_to"),
				finishedFrom: search.get("llm_finished_from"),
				finishedBefore: search.get("llm_finished_before"),
				legacyField: search.get("llm_time_field"),
			};
		})
		.toEqual({
			status: "failed",
			model: "gpt-4o-mini",
			source: null,
			requestedBy: null,
			startedFrom: null,
			startedTo: null,
			finishedFrom: "2026-02-26T02:00:00.000Z",
			finishedBefore: "2026-02-26T03:00:00.000Z",
			legacyField: null,
		});
	const results = page.getByRole("region", { name: "LLM 调用记录结果" });
	await expect(results).toBeFocused();
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await latestCell.focus();
	await latestCell.click({ button: "right" });
	await page.getByRole("menuitem", { name: "查看失败调用" }).click();
	await expect(results).toBeFocused();
	await page.getByRole("button", { name: "LLM 结束时间范围" }).click();
	await expect(
		page.getByRole("group", { name: "LLM 结束时间后日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("group", { name: "LLM 结束时间前（不含）日历" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 结束时间后" }),
	).toHaveValue(
		await page.evaluate(() => {
			const value = new Date("2026-02-26T02:00:00.000Z");
			const pad = (part: number) => String(part).padStart(2, "0");
			return `${value.getFullYear()}/${pad(value.getMonth() + 1)}/${pad(value.getDate())} ${pad(value.getHours())}:${pad(value.getMinutes())}`;
		}),
	);
	await expect(
		page.getByRole("textbox", { name: "LLM 结束时间前（不含）" }),
	).toHaveValue(
		await page.evaluate(() => {
			const value = new Date("2026-02-26T03:00:00.000Z");
			const pad = (part: number) => String(part).padStart(2, "0");
			return `${value.getFullYear()}/${pad(value.getMonth() + 1)}/${pad(value.getDate())} ${pad(value.getHours())}:${pad(value.getMinutes())}`;
		}),
	);
	await page.keyboard.press("Escape");
	await page.getByRole("button", { name: "LLM 开始时间范围" }).click();
	await page
		.getByRole("textbox", { name: "LLM 开始时间后" })
		.fill("2026/02/26 01:00");
	await expect
		.poll(() => {
			const search = new URL(page.url()).searchParams;
			return {
				startedFrom: search.get("llm_started_from"),
				finishedFrom: search.get("llm_finished_from"),
				finishedBefore: search.get("llm_finished_before"),
			};
		})
		.toEqual({
			startedFrom: new Date("2026-02-26T01:00").toISOString(),
			finishedFrom: "2026-02-26T02:00:00.000Z",
			finishedBefore: "2026-02-26T03:00:00.000Z",
		});
	await page.keyboard.press("Escape");

	await page.goBack();
	await expect(page).toHaveURL(/llm_source=obsolete-source/);
	const restoredCell = page
		.getByTestId("llm-activity-grid")
		.getByRole("button", { name: /gpt-4o-mini，成功 8，失败 2/ });
	await restoredCell.click();
	await page.getByRole("button", { name: /gpt-4o-mini 的调用操作/ }).click();
	await page.getByRole("menuitem", { name: "查看全部调用" }).click();
	await expect
		.poll(() => {
			const search = new URL(page.url()).searchParams;
			return [
				search.get("llm_status"),
				search.get("llm_model"),
				search.get("llm_finished_from"),
				search.get("llm_finished_before"),
			];
		})
		.toEqual([
			null,
			"gpt-4o-mini",
			"2026-02-26T02:00:00.000Z",
			"2026-02-26T03:00:00.000Z",
		]);

	await page.getByRole("button", { name: "显示模型状态卡片" }).click();
	const modelStatusActions = page.getByRole("button", {
		name: /gpt-4o-mini 的调用操作/,
	});
	await modelStatusActions.focus();
	await page.keyboard.press("Shift+F10");
	await expect(
		page.getByRole("menuitem", { name: "查看失败调用" }),
	).toBeVisible();
	await page.keyboard.press("Escape");
	await modelStatusActions.dispatchEvent("pointerdown", {
		button: 0,
		isPrimary: true,
		pointerId: 1,
		pointerType: "touch",
	});
	await expect(page.getByRole("menu")).toHaveCount(1);
	await page.waitForTimeout(800);
	await expect(page.getByRole("menu")).toHaveCount(1);
	await page
		.locator('button[aria-label="gpt-4o-mini 的调用操作"]')
		.dispatchEvent("pointerup", {
			button: 0,
			isPrimary: true,
			pointerId: 1,
			pointerType: "touch",
		});
	await page.keyboard.press("Escape");
	await modelStatusActions.click();
	await page.getByRole("menuitem", { name: "查看全部调用" }).click();
	await expect
		.poll(() => {
			const search = new URL(page.url()).searchParams;
			return {
				model: search.get("llm_model"),
				startedFrom: search.get("llm_started_from"),
				startedTo: search.get("llm_started_to"),
				finishedFrom: search.get("llm_finished_from"),
				finishedBefore: search.get("llm_finished_before"),
			};
		})
		.toEqual({
			model: "gpt-4o-mini",
			startedFrom: null,
			startedTo: null,
			finishedFrom: null,
			finishedBefore: null,
		});
});

test("admin llm activity keeps context-menu hit targets contiguous", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1200, height: 900 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	const grid = page.getByTestId("llm-activity-grid");
	const firstModelCells = grid.getByRole("button", { name: /gpt-4o-mini/ });
	const secondModelCells = grid.getByRole("button", { name: /gpt-4\.1-mini/ });
	await expect(firstModelCells.first()).toBeVisible({ timeout: 10_000 });
	const [firstCellBox, secondCellBox, nextRowCellBox] = await Promise.all([
		firstModelCells.nth(0).boundingBox(),
		firstModelCells.nth(1).boundingBox(),
		secondModelCells.nth(0).boundingBox(),
	]);
	expect(firstCellBox).not.toBeNull();
	expect(secondCellBox).not.toBeNull();
	expect(nextRowCellBox).not.toBeNull();
	expect(
		Math.abs(
			(firstCellBox?.x ?? 0) +
				(firstCellBox?.width ?? 0) -
				(secondCellBox?.x ?? 0),
		),
	).toBeLessThan(0.5);
	expect(
		Math.abs(
			(firstCellBox?.y ?? 0) +
				(firstCellBox?.height ?? 0) -
				(nextRowCellBox?.y ?? 0),
		),
	).toBeLessThan(0.5);

	await page.mouse.click(
		secondCellBox?.x ?? 0,
		(firstCellBox?.y ?? 0) + (firstCellBox?.height ?? 0) / 2,
		{ button: "right" },
	);
	await expect(
		page.getByRole("menuitem", { name: "查看失败调用" }),
	).toBeVisible();
});

test("admin llm activity keeps the pointer tooltip clear of the grid", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1200, height: 900 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	const grid = page.getByTestId("llm-activity-grid");
	const cells = grid.getByRole("button", { name: /gpt-4o-mini/ });
	await expect(cells.first()).toBeVisible({ timeout: 10_000 });
	await cells.nth(20).hover();

	const summary = page.getByTestId("llm-activity-summary");
	await expect(summary).toBeVisible();
	const [summaryBox, cellBoxes] = await Promise.all([
		summary.boundingBox(),
		cells.evaluateAll((nodes) =>
			nodes
				.filter((node) => {
					const style = window.getComputedStyle(node);
					return style.display !== "none" && style.visibility !== "hidden";
				})
				.map((node) => {
					const box = node.getBoundingClientRect();
					return {
						left: box.left,
						right: box.right,
						top: box.top,
						bottom: box.bottom,
					};
				}),
		),
	]);
	expect(summaryBox).not.toBeNull();
	expect(
		cellBoxes.some(
			(cell) =>
				(summaryBox?.x ?? 0) < cell.right &&
				(summaryBox?.x ?? 0) + (summaryBox?.width ?? 0) > cell.left &&
				(summaryBox?.y ?? 0) < cell.bottom &&
				(summaryBox?.y ?? 0) + (summaryBox?.height ?? 0) > cell.top,
		),
	).toBe(false);

	await page.mouse.move((summaryBox?.x ?? 0) + 12, (summaryBox?.y ?? 0) + 12);
	await expect(summary).toBeVisible();
});

test("admin llm activity reanchors a pinned tooltip after activity refresh", async ({
	page,
}) => {
	await page.setViewportSize({ width: 1200, height: 900 });
	await installAdminJobsMocks(page, {
		emitStreamEvents: false,
		activityRefreshOnSecondRequest: true,
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/activity",
				afterCount: 1,
				times: 1,
				delayMs: 500,
			},
		],
	});
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	const grid = page.getByTestId("llm-activity-grid");
	const cells = grid.locator('button[aria-label*="gpt-4o-mini"]');
	await expect(cells.first()).toBeVisible({ timeout: 10_000 });
	await expect(cells).toHaveCount(50, { timeout: 10_000 });
	const pinnedCell = cells.nth(20);
	const pinnedLabel = await pinnedCell.getAttribute("aria-label");
	expect(pinnedLabel).not.toBeNull();
	await pinnedCell.click();

	const summary = page.getByTestId("llm-activity-summary");
	await expect(summary).toBeVisible();
	const activeCellSelector =
		'button[aria-expanded="true"][aria-label*="gpt-4o-mini"]';
	const beforeActiveCell = await grid.locator(activeCellSelector).boundingBox();
	const beforeTooltip = await summary.boundingBox();
	await page
		.getByRole("button", { name: "刷新" })
		.first()
		.dispatchEvent("click");
	await expect(summary).toBeVisible();
	await expect(
		grid.getByRole("button", { name: pinnedLabel ?? "" }),
	).toBeVisible();
	await page.waitForTimeout(700);
	const afterActiveCell = await grid.locator(activeCellSelector).boundingBox();
	const afterTooltip = await summary.boundingBox();
	expect(beforeActiveCell).not.toBeNull();
	expect(afterActiveCell).not.toBeNull();
	expect(beforeTooltip).not.toBeNull();
	expect(afterTooltip).not.toBeNull();
	expect(
		Math.abs((afterActiveCell?.x ?? 0) - (beforeActiveCell?.x ?? 0)),
	).toBeGreaterThan(0);
	expect(
		Math.abs((afterTooltip?.x ?? 0) - (beforeTooltip?.x ?? 0)),
	).toBeGreaterThan(0);
	const activeCellShift =
		(afterActiveCell?.x ?? 0) +
		(afterActiveCell?.width ?? 0) / 2 -
		((beforeActiveCell?.x ?? 0) + (beforeActiveCell?.width ?? 0) / 2);
	const tooltipShift =
		(afterTooltip?.x ?? 0) +
		(afterTooltip?.width ?? 0) / 2 -
		((beforeTooltip?.x ?? 0) + (beforeTooltip?.width ?? 0) / 2);
	expect(Math.abs(tooltipShift - activeCellShift)).toBeLessThan(1);
	const refreshedCellBoxes = await cells.evaluateAll((nodes) =>
		nodes
			.filter((node) => {
				const style = window.getComputedStyle(node);
				return style.display !== "none" && style.visibility !== "hidden";
			})
			.map((node) => {
				const box = node.getBoundingClientRect();
				return {
					left: box.left,
					right: box.right,
					top: box.top,
					bottom: box.bottom,
				};
			}),
	);
	expect(
		refreshedCellBoxes.some(
			(cell) =>
				(afterTooltip?.x ?? 0) < cell.right &&
				(afterTooltip?.x ?? 0) + (afterTooltip?.width ?? 0) > cell.left &&
				(afterTooltip?.y ?? 0) < cell.bottom &&
				(afterTooltip?.y ?? 0) + (afterTooltip?.height ?? 0) > cell.top,
		),
	).toBe(false);
});

test("admin llm activity handles tooltip edges across rows and viewports", async ({
	page,
}) => {
	for (const viewport of [
		{ width: 1200, height: 500 },
		{ width: 393, height: 852 },
	]) {
		await page.setViewportSize(viewport);
		await installAdminJobsMocks(page, { emitStreamEvents: false });
		await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

		const grid = page.getByTestId("llm-activity-grid");
		await expect(grid).toBeVisible({ timeout: 10_000 });
		await grid.scrollIntoViewIfNeeded();
		const visibleCells = grid.locator(
			'button[aria-controls="llm-activity-summary"]:visible',
		);
		await expect(visibleCells.first()).toBeVisible();
		await visibleCells.first().hover();

		const summary = page.getByTestId("llm-activity-summary");
		await expect(summary).toBeVisible();
		const [summaryBox, viewportOverflow] = await Promise.all([
			summary.boundingBox(),
			page.evaluate(() => ({
				documentOverflow:
					document.documentElement.scrollWidth -
					document.documentElement.clientWidth,
				viewportWidth: window.innerWidth,
				viewportHeight: window.innerHeight,
			})),
		]);
		expect(summaryBox).not.toBeNull();
		expect(summaryBox?.x ?? -1).toBeGreaterThanOrEqual(0);
		expect(summaryBox?.y ?? -1).toBeGreaterThanOrEqual(0);
		expect((summaryBox?.x ?? 0) + (summaryBox?.width ?? 0)).toBeLessThanOrEqual(
			viewportOverflow.viewportWidth,
		);
		expect(
			(summaryBox?.y ?? 0) + (summaryBox?.height ?? 0),
		).toBeLessThanOrEqual(viewportOverflow.viewportHeight);
		expect(viewportOverflow.documentOverflow).toBeLessThanOrEqual(1);

		const cellBoxes = await visibleCells.evaluateAll((nodes) =>
			nodes.map((node) => {
				const rect = node.getBoundingClientRect();
				return {
					left: rect.left,
					right: rect.right,
					top: rect.top,
					bottom: rect.bottom,
				};
			}),
		);
		expect(
			cellBoxes.some(
				(cell) =>
					(summaryBox?.x ?? 0) < cell.right &&
					(summaryBox?.x ?? 0) + (summaryBox?.width ?? 0) > cell.left &&
					(summaryBox?.y ?? 0) < cell.bottom &&
					(summaryBox?.y ?? 0) + (summaryBox?.height ?? 0) > cell.top,
			),
		).toBe(false);

		await visibleCells.last().click();
		await page.mouse.wheel(0, 120);
		await expect(summary).toBeVisible();
		await page.keyboard.press("Escape");
		await expect(summary).toHaveCount(0);
	}
});

test("admin llm activity fits a dynamic recent window without horizontal overflow", async ({
	page,
}) => {
	const visibleBucketCounts: number[] = [];
	for (const width of [1200, 960, 640, 393, 320]) {
		await page.setViewportSize({ width, height: 852 });
		await installAdminJobsMocks(page, { emitStreamEvents: false });
		await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

		const grid = page.getByTestId("llm-activity-grid");
		const cells = grid.getByRole("button", { name: /gpt-4o-mini/ });
		await expect(cells.first()).toBeVisible({ timeout: 10_000 });
		const [visibleCells, layout] = await Promise.all([
			cells.evaluateAll(
				(nodes) =>
					nodes.filter((node) => {
						const style = window.getComputedStyle(node);
						return (
							style.display !== "none" &&
							style.visibility !== "hidden" &&
							node.getBoundingClientRect().width > 0
						);
					}).length,
			),
			page.evaluate(() => ({
				documentOverflow:
					document.documentElement.scrollWidth -
					document.documentElement.clientWidth,
			})),
		]);
		visibleBucketCounts.push(visibleCells);
		expect(visibleCells).toBeGreaterThan(0);
		expect(visibleCells).toBeLessThanOrEqual(50);
		expect(layout.documentOverflow).toBeLessThanOrEqual(1);
		expect(
			await grid.evaluate((node) => node.scrollWidth - node.clientWidth),
		).toBeLessThanOrEqual(1);
		expect(
			await grid.evaluate((node) => {
				const surface = node.querySelector<HTMLElement>(
					'[data-testid="llm-activity-surface"]',
				);
				const rowCells = Array.from(
					node.querySelectorAll<HTMLButtonElement>(
						'button[aria-label*="gpt-4o-mini"]',
					),
				);
				const lastCell = rowCells.at(-1);
				if (!surface || !lastCell) return Number.POSITIVE_INFINITY;
				return Math.abs(
					surface.getBoundingClientRect().right -
						lastCell.getBoundingClientRect().right,
				);
			}),
		).toBeLessThanOrEqual(1);
		if (width >= 640) {
			const timeLabels = grid.getByTestId("llm-activity-time-label");
			const surfaceBox = await grid
				.getByTestId("llm-activity-surface")
				.boundingBox();
			const [firstTimeLabel, lastTimeLabel] = await Promise.all([
				timeLabels.first().boundingBox(),
				timeLabels.last().boundingBox(),
			]);
			const timeLabelBoxes = await timeLabels.evaluateAll((nodes) =>
				nodes
					.map((node) => node.getBoundingClientRect())
					.filter((rect) => rect.width > 0)
					.map((rect) => ({ left: rect.left, right: rect.right }))
					.sort((left, right) => left.left - right.left),
			);
			expect(await timeLabels.count()).toBeGreaterThan(1);
			expect(surfaceBox).not.toBeNull();
			for (const labelBox of [firstTimeLabel, lastTimeLabel]) {
				expect(labelBox).not.toBeNull();
				expect(labelBox?.x ?? 0).toBeGreaterThanOrEqual(
					(surfaceBox?.x ?? 0) - 1,
				);
				expect((labelBox?.x ?? 0) + (labelBox?.width ?? 0)).toBeLessThanOrEqual(
					(surfaceBox?.x ?? 0) + (surfaceBox?.width ?? 0) + 1,
				);
			}
			for (let index = 1; index < timeLabelBoxes.length; index += 1) {
				expect(timeLabelBoxes[index - 1].right).toBeLessThanOrEqual(
					timeLabelBoxes[index].left + 1,
				);
			}
		}

		if (width < 640) {
			await expect(grid.getByRole("list", { name: "模型图例" })).toBeVisible();
		}
	}

	expect(visibleBucketCounts[0]).toBeGreaterThan(visibleBucketCounts[2]);
	expect(visibleBucketCounts[1]).toBeGreaterThan(visibleBucketCounts[2]);
	expect(visibleBucketCounts[3]).toBeGreaterThan(visibleBucketCounts[4]);
});

test("admin llm activity prioritizes the grid on mobile", async ({ page }) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	const grid = page.getByTestId("llm-activity-grid");
	await expect(grid).toBeVisible({ timeout: 10_000 });
	await expect(grid.getByText(/最近 \d+ 小时/, { exact: false })).toBeVisible();
	await expect(grid.getByTestId("llm-activity-mobile-range")).toContainText(
		"至",
	);
	const modelLabel = grid.locator('span[title="gpt-4o-mini"]');
	await expect(modelLabel).toBeHidden();
	const mobileModelCells = grid.getByRole("button", { name: /gpt-4o-mini/ });
	expect(await mobileModelCells.count()).toBeGreaterThan(0);
	expect(await mobileModelCells.count()).toBeLessThan(50);
	const [firstCellBox, secondCellBox] = await Promise.all([
		mobileModelCells.nth(0).boundingBox(),
		mobileModelCells.nth(1).boundingBox(),
	]);
	expect(firstCellBox).not.toBeNull();
	expect(secondCellBox).not.toBeNull();
	expect(
		Math.abs(
			(firstCellBox?.x ?? 0) +
				(firstCellBox?.width ?? 0) -
				(secondCellBox?.x ?? 0),
		),
	).toBeLessThan(0.5);

	await expect(grid.getByRole("list", { name: "模型图例" })).toContainText(
		"gpt-4o-mini",
	);
});

test("admin llm mobile filters keep context around the time range panel", async ({
	page,
}) => {
	await page.setViewportSize({ width: 393, height: 852 });
	await installAdminJobsMocks(page, { emitStreamEvents: false });
	await page.goto("/admin/jobs/llm", { waitUntil: "domcontentloaded" });

	await expect(
		page.getByRole("button", { name: "打开 LLM 调用筛选" }),
	).toBeVisible();
	await expect(
		page.getByRole("textbox", { name: "LLM 调用模型筛选" }),
	).toHaveCount(0);
	await page.getByRole("button", { name: "打开 LLM 调用筛选" }).click();
	const filters = page.getByRole("dialog", { name: "筛选调用记录" });
	await expect(filters).toBeVisible();
	await expect(filters).toHaveClass(/right-0/);
	await expect(
		filters.getByRole("textbox", { name: "LLM 调用模型筛选" }),
	).toBeVisible();
	await filters.getByRole("button", { name: "LLM 开始时间范围" }).click();
	const drawer = filters.getByRole("region", {
		name: "LLM 开始时间范围设置",
	});
	await expect(drawer).toBeVisible();
	await expect(filters).toBeVisible();
	await expect(page.getByRole("dialog")).toHaveCount(1);
	await expect(drawer).toHaveClass(/bottom-0/);
	await expect(
		drawer.getByRole("group", { name: "LLM 开始时间后日历" }),
	).toBeVisible();
	await expect(
		drawer.getByRole("group", { name: "LLM 开始时间前日历" }),
	).toHaveCount(0);
	await drawer.getByRole("textbox", { name: "LLM 开始时间前" }).click();
	await expect(
		drawer.getByRole("group", { name: "LLM 开始时间后日历" }),
	).toHaveCount(0);
	await expect(
		drawer.getByRole("group", { name: "LLM 开始时间前日历" }),
	).toBeVisible();
	const scrollMetrics = await drawer
		.getByTestId("llm-time-range-drawer-scroll")
		.evaluate((element) => ({
			clientHeight: element.clientHeight,
			scrollHeight: element.scrollHeight,
		}));
	expect(scrollMetrics.scrollHeight).toBeLessThanOrEqual(
		scrollMetrics.clientHeight + 1,
	);
	await page.keyboard.press("Escape");
	await expect(drawer).toHaveCount(0);
	await expect(filters).toBeVisible();
	await expect(
		filters.getByRole("button", { name: "LLM 开始时间范围" }),
	).toBeFocused();
	await expect(
		filters.getByRole("textbox", { name: "LLM 调用模型筛选" }),
	).toBeVisible();
});

test("admin keeps llm calls visible during sse refresh", async ({ page }) => {
	test.slow();
	const delayedRefreshTimeoutMs = 12_000;
	await installAdminJobsMocks(page, {
		responseDelayMs: 4000,
		delayedPaths: ["/api/admin/jobs/llm/calls", "/api/admin/jobs/llm/activity"],
		emitStreamEvents: true,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible({
		timeout: delayedRefreshTimeoutMs,
	});
	await expect(page.getByText("LLM 调度更新中...")).toBeVisible({
		timeout: delayedRefreshTimeoutMs,
	});
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible({
		timeout: delayedRefreshTimeoutMs,
	});
	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);
	await expect(page.getByTestId("llm-activity-grid")).toBeVisible();
	await expect(page.getByText("更新中", { exact: true })).toBeVisible();
});

test("admin keeps newest llm filter results after overlapping refreshes", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 2,
				times: 2,
				delayMs: 1200,
			},
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=failed",
				times: 1,
				delayMs: 2200,
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();
	await page.getByRole("combobox", { name: "LLM 调用状态筛选" }).click();
	await page.getByRole("option", { name: "状态：失败" }).click();

	await expect(page.getByText("LLM 调度更新中...")).toBeVisible();
	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	const staleLlmCard = page
		.getByText("ID: llm-call-2")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await expect(
		staleLlmCard.getByRole("button", { name: "详情" }),
	).toBeDisabled();
	await expect(refreshButton).toBeDisabled();

	await page.waitForTimeout(2400);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);
	await expect(refreshButton).toBeEnabled();
});

test("admin keeps blocking loader before first realtime load completes", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/realtime",
				search: "status=all&task_group=realtime",
				times: 1,
				delayMs: 1200,
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs", { waitUntil: "domcontentloaded" });

	await expect(
		page.locator('[data-list-state="initial-loading"]').first(),
	).toBeVisible();
	await expect(page.getByText(/^SSE (已连接|重连中\.\.\.)$/)).toBeVisible();
	await expect(page.getByText("任务列表更新中...")).toHaveCount(0);
	await expect(page.getByText("暂无任务")).toHaveCount(0);
	await expect(page.getByText("sync.releases")).toBeVisible();
});

test("admin ignores stale llm refresh errors after filter change", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 1,
				times: 1,
				delayMs: 1200,
			},
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=failed",
				times: 1,
				delayMs: 750,
			},
		],
		failureRules: [
			{
				pathname: "/api/admin/jobs/llm/calls",
				search: "status=all",
				afterCount: 1,
				times: 1,
				message: "stale llm refresh failed",
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(
		page.getByRole("combobox", { name: "LLM 调用状态筛选" }),
	).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();
	await page.getByRole("combobox", { name: "LLM 调用状态筛选" }).click();
	const filteredLlmRequest = page.waitForRequest((request) => {
		const url = new URL(request.url());
		return (
			request.method() === "GET" &&
			url.pathname === "/api/admin/jobs/llm/calls" &&
			url.searchParams.get("status") === "failed"
		);
	});
	await page.getByRole("option", { name: "状态：失败" }).click();
	await filteredLlmRequest;
	await expect(refreshButton).toBeDisabled();

	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("stale llm refresh failed")).toHaveCount(0);

	await page.waitForTimeout(1300);
	await expect(page.getByText("job.api.translate_release")).toBeVisible();
	await expect(page.getByText("api.translate_releases_batch")).toHaveCount(0);
	await expect(refreshButton).toBeEnabled();
	await expect(page.getByText("stale llm refresh failed")).toHaveCount(0);
});

test("admin keeps realtime tasks visible while status filter refreshes", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		delayRules: [
			{
				pathname: "/api/admin/jobs/realtime",
				search: "status=running",
				times: 1,
				delayMs: 600,
			},
		],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("translate.release.batch")).toBeVisible();
	await page.getByRole("combobox", { name: "实时异步任务状态筛选" }).click();
	await page.getByRole("option", { name: "状态：运行中" }).click();

	await expect(page.getByText("正在加载任务...")).toHaveCount(0);
	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("translate.release.batch")).toBeVisible();
	const staleTaskCard = page
		.getByText("ID: task-translate-batch-1")
		.locator("xpath=ancestor::div[.//button[normalize-space()='详情']][1]");
	await expect(
		staleTaskCard.getByRole("button", { name: "详情" }),
	).toBeDisabled();

	await page.waitForTimeout(700);
	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("translate.release.batch")).toHaveCount(0);
});

test("admin refresh keeps scheduled runs visible", async ({ page }) => {
	test.slow();
	await installAdminJobsMocks(page, {
		responseDelayMs: 1200,
		delayedPaths: ["/api/admin/jobs/realtime"],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await page.getByRole("tab", { name: "定时任务" }).click();
	await expect(page.getByText("定时日报")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();

	await expect(refreshButton).toBeDisabled();
	await expect(page.getByText("定时日报")).toBeVisible();
	await expect(page.getByText("运行记录更新中...")).toBeVisible();
	await expect(page.getByText("正在加载运行记录...")).toHaveCount(0);
	await expect(refreshButton).toBeEnabled();
	await expect(page.getByText("运行记录更新中...")).toHaveCount(0);
});

test("admin refresh keeps existing jobs and llm calls visible", async ({
	page,
}) => {
	test.slow();
	await installAdminJobsMocks(page, {
		responseDelayMs: 1200,
		delayedPaths: ["/api/admin/jobs/realtime", "/api/admin/jobs/llm/calls"],
		emitStreamEvents: false,
	});
	await page.goto("/admin/jobs");

	await expect(page.getByText("sync.releases")).toBeVisible();
	const refreshButton = page.getByRole("button", { name: "刷新" });
	await refreshButton.click();

	await expect(refreshButton).toBeDisabled();
	await expect(page.getByText("sync.releases")).toBeVisible();
	await expect(page.getByText("正在加载任务...")).toHaveCount(0);

	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(page.getByText("api.translate_releases_batch")).toBeVisible();
	await expect(page.getByText("正在加载调用记录...")).toHaveCount(0);

	await expect(refreshButton).toBeEnabled();
	await expect(page.getByText("任务列表更新中...")).toHaveCount(0);
	await expect(page.getByText("LLM 调度更新中...")).toHaveCount(0);
});

test("admin can inspect translation scheduler", async ({ page }) => {
	await installAdminJobsMocks(page);

	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "翻译调度" }).click({ force: true });
	await expect(page.getByRole("heading", { name: "翻译调度" })).toBeVisible();
	await expect(page.getByText("工作者板")).toBeVisible();
	await expect(page.getByText("W4 · 用户专用")).toBeVisible();
	const auditInput = page.getByLabel("发布记录 ID");
	await auditInput.fill("290978079");
	await page.getByRole("button", { name: "查询发布记录重试审计" }).click();
	await expect(page.getByText("已安排重试").first()).toBeVisible();
	await expect(page.getByText("自动重试").first()).toBeVisible();
	await expect(page.getByText("Markdown 结构校验失败").first()).toBeVisible();
	await page.getByRole("button", { name: "打开 W4 · 用户专用 详情" }).click();
	await expect(page.getByRole("heading", { name: "工作者详情" })).toBeVisible();
	await expect(page.getByText("translation-worker-4")).toBeVisible();
	await page.getByRole("button", { name: "关闭" }).click();
	await expect(page.getByRole("heading", { name: "工作者详情" })).toHaveCount(
		0,
	);
	await expect(page.getByRole("tab", { name: "需求队列" })).toBeVisible();
	const translationRequestRow = page
		.getByText("feed.auto_translate:release:290978079")
		.locator("xpath=ancestor::tr[1]");
	await expect(translationRequestRow).toBeVisible();
	await translationRequestRow.getByRole("button", { name: "详情" }).click();
	await expect(
		page.getByRole("heading", { name: "翻译请求详情" }),
	).toBeVisible();
	const requestDialog = page.getByLabel("翻译请求详情");
	await expect(
		requestDialog.getByText("release_detail · feed_body", { exact: true }),
	).toBeVisible();
	await expect(
		requestDialog.getByText(
			"entity 290978079 · producer_ref feed.auto_translate:release:290978079",
		),
	).toBeVisible();
	await page.getByRole("button", { name: "查看批次" }).click();
	await expect(
		page.getByRole("heading", { name: "翻译批次详情" }),
	).toBeVisible();
	await expect(page.getByText("translation.scheduler.deadline")).toBeVisible();
	await page.getByRole("button", { name: "打开 LLM 详情" }).click();
	const llmDialog = page.getByRole("dialog", { name: "LLM 调用详情" });
	await expect(
		llmDialog.getByRole("heading", { name: "LLM 调用详情" }),
	).toBeVisible();
	await expect(llmDialog.getByText("llm-translation-1")).toBeVisible();
});

test("admin can update llm runtime settings from settings dialog", async ({
	page,
}) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "LLM调度" }).click();

	await page.getByRole("button", { name: "配置 LLM 运行参数" }).click();
	const dialog = page.getByRole("dialog", { name: "配置 LLM 运行参数" });
	await expect(dialog).toBeVisible();
	const concurrencyInput = dialog.getByLabel("最大并发数");
	const modelInput = dialog.getByLabel("LLM 输入长度上限（tokens）");
	const firstModelInput = dialog.getByPlaceholder("例如 gpt-4o-mini").first();
	await expect(concurrencyInput).toHaveValue("2");
	await expect(modelInput).toHaveValue("");
	await expect(firstModelInput).toHaveValue("gpt-4o-mini");
	await concurrencyInput.fill("0");
	await dialog.getByRole("button", { name: "保存设置" }).click();
	await expect(dialog.getByText("并发上限必须是大于 0 的整数。")).toBeVisible();
	await concurrencyInput.fill("5");
	await dialog.getByRole("button", { name: "新增模型" }).click();
	const modelInputs = dialog.getByPlaceholder("例如 gpt-4o-mini");
	await firstModelInput.fill("gpt-4.1-mini");
	await modelInputs.nth(1).fill("gpt-4o-mini");
	await dialog.getByRole("button", { name: "删除模型 3" }).click();
	await modelInput.fill("65536");
	await dialog.getByRole("button", { name: "保存设置" }).click();
	await expect(dialog).toHaveCount(0);
	await expect(
		page.getByText("并发上限 5 · 可用 4 · 输入 65,536 tokens"),
	).toBeVisible();
	await expect(page.getByText("当前优先模型：")).toBeVisible();
	await page.getByRole("button", { name: "显示模型状态卡片" }).click();
	await expect(page.getByText("1. gpt-4.1-mini")).toBeVisible();
});

test("admin refreshes llm scheduler via shared sse stream", async ({
	page,
}) => {
	await installAdminJobsMocks(page, {
		emitLlmSchedulerEvents: true,
		delayRules: [
			{
				pathname: "/api/admin/jobs/llm/status",
				afterCount: 1,
				times: 1,
				delayMs: 1200,
			},
		],
	});

	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "LLM调度" }).click();
	await expect(
		page.getByText("并发上限 5 · 可用 4 · 输入 1,047,576 tokens"),
	).toBeVisible();
});

test("admin can update translation worker counts from settings dialog", async ({
	page,
}) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "翻译调度" }).click({ force: true });

	const settingsButton = page.getByRole("button", {
		name: "配置翻译 worker 数量",
	});
	await expect(settingsButton).toBeVisible();
	await page.getByRole("tab", { name: "任务记录" }).click();
	await expect(settingsButton).toBeVisible();
	await settingsButton.click();

	const dialog = page.getByRole("dialog", { name: "配置翻译 worker 数量" });
	await expect(dialog).toBeVisible();
	const generalInput = dialog.getByLabel("通用 worker 数量");
	const dedicatedInput = dialog.getByLabel("用户专用 worker 数量");
	await expect(generalInput).toHaveValue("3");
	await expect(dedicatedInput).toHaveValue("1");
	await generalInput.fill("0");
	await dialog.getByRole("button", { name: "保存设置" }).click();
	await expect(
		dialog.getByText("通用 worker 数量必须是大于 0 的整数。"),
	).toBeVisible();
	await generalInput.fill("5");
	await dedicatedInput.fill("2");
	await dialog.getByRole("button", { name: "保存设置" }).click();
	await expect(dialog).toHaveCount(0);
	await expect(
		page.getByText(
			"目标配置为 5 个通用 worker 与 2 个用户专用 worker；下方展示实时槽位状态。",
		),
	).toBeVisible();
	await expect(page.getByText("W7 · 用户专用")).toBeVisible();
});

test("translation worker drawer closes when resize removes the selected worker", async ({
	page,
}) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "翻译调度" }).click({ force: true });

	await page.getByRole("button", { name: "打开 W3 · 通用 详情" }).click();
	await expect(page.getByRole("heading", { name: "工作者详情" })).toBeVisible();
	await expect(page.getByText("translation-worker-3")).toBeVisible();

	await page.evaluate(() => {
		const button = document.querySelector<HTMLButtonElement>(
			'button[aria-label="配置翻译 worker 数量"]',
		);
		if (!button) {
			throw new Error("translation settings button not found");
		}
		button.click();
	});
	const dialog = page.getByRole("dialog", { name: "配置翻译 worker 数量" });
	await dialog.getByLabel("通用 worker 数量").fill("2");
	await dialog.getByLabel("用户专用 worker 数量").fill("1");
	await dialog.getByRole("button", { name: "保存设置" }).click();

	await expect(dialog).toHaveCount(0);
	await expect(page.getByRole("heading", { name: "工作者详情" })).toHaveCount(
		0,
	);
	await expect(page.getByText("W3 · 通用")).toHaveCount(0);
});

test("admin refreshes translation scheduler via shared sse stream", async ({
	page,
}) => {
	await installAdminJobsMocks(page, {
		emitTranslationEvents: true,
		delayRules: [
			{
				pathname: "/api/admin/jobs/translations/status",
				afterCount: 1,
				times: 1,
				delayMs: 1200,
			},
			{
				pathname: "/api/admin/jobs/translations/requests",
				afterCount: 1,
				times: 1,
				delayMs: 1200,
			},
			{
				pathname: "/api/admin/jobs/translations/batches",
				afterCount: 1,
				times: 1,
				delayMs: 1200,
			},
		],
	});

	await page.goto("/admin/jobs");
	await page.getByRole("tab", { name: "翻译调度" }).click({ force: true });
	await expect(page.getByText("W4 · 用户专用")).toBeVisible();
	const dedicatedWorkerCard = page
		.getByText("W4 · 用户专用")
		.locator("xpath=ancestor::div[.//*[normalize-space()='已工作时长']][1]");
	const queuedRequestRow = page
		.getByText("feed.auto_translate:release:290978079")
		.locator("xpath=ancestor::tr[1]");
	await expect(queuedRequestRow.getByText("已完成")).toBeVisible();
	await expect(dedicatedWorkerCard.getByText("idle")).toBeVisible();
	await page.getByRole("tab", { name: "任务记录" }).click();
	await expect(page.getByRole("cell", { name: "deadline" })).toBeVisible();
	await expect(page.getByRole("cell", { name: "W4" }).last()).toBeVisible();
});

test("admin jobs keeps header utilities inline on tablet widths", async ({
	page,
}) => {
	await installAdminJobsMocks(page, {
		currentUserLogin: LONG_ADMIN_LOGIN,
	});

	for (const viewport of [
		{ width: 640, height: 960 },
		{ width: 757, height: 827 },
		{ width: 853, height: 1280 },
		{ width: 1023, height: 1280 },
	]) {
		await test.step(`${viewport.width}x${viewport.height}`, async () => {
			await page.setViewportSize(viewport);

			await page.goto("/admin/jobs");
			await expect(
				page.getByRole("navigation", { name: "管理员导航" }),
			).toBeVisible();
			await expect(
				page.getByRole("link", { name: "返回前台首页" }),
			).toBeVisible();

			const layout = await page.evaluate(() => {
				const mainRowElement = document.querySelector(
					"[data-admin-header-main-row]",
				);
				const navBlockElement = document.querySelector(
					"[data-admin-nav-block]",
				);
				const actionClusterElement = document.querySelector(
					"[data-admin-primary-actions]",
				);
				const loginLabelElement = document.querySelector(
					"[data-admin-login-label]",
				);
				if (
					!(mainRowElement instanceof HTMLElement) ||
					!(navBlockElement instanceof HTMLElement) ||
					!(actionClusterElement instanceof HTMLElement) ||
					!(loginLabelElement instanceof HTMLElement)
				) {
					throw new Error("Expected admin header layout anchors");
				}

				const mainRect = mainRowElement.getBoundingClientRect();
				const navRect = navBlockElement.getBoundingClientRect();
				const actionRect = actionClusterElement.getBoundingClientRect();
				const loginRect = loginLabelElement.getBoundingClientRect();
				return {
					rowOverflow: mainRowElement.scrollWidth - mainRowElement.clientWidth,
					actionTopDelta: actionRect.top - mainRect.top,
					actionVsNavTopDelta: actionRect.top - navRect.top,
					actionRight: actionRect.right,
					rowRightGap: mainRect.right - actionRect.right,
					loginRight: loginRect.right,
					rowRight: mainRect.right,
				};
			});

			expect(layout.rowOverflow).toBeLessThanOrEqual(1);
			expect(layout.actionTopDelta).toBeLessThanOrEqual(12);
			expect(layout.actionVsNavTopDelta).toBeLessThanOrEqual(12);
			expect(layout.actionRight).toBeLessThanOrEqual(layout.rowRight + 1);
			expect(layout.rowRightGap).toBeLessThanOrEqual(12);
			expect(layout.loginRight).toBeLessThanOrEqual(layout.rowRight + 1);
		});
	}
});

test("admin translation scheduler falls back to single-line mobile lists", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await installAdminJobsMocks(page);

	await page.goto("/admin/jobs");
	await expect(
		page.locator("[data-app-meta-footer-hidden='false']"),
	).toHaveCount(1);
	await page.getByRole("tab", { name: "翻译调度" }).click({ force: true });
	await expect(page.getByText("工作者板")).toBeVisible();
	await expect(page.getByText("W4 · 用户专用")).toBeVisible();
	await expect(
		page.getByText("release_detail · feed_body · entity 290978079"),
	).toBeVisible();

	await page.evaluate(() => window.scrollTo(0, 900));
	await page.waitForTimeout(120);
	await expect(
		page.locator("[data-app-meta-footer-hidden='true']"),
	).toHaveCount(1);
	await expect(page.locator("[data-admin-header-compact='true']")).toHaveCount(
		1,
	);

	await page.evaluate(() => window.scrollTo(0, 360));
	await page.waitForTimeout(120);
	await expect(page.locator("[data-admin-header-compact='true']")).toHaveCount(
		0,
	);
	await page.getByRole("tab", { name: "任务记录" }).click();
	await expect(page.getByText("W4 · 请求 1 · work items 1")).toBeVisible();

	await page.evaluate(() => window.scrollTo(0, 0));
	await page.waitForTimeout(120);
	await expect(
		page.locator("[data-app-meta-footer-hidden='false']"),
	).toHaveCount(1);
});

test("skipped subscription workflow renders skipped semantics", async ({
	page,
}) => {
	await installAdminJobsMocks(page);
	await page.goto("/admin/jobs/subscriptions");

	const skippedWorkflowCard = page
		.getByText("ID: task-subscriptions-skipped")
		.locator("xpath=ancestor::div[contains(@class, 'rounded-xl')][1]");
	await expect(
		skippedWorkflowCard.locator("[data-slot='badge']").getByText("已跳过"),
	).toBeVisible();
	await expect(
		skippedWorkflowCard.locator("[data-slot='badge']").getByText("成功"),
	).toHaveCount(0);

	await page.goto("/admin/jobs/subscriptions/task-subscriptions-skipped");

	await expect(
		page.getByRole("heading", { name: "订阅同步工作流详情" }),
	).toBeVisible();
	await expect(
		page.locator("[data-slot='badge']").getByText("已跳过"),
	).toHaveCount(7);
	await expect(page.getByText("业务结果")).toBeVisible();
	await expect(
		page.getByText("上一轮订阅同步仍在执行，本轮仅记录跳过结果。"),
	).toBeVisible();
	await expect(
		page.locator("#subscription-stage-collect").getByText("上一轮仍在执行", {
			exact: true,
		}),
	).toBeVisible();
	await expect(page.getByText("等待")).toHaveCount(0);
	await expect(
		page.locator("[data-slot='badge']").getByText("成功"),
	).toHaveCount(0);
});

test.describe("localized admin diagnostics timestamps", () => {
	test.use({ timezoneId: "Asia/Shanghai" });

	test("task detail recent events render in the browser timezone", async ({
		page,
	}) => {
		await installAdminJobsMocks(page);
		await page.goto("/admin/jobs/subscriptions/task-subscriptions-1");

		await expect(
			page.getByRole("heading", { name: "订阅同步工作流详情" }),
		).toBeVisible();
		await expect(page.getByText("最近关键事件", { exact: true })).toBeVisible();
		await expect(page.getByText("02/26, 22:31:40")).toBeVisible();
		await expect(
			page.getByText(
				"release sync candidate failed for octo/private-repo with user #4h6p9s3t5z8e2x4c",
			),
		).toBeVisible();
	});
});
