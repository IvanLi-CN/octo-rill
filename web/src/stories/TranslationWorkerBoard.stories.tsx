import type { Meta, StoryObj } from "@storybook/react-vite";
import { Settings2 } from "lucide-react";

import type { AdminTranslationWorkerStatus } from "@/api";
import { TranslationWorkerBoard } from "@/admin/TranslationWorkerBoard";
import { Button } from "@/components/ui/button";

const idleWorkers: AdminTranslationWorkerStatus[] = [1, 2, 3, 4].map(
	(slot) => ({
		worker_id:
			slot === 4
				? "translation-worker-user-dedicated-1"
				: `translation-worker-general-${slot}`,
		worker_slot: slot,
		worker_kind: slot === 4 ? "user_dedicated" : "general",
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
		error_code: "unknown_internal_error",
		error_summary: "翻译失败",
		error_detail: "worker heartbeat timeout",
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

const resizedWorkers: AdminTranslationWorkerStatus[] = [
	...Array.from({ length: 5 }, (_, index) => ({
		worker_id: `translation-worker-general-${index + 1}`,
		worker_slot: index + 1,
		worker_kind: "general" as const,
		status: index === 1 ? ("running" as const) : ("idle" as const),
		current_batch_id: index === 1 ? "batch-translation-21" : null,
		request_count: index === 1 ? 3 : 0,
		work_item_count: index === 1 ? 7 : 0,
		trigger_reason: index === 1 ? "token_threshold" : null,
		updated_at:
			index === 1
				? new Date(Date.now() - 42_000).toISOString()
				: "2026-02-26T04:00:03Z",
		error_text: null,
		error_code: null,
		error_summary: null,
		error_detail: null,
	})),
	...Array.from({ length: 2 }, (_, index) => ({
		worker_id: `translation-worker-user-dedicated-${index + 1}`,
		worker_slot: 6 + index,
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

const meta = {
	title: "Admin/TranslationWorkerBoard",
	component: TranslationWorkerBoard,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"翻译调度中的独立工作者板组件，支持在 header 右侧挂载设置入口，并展示当前 worker 槽位状态。",
			},
		},
	},
	args: {
		workers: idleWorkers,
		loading: false,
		headerAction: (
			<Button
				type="button"
				variant="outline"
				size="icon"
				aria-label="配置翻译 worker 数量"
			>
				<Settings2 />
			</Button>
		),
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

export const ResizedAfterSave: Story = {
	args: {
		workers: resizedWorkers,
		description:
			"目标配置为 5 个通用 worker 与 2 个用户专用 worker；下方展示实时槽位状态。",
	},
};
