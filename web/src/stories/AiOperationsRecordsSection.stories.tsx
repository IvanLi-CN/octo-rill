import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { AiOperationsRecordsSection } from "@/admin/AiOperationsRecordsSection";
import type {
	AdminCollectionActivityResponse,
	AdminCollectionRecordDetail,
	AdminCollectionRecordsResponse,
	AdminLlmCallDetailResponse,
} from "@/api";

export const failedCall: AdminLlmCallDetailResponse = {
	id: "call-release-smart-1",
	status: "succeeded",
	source: "translation.scheduler.release_smart.work_item.383114065",
	model: "gpt-4.1-mini",
	requested_by: null,
	parent_task_id: null,
	parent_task_type: null,
	max_tokens: 1024,
	attempt_count: 1,
	scheduler_wait_ms: 12,
	first_token_wait_ms: 210,
	duration_ms: 980,
	input_tokens: 360,
	output_tokens: 1024,
	finish_reason: "length",
	provider_request_id: "req-demo-383114065",
	provider_http_status: 200,
	processing_stage: "release_smart_body",
	provider_status: "succeeded",
	output_contract_status: "failed",
	retry_disposition: "scheduled",
	relation_role: "primary",
	evidence_availability: "available",
	cached_input_tokens: null,
	total_tokens: 1384,
	failure_class: null,
	final_model: "gpt-4.1-mini",
	fallback_count: 0,
	retry_scheduled_at: null,
	recovery_attempt_count: 0,
	created_at: "2026-09-05T06:10:00Z",
	started_at: "2026-09-05T06:10:00Z",
	finished_at: "2026-09-05T06:10:01Z",
	updated_at: "2026-09-05T06:10:01Z",
	input_messages_json: null,
	output_messages_json: null,
	prompt_text: "system: summarize a release\n\nuser: Bun v1.4.2",
	response_text: '{"valuable":true,"title_zh":"Bun v1.4.2 新增',
	error_text: null,
	attempt_history: [
		{
			event_type: "llm.succeeded",
			status: "succeeded",
			model: "gpt-4.1-mini",
			attempt: 1,
			failure_class: null,
			retry_after_ms: null,
			from_model: null,
			to_model: null,
			fallback_count: 0,
			finish_reason: "length",
			provider_request_id: "req-demo-383114065",
			provider_http_status: 200,
			created_at: "2026-09-05T06:10:01Z",
		},
	],
};

const recordDetail: AdminCollectionRecordDetail = {
	record: {
		id: "383114065",
		kind: "release",
		repository: "oven-sh/bun",
		title: "Bun v1.4.2",
		occurred_at: "2026-09-05T05:55:00Z",
		detected_at: "2026-09-05T06:00:00Z",
		generated_at: null,
		translation: {
			status: "running",
			display_status: "running",
			status_origin: "global_work",
			retry_count: 1,
			started_at: "2026-09-05T06:08:00Z",
			last_attempt_at: "2026-09-05T06:09:00Z",
			finished_at: null,
			global_work: {
				status: "running",
				status_origin: "global_work",
				work_item_id: "global-work-383114065",
				source_hash: "hash-demo",
				updated_at: "2026-09-05T06:09:00Z",
			},
			result_projection: {
				status: "ready",
				status_origin: "result_projection",
				work_item_id: "global-work-old",
				source_hash: "hash-old",
				updated_at: "2026-09-05T06:00:00Z",
			},
			legacy_evidence: {
				status: "legacy_conflict",
				status_origin: "legacy_evidence",
				work_item_id: null,
				source_hash: null,
				updated_at: null,
			},
		},
		polish: {
			status: "failed",
			display_status: "failed",
			status_origin: "work_item",
			retry_count: 0,
			started_at: "2026-09-05T06:10:00Z",
			last_attempt_at: "2026-09-05T06:10:01Z",
			finished_at: "2026-09-05T06:10:01Z",
			global_work: {
				status: "failed",
				status_origin: "global_work",
				work_item_id: "global-work-383114065",
				source_hash: "hash-demo",
				updated_at: "2026-09-05T06:10:01Z",
			},
			result_projection: null,
			legacy_evidence: {
				status: "legacy_cached",
				status_origin: "legacy_evidence",
				work_item_id: null,
				source_hash: null,
				updated_at: "2026-09-05T05:59:00Z",
			},
		},
	},
	attempts: [
		{
			id: "work-item-383114065:1",
			pipeline: "polish",
			attempt_no: 1,
			trigger: "initial",
			status: "error",
			started_at: "2026-09-05T06:10:00Z",
			last_attempt_at: "2026-09-05T06:10:01Z",
			finished_at: "2026-09-05T06:10:01Z",
			error_code: "output_contract_invalid",
			error_summary: "模型调用成功，但输出未通过 JSON 契约",
			failure_class: "empty_content",
			processing_stage: "release_smart",
			provider_status: "succeeded",
			output_contract_status: "failed",
			retry_disposition: "scheduled",
			retry_eligible: true,
			next_retry_at: "2026-09-05T06:11:01Z",
			llm_calls: [
				{
					id: failedCall.id,
					status: failedCall.status,
					source: failedCall.source,
					model: failedCall.model,
				},
			],
		},
	],
};

