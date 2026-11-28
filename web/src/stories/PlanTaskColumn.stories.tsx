import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fn, screen, userEvent, within } from "storybook/test";

import type { AdminPlanTaskType } from "@/api";
import { PlanTaskColumn } from "@/admin/PlanTaskColumn";

const polishBackfill: AdminPlanTaskType = {
	task_type: "polish.backfill",
	label: "润色受众变体回填",
	group_key: "translation",
	group_label: "翻译",
	lane: "scheduled",
	triggerable: true,
	trigger_params: [
		{
			name: "page_size",
			label: "单次扫描 Release 数",
			kind: "integer",
			min: 25,
			max: 100,
			default: 25,
		},
	],
};

const syncReleases: AdminPlanTaskType = {
	task_type: "sync.releases",
	label: "同步 Release",
	group_key: "sync",
	group_label: "同步",
	lane: "scheduled",
	triggerable: false,
	trigger_params: [],
};

const briefSlot: AdminPlanTaskType = {
	task_type: "brief.daily_slot",
	label: "定时日报",
	group_key: "briefs",
	group_label: "日报",
	lane: "scheduled",
	triggerable: false,
	trigger_params: [],
};

const meta = {
	title: "Admin/PlanTaskColumn",
	component: PlanTaskColumn,
	args: {
		tasks: [polishBackfill, syncReleases, briefSlot],
		selectedTaskType: "polish.backfill",
		onSelect: fn(),
		inflightTaskTypes: {},
		onTrigger: fn(async () => {}),
		triggerBusyTaskType: null,
	},
} satisfies Meta<typeof PlanTaskColumn>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	tags: ["plan-task-column"],
};

export const InflightDisablesTrigger: Story = {
	args: { inflightTaskTypes: { "polish.backfill": "running" } },
	tags: ["plan-task-column"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("运行中")).toBeTruthy();
		await expect(
			canvas.queryByRole("button", { name: "触发 润色受众变体回填" }),
		).toBeNull();
	},
};

export const TriggerPopupShowsParams: Story = {
	tags: ["plan-task-column"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(
			canvas.getByRole("button", { name: "触发 润色受众变体回填" }),
		);
		// The popover renders in a portal outside the story canvas.
		await expect(await screen.findByText("单次扫描 Release 数")).toBeTruthy();
		await expect(screen.getByText("允许范围 25–100")).toBeTruthy();
	},
};

export const SelectionIsMarked: Story = {
	tags: ["plan-task-column"],
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const selected = canvas.getByRole("button", { current: true });
		await expect(selected.textContent).toContain("润色受众变体回填");
	},
};
