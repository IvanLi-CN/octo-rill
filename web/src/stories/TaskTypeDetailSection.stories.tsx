import type { Meta, StoryObj } from "@storybook/react-vite";

import { TaskTypeDetailSection } from "@/admin/TaskTypeDetailSection";
import type { AdminRealtimeTaskDetailResponse } from "@/api";

const TARGET_USER_ID = "2f4k7m9p3x6c8v2a";
const RELATED_USER_A = "4h6p9s3t5z8e2x4c";
const RELATED_USER_B = "5j7r9v3x6b8d2f4h";
const SLOT_USER_A = "6k8m2p4r7t9w3y5b";
const SLOT_USER_B = "7n9q3s5v8x2c4f6h";
const SLOT_USER_C = "8p2r4t6w9y3d5g7k";

function buildDetail(
	taskType: string,
	payload: Record<string, unknown>,
	result: Record<string, unknown> | null,
	eventPayloads: Array<Record<string, unknown>> = [],
	options?: {
		eventMeta?: AdminRealtimeTaskDetailResponse["event_meta"];
		diagnostics?: AdminRealtimeTaskDetailResponse["diagnostics"];
	},
): AdminRealtimeTaskDetailResponse {
	return {
		task: {
			id: `task-${taskType}`,
			task_type: taskType,
			status: "succeeded",
			source: "storybook.mock",
			requested_by: TARGET_USER_ID,
			parent_task_id: null,
			cancel_requested: false,
			error_message: null,
			created_at: "2026-02-26T12:00:00Z",
			started_at: "2026-02-26T12:00:05Z",
			finished_at: "2026-02-26T12:00:30Z",
			updated_at: "2026-02-26T12:00:30Z",
			payload_json: JSON.stringify(payload),
			result_json: result ? JSON.stringify(result) : null,
		},
		events: eventPayloads.map((item, idx) => ({
			id: `evt-task-${idx + 1}`,
			event_type: "task.progress",
			payload_json: JSON.stringify(item),
			created_at: "2026-02-26T12:00:10Z",
		})),
		event_meta: options?.eventMeta,
		diagnostics: options?.diagnostics,
	};
}

const meta = {
	title: "Admin/Task Type Detail",
	component: TaskTypeDetailSection,
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"任务详情区块负责把不同 task type 的参数、结果摘要与关联 LLM 调用压平展示，是 Admin Jobs 抽屉里的核心复用视图。通过这组 stories 可以验证不同任务类型的字段映射是否完整。\n\n相关公开文档：[产品说明](../product.html) · [Storybook 入口](../storybook.html)",
			},
		},
	},
	args: {
		detail: buildDetail(
			"sync.releases",
			{ user_id: TARGET_USER_ID },
			{ repos: 3, releases: 42 },
		),
	},
	render: (args) => (
		<div className="mx-auto max-w-4xl">
			<TaskTypeDetailSection {...args} />
		</div>
	),
} satisfies Meta<typeof TaskTypeDetailSection>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	parameters: {
		docs: {
			description: {
				story: "通用任务详情基线，展示同步 Release 任务的默认参数与结果摘要。",
			},
		},
	},
};

export const SyncStarred: Story = {
	args: {
		detail: buildDetail(
			"sync.starred",
			{ user_id: TARGET_USER_ID },
			{ repos: 18 },
		),
	},
};

export const SyncReleases: Story = {
	args: {
		detail: buildDetail(
			"sync.releases",
			{ user_id: TARGET_USER_ID },
			{ repos: 9, releases: 278 },
		),
	},
};

export const SyncNotifications: Story = {
	args: {
		detail: buildDetail(
			"sync.notifications",
			{ user_id: TARGET_USER_ID },
			{ notifications: 35, since: "2026-02-25T00:00:00Z" },
		),
	},
};

