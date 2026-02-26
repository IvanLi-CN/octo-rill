import type { Meta, StoryObj } from "@storybook/react-vite";

import { TaskTypeDetailPage } from "@/admin/TaskTypeDetailPage";
import type { AdminRealtimeTaskDetailResponse } from "@/api";

function buildDetail(
	taskType: string,
	payload: Record<string, unknown>,
	result: Record<string, unknown> | null,
	eventPayloads: Array<Record<string, unknown>> = [],
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
	};
}

const meta = {
	title: "Admin/TaskTypeDetailPage",
	component: TaskTypeDetailPage,
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
			<TaskTypeDetailPage {...args} />
		</div>
	),
} satisfies Meta<typeof TaskTypeDetailPage>;

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
				{ stage: "generate", index: 16, total: 24, user_id: 1016 },
				{ stage: "user_failed", user_id: 1017 },
				{ stage: "generate", index: 24, total: 24, user_id: 1024 },
			],
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
			],
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
