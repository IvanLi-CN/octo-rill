import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect, useRef } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";

import { AiOperationsRecordsSection } from "@/admin/AiOperationsRecordsSection";
import type {
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
		translation: null,
		polish: {
			status: "failed",
			display_status: "failed",
			status_origin: "work_item",
			retry_count: 0,
			started_at: "2026-09-05T06:10:00Z",
			last_attempt_at: "2026-09-05T06:10:01Z",
			finished_at: "2026-09-05T06:10:01Z",
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
		viewport: { viewports: INITIAL_VIEWPORTS, defaultViewport: "desktop" },
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
				const url = new URL(
					typeof input === "string" ? input : input.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/ai-records/release")) {
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
				const url = new URL(
					typeof input === "string" ? input : input.toString(),
					window.location.origin,
				);
				if (url.pathname.endsWith("/ai-records/release")) {
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