const listResponse: AdminCollectionRecordsResponse = {
	items: [recordDetail.record],
	page: 1,
	page_size: 20,
	total: 1,
};

function activityResponse(
	kind: AdminCollectionRecordDetail["record"]["kind"] = "release",
): AdminCollectionActivityResponse {
	const windowStart = new Date("2026-09-20T00:00:00Z");
	const cells = [
		{
			id: "383114065",
			title: "Bun v1.4.2",
			repository: "oven-sh/bun",
			source_time: "2026-09-20T11:55:00Z",
			translation_status: "running",
			polish_status: "failed",
			composite_status: "exception" as const,
		},
		{
			id: "announcement-42",
			title: "Runtime compatibility notice",
			repository: "oven-sh/bun",
			source_time: "2026-09-20T10:35:00Z",
			translation_status: "succeeded",
			polish_status: "queued",
			composite_status: "processing" as const,
		},
		{
			id: "notification-32",
			title: "New security advisory",
			repository: "oven-sh/bun",
			source_time: "2026-09-20T09:30:00Z",
			translation_status: "succeeded",
			polish_status: "not_applicable",
			composite_status: "completed" as const,
		},
		{
			id: "release-previous",
			title: "A release with historical evidence only",
			repository: "oven-sh/bun",
			source_time: "2026-09-20T08:15:00Z",
			translation_status: "historical_unknown",
			polish_status: "legacy_cached",
			composite_status: "neutral" as const,
		},
	];
	const cellsByHour = new Map<number, typeof cells>();
	for (const cell of cells) {
		const hour = Number(cell.source_time.slice(11, 13));
		const bucketIndex = 11 - hour;
		const bucketCells = cellsByHour.get(bucketIndex) ?? [];
		bucketCells.push(cell);
		cellsByHour.set(bucketIndex, bucketCells);
	}
	return {
		kind,
		bucket_minutes: 60,
		bucket_count: 12,
		window_started_at: windowStart.toISOString(),
		window_ended_at: "2026-09-20T12:00:00Z",
		summary: {
			content_count: cells.length,
			completed_count: 1,
			processing_count: 1,
			exception_count: 1,
			neutral_count: 1,
		},
		buckets: Array.from({ length: 12 }, (_, index) => {
			const startedAt = new Date(
				windowStart.getTime() + (11 - index) * 3_600_000,
			);
			return {
				started_at: startedAt.toISOString(),
				ended_at: new Date(startedAt.getTime() + 3_600_000).toISOString(),
				cells: cellsByHour.get(index) ?? [],
			};
		}),
	};
}

export const expiredCall: AdminLlmCallDetailResponse = {
	...failedCall,
	id: "call-release-smart-expired",
	status: "failed",
	response_text: null,
	output_contract_status: "failed",
	evidence_availability: "expired",
	relation_role: "primary",
};

const expiredRecordDetail: AdminCollectionRecordDetail = {
	...recordDetail,
	attempts: [
		{
			...recordDetail.attempts[0],
			llm_calls: [
				{
					id: expiredCall.id,
					status: expiredCall.status,
					source: expiredCall.source,
					model: expiredCall.model,
					relation_role: "primary",
					evidence_availability: "expired",
				},
			],
		},
	],
};

