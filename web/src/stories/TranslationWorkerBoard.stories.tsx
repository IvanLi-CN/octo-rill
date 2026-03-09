import type { Meta, StoryObj } from "@storybook/react-vite";

import type { AdminTranslationWorkerStatus } from "@/api";
import { TranslationWorkerBoard } from "@/admin/TranslationWorkerBoard";

const idleWorkers: AdminTranslationWorkerStatus[] = [1, 2, 3, 4].map(
	(slot) => ({
		worker_id: `translation-worker-${slot}`,
		worker_slot: slot,
		worker_kind: slot === 4 ? "user_dedicated" : "general",
		status: "idle",
		current_batch_id: null,
		request_count: 0,
		work_item_count: 0,
		trigger_reason: null,
		updated_at: "2026-02-26T04:00:03Z",
		error_text: null,
	}),
);

const busyWorkers: AdminTranslationWorkerStatus[] = [
	{
		...idleWorkers[0],
		status: "running",
		current_batch_id: "batch-translation-11",
		request_count: 2,
		work_item_count: 6,
		trigger_reason: "deadline",
		updated_at: new Date(Date.now() - 92_000).toISOString(),
	},
	idleWorkers[1],
	{
		...idleWorkers[2],
		status: "error",
		error_text: "worker heartbeat timeout",
	},
	{
		...idleWorkers[3],
		status: "running",
		current_batch_id: "batch-translation-12",
		request_count: 1,
		work_item_count: 1,
		trigger_reason: "user_request",
		updated_at: new Date(Date.now() - 24_000).toISOString(),
	},
];

const meta = {
	title: "Admin/TranslationWorkerBoard",
	component: TranslationWorkerBoard,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"翻译调度中的独立工作者板组件，专门展示 3 个通用 worker 与 1 个用户专用 worker 的实时状态卡片。",
			},
		},
	},
	args: {
		workers: idleWorkers,
		loading: false,
	},
	render: (args) => (
		<div className="mx-auto max-w-6xl">
			<TranslationWorkerBoard {...args} />
		</div>
	),
} satisfies Meta<typeof TranslationWorkerBoard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const Busy: Story = {
	args: {
		workers: busyWorkers,
	},
};

export const Loading: Story = {
	args: {
		workers: [],
		loading: true,
	},
};

export const Empty: Story = {
	args: {
		workers: [],
		loading: false,
	},
};