export const SyncAll: Story = {
	args: {
		detail: buildDetail(
			"sync.all",
			{ user_id: TARGET_USER_ID },
			{
				starred: { repos: 6 },
				releases: { repos: 6, releases: 120 },
				notifications: { notifications: 18, since: "2026-02-25T18:00:00Z" },
			},
		),
	},
};

export const SyncSubscriptions: Story = {
	args: {
		detail: buildDetail(
			"sync.subscriptions",
			{ trigger: "schedule", schedule_key: "2026-03-06T14:30" },
			{
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
			},
			[
				{ stage: "collect", total_users: 12 },
				{
					stage: "star_summary",
					total_users: 12,
					succeeded_users: 11,
					failed_users: 1,
				},
				{ stage: "repo_collect", total_repos: 128 },
				{
					stage: "release_summary",
					total_repos: 128,
					succeeded_repos: 123,
					failed_repos: 5,
					releases_written: 1840,
				},
				{
					stage: "social_summary",
					total_users: 11,
					succeeded_users: 9,
					failed_users: 2,
					repo_stars: 48,
					followers: 19,
					events: 67,
				},
				{
					stage: "notifications_summary",
					total_users: 11,
					succeeded_users: 10,
					failed_users: 1,
					notifications: 192,
				},
			],
			{
				diagnostics: {
					business_outcome: {
						code: "partial",
						label: "部分成功",
						message: "任务已完成，但存在失败或关键告警，请查看最近关键事件。",
					},
					sync_subscriptions: {
						trigger: "schedule",
						schedule_key: "2026-03-06T14:30",
						skipped: false,
						skip_reason: null,
						log_available: true,
						log_download_path:
							"/api/admin/jobs/realtime/task-sync.subscriptions/log",
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
						recent_events: [
							{
								id: "evt-sync-9",
								stage: "release",
								event_type: "repo_inaccessible",
								severity: "error",
								recoverable: false,
								attempt: 1,
								user_id: RELATED_USER_A,
								repo_id: 9001,
								repo_full_name: "octo/private-repo",
								message:
									"release sync candidate failed for octo/private-repo with user #4h6p9s3t5z8e2x4c",
								created_at: "2026-03-06T14:31:40Z",
							},
							{
								id: "evt-sync-8",
								stage: "release",
								event_type: "rate_limited",
								severity: "warning",
								recoverable: true,
								attempt: 2,
								user_id: RELATED_USER_B,
								repo_id: 8128,
								repo_full_name: "octo/public-repo",
								message:
									"retryable release sync error for octo/public-repo with user #5j7r9v3x6b8d2f4h",
								created_at: "2026-03-06T14:31:10Z",
							},
							{
								id: "evt-sync-7",
								stage: "social",
								event_type: "social_sync_failed",
								severity: "error",
								recoverable: false,
								attempt: 1,
								user_id: RELATED_USER_B,
								repo_id: null,
								repo_full_name: null,
								message:
									"failed to refresh social activity for user #5j7r9v3x6b8d2f4h",
								created_at: "2026-03-06T14:32:10Z",
							},
							{
								id: "evt-sync-6",
								stage: "notifications",
								event_type: "notifications_sync_failed",
								severity: "error",
								recoverable: false,
								attempt: 1,
								user_id: RELATED_USER_A,
								repo_id: null,
								repo_full_name: null,
								message:
									"failed to refresh inbox notifications for user #4h6p9s3t5z8e2x4c",
								created_at: "2026-03-06T14:32:40Z",
							},
						],
					},
				},
			},
		),
	},
};

