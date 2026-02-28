import type { Meta, StoryObj } from "@storybook/react-vite";

import { TaskTypeDetailSection } from "@/admin/TaskTypeDetailSection";
import type { AdminRealtimeTaskDetailResponse } from "@/api";

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
			requested_by: 4,
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
			id: idx + 1,
			event_type: "task.progress",
			payload_json: JSON.stringify(item),
			created_at: "2026-02-26T12:00:10Z",
		})),
		event_meta: options?.eventMeta,
		diagnostics: options?.diagnostics,
	};
}

const meta = {
	title: "Admin/TaskTypeDetailSection",
	component: TaskTypeDetailSection,
	parameters: {
		layout: "padded",
	},
	args: {
		detail: buildDetail(
			"sync.releases",
			{ user_id: 4 },
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

export const Default: Story = {};

export const SyncStarred: Story = {
	args: {
		detail: buildDetail("sync.starred", { user_id: 4 }, { repos: 18 }),
	},
};

export const SyncReleases: Story = {
	args: {
		detail: buildDetail(
			"sync.releases",
			{ user_id: 4 },
			{ repos: 9, releases: 278 },
		),
	},
};

export const SyncNotifications: Story = {
	args: {
		detail: buildDetail(
			"sync.notifications",
			{ user_id: 4 },
			{ notifications: 35, since: "2026-02-25T00:00:00Z" },
		),
	},
};

export const SyncAll: Story = {
	args: {
		detail: buildDetail(
			"sync.all",
			{ user_id: 4 },
			{
				starred: { repos: 6 },
				releases: { repos: 6, releases: 120 },
				notifications: { notifications: 18, since: "2026-02-25T18:00:00Z" },
			},
		),
	},
};

export const BriefGenerate: Story = {
	args: {
		detail: buildDetail(
			"brief.generate",
			{ user_id: 4 },
			{ content_length: 3840 },
		),
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
				{ stage: "generate", index: 8, total: 24, user_id: 1008 },
				{
					stage: "user_succeeded",
					user_id: 1008,
					key_date: "2026-02-26",
					content_length: 1620,
				},
				{ stage: "generate", index: 16, total: 24, user_id: 1016 },
				{ stage: "user_failed", user_id: 1017 },
				{ stage: "generate", index: 24, total: 24, user_id: 1024 },
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
								user_id: 1008,
								key_date: "2026-02-26",
								state: "succeeded",
								error: null,
								last_event_at: "2026-02-26T12:00:25Z",
							},
							{
								user_id: 1017,
								key_date: "2026-02-26",
								state: "failed",
								error: "ai timeout",
								last_event_at: "2026-02-26T12:00:28Z",
							},
						],
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
			{ user_id: 4, release_id: "290836643" },
			{ status: "ready" },
		),
	},
};

export const TranslateReleaseBatch: Story = {
	args: {
		detail: buildDetail(
			"translate.release.batch",
			{ user_id: 4, release_ids: [290836643, 290822914, 290757276] },
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
						target_user_id: 4,
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
			{ user_id: 4, release_id: "290836643" },
			{ status: "ready" },
		),
	},
};

export const TranslateNotification: Story = {
	args: {
		detail: buildDetail(
			"translate.notification",
			{ user_id: 4, thread_id: "thread-1234" },
			{ status: "ready" },
		),
	},
};