const meta = {
	title: "Admin/AiOperationsRecordsSection",
	component: AiOperationsRecordsSection,
	tags: ["autodocs"],
	parameters: {
		viewport: {
			viewports: {
				...INITIAL_VIEWPORTS,
				adminMobile: {
					name: "Admin mobile",
					styles: { width: "393px", height: "852px" },
				},
			},
			defaultViewport: "desktop",
		},
		docs: {
			description: {
				component:
					"失败的 Release 润色详情默认展开脱敏响应，并保留 provider、输出契约与过期证据状态。",
			},
		},
	},
} satisfies Meta<typeof AiOperationsRecordsSection>;

export default meta;
type Story = StoryObj<typeof meta>;

export const FailedResponseWithDiagnostics: Story = {
	args: {
		detailRoute: {
			kind: "release",
			id: recordDetail.record.id,
			attemptId: recordDetail.attempts[0].id,
			llmCallId: failedCall.id,
		},
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				if (url.pathname.includes("/ai-records/release")) {
					return new Response(JSON.stringify(listResponse), { status: 200 });
				}
				if (url.pathname.endsWith("/ai-records/release/383114065")) {
					return new Response(JSON.stringify(recordDetail), { status: 200 });
				}
				if (url.pathname.endsWith(`/llm/calls/${failedCall.id}`)) {
					return new Response(JSON.stringify(failedCall), { status: 200 });
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return (
				<div
					data-visual-evidence-surface
					className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
				>
					<div data-visual-evidence-target className="mx-auto max-w-5xl p-6">
						<Story />
					</div>
				</div>
			);
		},
	],
};

export const GlobalEvidenceOverview: Story = {
	args: {
		detailRoute: {
			kind: "release",
			id: recordDetail.record.id,
		},
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				if (url.pathname.includes("/ai-records/release")) {
					return new Response(JSON.stringify(listResponse), { status: 200 });
				}
				if (url.pathname.endsWith("/ai-records/release/383114065")) {
					return new Response(JSON.stringify(recordDetail), { status: 200 });
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return (
				<div
					data-visual-evidence-surface
					className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
				>
					<div data-visual-evidence-target className="mx-auto max-w-5xl p-6">
						<Story />
					</div>
				</div>
			);
		},
	],
};

export const ExpiredDiagnosticEvidence: Story = {
	args: {
		detailRoute: {
			kind: "release",
			id: expiredRecordDetail.record.id,
			attemptId: expiredRecordDetail.attempts[0].id,
			llmCallId: expiredCall.id,
		},
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				if (url.pathname.includes("/ai-records/release")) {
					return new Response(
						JSON.stringify({
							...listResponse,
							items: [expiredRecordDetail.record],
						}),
						{ status: 200 },
					);
				}
				if (url.pathname.endsWith("/ai-records/release/383114065")) {
					return new Response(JSON.stringify(expiredRecordDetail), {
						status: 200,
					});
				}
				if (url.pathname.endsWith(`/llm/calls/${expiredCall.id}`)) {
					return new Response(JSON.stringify(expiredCall), { status: 200 });
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return (
				<div
					data-visual-evidence-surface
					className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
				>
					<div data-visual-evidence-target className="mx-auto max-w-5xl p-6">
						<Story />
					</div>
				</div>
			);
		},
	],
};

export const TimeoutRead: Story = {
	tags: ["admin-collection-read-budget"],
	args: {
		detailRoute: null,
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: {
								code: "admin_collection_records_timeout",
								message: "admin collection records read timed out",
							},
						}),
						{ status: 503, headers: { "content-type": "application/json" } },
					);
				}
				if (url.pathname.includes("/ai-records/release")) {
					return new Response(
						JSON.stringify({
							ok: false,
							error: {
								code: "admin_collection_records_timeout",
								message: "admin collection records read timed out",
							},
						}),
						{ status: 503, headers: { "content-type": "application/json" } },
					);
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return (
				<div
					data-visual-evidence-surface
					className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
				>
					<div data-visual-evidence-target className="mx-auto max-w-5xl p-6">
						<Story />
					</div>
				</div>
			);
		},
	],
	play: async ({ canvasElement }) => {
		await waitFor(() =>
			expect(
				within(canvasElement).getByRole("heading", {
					name: "记录暂时无法读取",
				}),
			).toBeVisible(),
		);
		const errorPanel = within(canvasElement).getByRole("alert");
		expect(
			within(errorPanel).getByRole("button", { name: "刷新记录" }),
		).toBeVisible();
	},
};