export const SyncSubscriptionsSkipped: Story = {
	args: {
		detail: buildDetail(
			"sync.subscriptions",
			{ trigger: "schedule", schedule_key: "2026-03-06T15:00" },
			{
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
			},
			[{ stage: "skipped", skip_reason: "previous_run_active" }],
			{
				diagnostics: {
					business_outcome: {
						code: "disabled",
						label: "已跳过",
						message: "上一轮订阅同步仍在执行，本轮仅记录跳过结果。",
					},
					sync_subscriptions: {
						trigger: "schedule",
						schedule_key: "2026-03-06T15:00",
						skipped: true,
						skip_reason: "previous_run_active",
						log_available: true,
						log_download_path:
							"/api/admin/jobs/realtime/task-sync.subscriptions/log",
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
			},
		),
	},
};

export const BriefGenerate: Story = {
	args: {
		detail: buildDetail(
			"brief.generate",
			{ user_id: TARGET_USER_ID, key_date: "2026-02-26" },
			{
				content_length: 3840,
				brief_id: "brief_20260226",
				date: "2026-02-26",
				window_start_utc: "2026-02-25T00:00:00Z",
				window_end_utc: "2026-02-26T00:00:00Z",
				effective_time_zone: "Asia/Shanghai",
				effective_local_boundary: "08:00",
				release_count: 5,
			},
			[],
			{
				diagnostics: {
					business_outcome: {
						code: "ok",
						label: "生成成功",
						message: "日报快照已写入数据库并补齐 release memberships。",
					},
					brief_generate: {
						target_user_id: TARGET_USER_ID,
						content_length: 3840,
						key_date: "2026-02-26",
						brief_id: "brief_20260226",
						date: "2026-02-26",
						window_start_utc: "2026-02-25T00:00:00Z",
						window_end_utc: "2026-02-26T00:00:00Z",
						effective_time_zone: "Asia/Shanghai",
						effective_local_boundary: "08:00",
						release_count: 5,
					},
				},
			},
		),
	},
};

export const WithRelatedLlmCalls: Story = {
	args: {
		detail: buildDetail(
			"sync.releases",
			{ user_id: TARGET_USER_ID },
			{ repos: 9, releases: 278 },
		),
		relatedLlmCalls: [
			{
				id: "llm-call-story-1",
				status: "failed",
				source: "job.api.translate_release",
				model: "gpt-4o-mini",
				requested_by: TARGET_USER_ID,
				parent_task_id: "task-sync.releases",
				parent_task_type: "sync.releases",
				max_tokens: 900,
				attempt_count: 2,
				scheduler_wait_ms: 240,
				first_token_wait_ms: 310,
				duration_ms: 1400,
				input_tokens: 800,
				output_tokens: 0,
				cached_input_tokens: 320,
				total_tokens: 800,
				created_at: "2026-02-26T12:00:20Z",
				started_at: "2026-02-26T12:00:21Z",
				finished_at: "2026-02-26T12:00:22Z",
				updated_at: "2026-02-26T12:00:22Z",
			},
		],
	},
};

export const BriefDailySlot: Story = {
	args: {
		detail: buildDetail(
			"brief.daily_slot",
			{ hour_utc: 8 },
			{ hour_utc: 8, total: 24, succeeded: 21, failed: 3 },
			[
				{ stage: "collect", total_users: 24, hour_utc: 8 },
				{ stage: "generate", index: 8, total: 24, user_id: SLOT_USER_A },
				{
					stage: "user_succeeded",
					user_id: SLOT_USER_A,
					key_date: "2026-02-26",
					content_length: 1620,
				},
				{ stage: "generate", index: 16, total: 24, user_id: SLOT_USER_B },
				{ stage: "user_failed", user_id: SLOT_USER_C },
				{ stage: "generate", index: 24, total: 24, user_id: SLOT_USER_B },
				{
					stage: "summary",
					total: 24,
					succeeded: 21,
					failed: 3,
					canceled: false,
				},
			],
			{
				diagnostics: {
					business_outcome: {
						code: "partial",
						label: "部分成功",
						message: "部分用户日报生成成功，部分失败。",
					},
					brief_daily_slot: {
						hour_utc: 8,
						summary: {
							total_users: 24,
							progressed_users: 24,
							succeeded_users: 21,
							failed_users: 3,
							canceled: false,
						},
						users: [
							{
								user_id: SLOT_USER_A,
								key_date: "2026-02-26",
								state: "succeeded",
								error: null,
								local_boundary: "08:00",
								time_zone: "Asia/Shanghai",
								window_start_utc: "2026-02-25T00:00:00Z",
								window_end_utc: "2026-02-26T00:00:00Z",
								last_event_at: "2026-02-26T12:00:25Z",
							},
							{
								user_id: SLOT_USER_C,
								key_date: "2026-02-26",
								state: "failed",
								error: "ai timeout",
								local_boundary: "09:00",
								time_zone: "America/New_York",
								window_start_utc: "2026-02-25T14:00:00Z",
								window_end_utc: "2026-02-26T14:00:00Z",
								last_event_at: "2026-02-26T12:00:28Z",
							},
						],
					},
				},
			},
		),
	},
};

export const BriefHistoryRecompute: Story = {
	args: {
		detail: buildDetail(
			"brief.history_recompute",
			{},
			{},
			[
				{
					stage: "progress",
					total: 12,
					processed: 7,
					succeeded: 6,
					failed: 1,
					brief_id: "brief_legacy_07",
				},
			],
			{
				diagnostics: {
					business_outcome: {
						code: "partial",
						label: "部分成功",
						message: "历史重算已推进过半，仍有少量 legacy brief 需要重试。",
					},
					brief_history_recompute: {
						total: 12,
						processed: 7,
						succeeded: 6,
						failed: 1,
						current_brief_id: "brief_legacy_07",
						last_error: "invalid legacy brief date: 2026-02-30",
					},
				},
			},
		),
	},
};

export const TranslateRelease: Story = {
	args: {
		detail: buildDetail(
			"translate.release",
			{ user_id: TARGET_USER_ID, release_id: "290836643" },
			{ status: "ready" },
		),
	},
};

export const TranslateReleaseBatch: Story = {
	args: {
		detail: buildDetail(
			"translate.release.batch",
			{
				user_id: TARGET_USER_ID,
				release_ids: ["290836643", "290822914", "290757276"],
			},
			{ total: 3, ready: 2, missing: 1, disabled: 0, error: 0 },
			[
				{ stage: "collect", total_releases: 3 },
				{ stage: "release", release_id: "290836643", item_status: "ready" },
				{ stage: "release", release_id: "290822914", item_status: "ready" },
				{
					stage: "release",
					release_id: "290757276",
					item_status: "missing",
					item_error: "release not found",
				},
			],
			{
				eventMeta: {
					returned: 3,
					total: 6,
					limit: 3,
					truncated: true,
				},
				diagnostics: {
					business_outcome: {
						code: "partial",
						label: "部分成功",
						message: "部分 Release 翻译成功，部分失败或缺失。",
					},
					translate_release_batch: {
						target_user_id: TARGET_USER_ID,
						release_total: 3,
						summary: {
							total: 3,
							ready: 2,
							missing: 1,
							disabled: 0,
							error: 0,
						},
						progress: {
							processed: 3,
							last_stage: "release",
						},
						items: [
							{
								release_id: "290836643",
								item_status: "ready",
								item_error: null,
								last_event_at: "2026-02-26T12:00:15Z",
							},
							{
								release_id: "290822914",
								item_status: "ready",
								item_error: null,
								last_event_at: "2026-02-26T12:00:20Z",
							},
							{
								release_id: "290757276",
								item_status: "missing",
								item_error: "release not found",
								last_event_at: "2026-02-26T12:00:25Z",
							},
						],
					},
				},
			},
		),
	},
};

export const TranslateReleaseDetail: Story = {
	args: {
		detail: buildDetail(
			"translate.release_detail",
			{ user_id: TARGET_USER_ID, release_id: "290836643" },
			{ status: "ready" },
		),
	},
};

export const TranslateNotification: Story = {
	args: {
		detail: buildDetail(
			"translate.notification",
			{ user_id: TARGET_USER_ID, thread_id: "thread-1234" },
			{ status: "ready" },
		),
	},
};