export const CancelsStaleRead: Story = {
	tags: ["admin-collection-read-budget"],
	args: {
		detailRoute: null,
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			(
				window as Window & { __adminCollectionAbortCount?: number }
			).__adminCollectionAbortCount = 0;
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				if (url.pathname.includes("/ai-records/release")) {
					return await new Promise<Response>((_resolve, reject) => {
						init?.signal?.addEventListener("abort", () => {
							const target = window as Window & {
								__adminCollectionAbortCount?: number;
							};
							target.__adminCollectionAbortCount =
								(target.__adminCollectionAbortCount ?? 0) + 1;
							reject(new DOMException("aborted", "AbortError"));
						});
					});
				}
				if (url.pathname.endsWith("/ai-records/announcement")) {
					return new Response(JSON.stringify(listResponse), { status: 200 });
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
				},
				[restoreFetch],
			);
			return <Story />;
		},
	],
	play: async ({ canvasElement }) => {
		await userEvent.click(
			within(canvasElement).getByRole("tab", { name: "公告" }),
		);
		await waitFor(() => {
			expect(
				(window as Window & { __adminCollectionAbortCount?: number })
					.__adminCollectionAbortCount,
			).toBeGreaterThan(0);
		});
	},
};

type ActivityRequestWindow = Window & {
	__collectionActivityPaths?: string[];
};

type ActivityCancellationWindow = ActivityRequestWindow & {
	__collectionActivityAbortCount?: number;
	__collectionActivityNowOffset?: number;
	__resolveCollectionActivity?: () => void;
};

export const ActivityReadIsIndependentOfListPaging: Story = {
	tags: ["admin-collection-activity"],
	args: {
		detailRoute: null,
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const originalNow = useRef(Date.now);
			const activityCallCount = useRef(0);
			const restoreFetch = originalFetch.current;
			const requestWindow = window as ActivityCancellationWindow;
			requestWindow.__collectionActivityPaths = [];
			requestWindow.__collectionActivityAbortCount = 0;
			requestWindow.__collectionActivityNowOffset = 0;
			Date.now = () =>
				originalNow.current() +
				(requestWindow.__collectionActivityNowOffset ?? 0);
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					requestWindow.__collectionActivityPaths?.push(url.pathname);
					activityCallCount.current += 1;
					if (activityCallCount.current === 3) {
						return await new Promise<Response>((resolve, reject) => {
							const signal = init?.signal;
							const kind = url.pathname
								.split("/")
								.at(-2) as AdminCollectionActivityResponse["kind"];
							requestWindow.__resolveCollectionActivity = () => {
								resolve(
									new Response(JSON.stringify(activityResponse(kind)), {
										status: 200,
									}),
								);
							};
							const rejectOnAbort = () => {
								requestWindow.__collectionActivityAbortCount =
									(requestWindow.__collectionActivityAbortCount ?? 0) + 1;
								reject(new DOMException("aborted", "AbortError"));
							};
							if (signal?.aborted) rejectOnAbort();
							else
								signal?.addEventListener("abort", rejectOnAbort, {
									once: true,
								});
						});
					}
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				const listKind = url.pathname.match(
					/\/ai-records\/(release|announcement|notification|brief)$/,
				)?.[1] as AdminCollectionActivityResponse["kind"] | undefined;
				if (listKind) {
					return new Response(
						JSON.stringify({
							...listResponse,
							items: listResponse.items.map((item) => ({
								...item,
								kind: listKind,
							})),
							total: 40,
						}),
						{ status: 200 },
					);
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
					Date.now = originalNow.current;
					delete requestWindow.__collectionActivityPaths;
					delete requestWindow.__collectionActivityAbortCount;
					delete requestWindow.__collectionActivityNowOffset;
					delete requestWindow.__resolveCollectionActivity;
				},
				[originalNow, restoreFetch, requestWindow],
			);
			return <Story />;
		},
	],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const requestWindow = window as ActivityCancellationWindow;
		const releasePath = "/api/admin/jobs/ai-records/release/activity";
		await waitFor(() =>
			expect(requestWindow.__collectionActivityPaths).toEqual([releasePath]),
		);
		requestWindow.__collectionActivityNowOffset = 6_000;
		await userEvent.click(canvas.getByRole("tab", { name: "公告" }));
		await waitFor(() =>
			expect(requestWindow.__collectionActivityPaths).toContain(
				"/api/admin/jobs/ai-records/announcement/activity",
			),
		);
		await userEvent.click(canvas.getByRole("tab", { name: "Release" }));
		await waitFor(() =>
			expect(
				requestWindow.__collectionActivityPaths?.filter(
					(path) => path === releasePath,
				),
			).toHaveLength(2),
		);
		await waitFor(() =>
			expect(canvas.getByRole("button", { name: "下一页" })).toBeEnabled(),
		);
		await userEvent.click(canvas.getByRole("button", { name: "下一页" }));
		await expect(canvas.getByText("共 40 条 · 第 2/2 页")).toBeVisible();
		await expect(requestWindow.__collectionActivityAbortCount).toBe(0);
		await expect(requestWindow.__collectionActivityPaths).toHaveLength(3);
		requestWindow.__resolveCollectionActivity?.();
		await waitFor(() =>
			expect(canvas.queryByText("正在更新")).not.toBeInTheDocument(),
		);
	},
};

export const ActivityTabIsolation: Story = {
	tags: ["admin-collection-activity"],
	args: {
		detailRoute: null,
		onFiltersChange: () => undefined,
		onOpenRecord: () => undefined,
		onOpenAttempt: () => undefined,
		onOpenLlm: () => undefined,
		onCloseRecord: () => undefined,
	},
	decorators: [
		(Story) => {
			const originalFetch = useRef(window.fetch);
			const restoreFetch = originalFetch.current;
			const requestWindow = window as ActivityRequestWindow;
			requestWindow.__collectionActivityPaths = [];
			window.fetch = async (input, init) => {
				const requestInput = input instanceof Request ? input.url : input;
				const url = new URL(
					typeof requestInput === "string"
						? requestInput
						: requestInput.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/activity")) {
					requestWindow.__collectionActivityPaths?.push(url.pathname);
					const kind = url.pathname
						.split("/")
						.at(-2) as AdminCollectionActivityResponse["kind"];
					return new Response(JSON.stringify(activityResponse(kind)), {
						status: 200,
					});
				}
				if (
					/\/ai-records\/(release|announcement|notification|brief)$/.test(
						url.pathname,
					)
				) {
					const kind = url.pathname
						.split("/")
						.at(-1) as AdminCollectionActivityResponse["kind"];
					return new Response(
						JSON.stringify({
							...listResponse,
							items: listResponse.items.map((item) => ({ ...item, kind })),
							total: 40,
						}),
						{ status: 200 },
					);
				}
				return restoreFetch(input, init);
			};
			useEffect(
				() => () => {
					window.fetch = restoreFetch;
					delete requestWindow.__collectionActivityPaths;
				},
				[restoreFetch, requestWindow],
			);
			return (
				<div
					data-visual-evidence-surface
					className="mx-auto box-border w-full max-w-[1072px] bg-background p-6"
				>
					<div data-visual-evidence-target className="mx-auto max-w-5xl p-6">
						<Story />
					</div>
				</div>
			);
		},
	],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const requestWindow = window as ActivityRequestWindow;
		await waitFor(() =>
			expect(requestWindow.__collectionActivityPaths).toContain(
				"/api/admin/jobs/ai-records/release/activity",
			),
		);
		await userEvent.click(canvas.getByRole("tab", { name: "公告" }));
		await waitFor(() =>
			expect(requestWindow.__collectionActivityPaths).toContain(
				"/api/admin/jobs/ai-records/announcement/activity",
			),
		);
		const requestCount = requestWindow.__collectionActivityPaths?.length ?? 0;
		const nextPageButton = canvas.getByRole("button", { name: "下一页" });
		await waitFor(() => {
			expect(canvas.getByText(/第 1\/2 页/)).toBeVisible();
			expect(nextPageButton).toBeEnabled();
		});
		await userEvent.click(nextPageButton);
		await waitFor(() => expect(canvas.getByText(/第 2\/2 页/)).toBeVisible());
		await expect(requestWindow.__collectionActivityPaths).toHaveLength(
			requestCount,
		);
	},
};
