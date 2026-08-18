import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, within } from "storybook/test";

import {
	LlmCallTimeRangeFilter,
	type LlmCallTimeRangeValue,
} from "@/admin/LlmCallTimeRangeFilter";

const TIME_RANGE_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	llmTimeRangeMobile393: {
		name: "LLM time range mobile 393x852",
		styles: { width: "393px", height: "852px" },
		type: "mobile",
	},
} as const;

function TimeRangeFilterStory(props: {
	started: LlmCallTimeRangeValue;
	finished: LlmCallTimeRangeValue;
}) {
	const [started, setStarted] = useState(props.started);
	const [finished, setFinished] = useState(props.finished);
	return (
		<div className="grid gap-2 sm:grid-cols-2">
			<LlmCallTimeRangeFilter
				label="开始时间"
				value={started}
				onValueChange={setStarted}
			/>
			<LlmCallTimeRangeFilter
				label="结束时间"
				exclusiveUpperBound
				value={finished}
				onValueChange={setFinished}
			/>
		</div>
	);
}

const meta = {
	title: "Admin/LlmCallTimeRangeFilter",
	component: TimeRangeFilterStory,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
		viewport: {
			options: TIME_RANGE_VIEWPORTS,
		},
	},
	decorators: [
		(Story) => (
			<div className="bg-slate-900 -m-4 min-h-screen p-12">
				<div className="min-h-56 rounded-md border bg-background p-6">
					<Story />
				</div>
			</div>
		),
	],
} satisfies Meta<typeof TimeRangeFilterStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CombinedRanges: Story = {
	args: {
		started: {
			from: "2026-08-16T09:00",
			to: "2026-08-16T12:00",
		},
		finished: {
			from: "2026-08-16T09:05",
			to: "2026-08-16T12:10",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await userEvent.click(
			canvas.getByRole("button", { name: "LLM 结束时间范围" }),
		);
		await expect(
			body.getByRole("group", { name: "LLM 结束时间后日历" }),
		).toBeVisible();
		await expect(
			body.getByRole("group", { name: "LLM 结束时间前（不含）日历" }),
		).toBeVisible();
		await expect(
			body.getByRole("textbox", { name: "LLM 结束时间后" }),
		).toHaveValue("2026/08/16 09:05");
		await expect(
			body.getByRole("textbox", { name: "LLM 结束时间前（不含）" }),
		).toHaveValue("2026/08/16 12:10");
		await userEvent.click(
			body.getByRole("textbox", { name: "LLM 结束时间后" }),
		);
		await userEvent.click(
			body.getByRole("button", { name: "结束时间后 2026年8月17日" }),
		);
		await expect(
			body.getByRole("textbox", { name: "LLM 结束时间后" }),
		).toHaveValue("2026/08/17 09:05");
		await userEvent.click(
			canvas.getByRole("button", { name: "LLM 开始时间范围" }),
		);
		await expect(
			body.getByRole("textbox", { name: "LLM 开始时间后" }),
		).toHaveValue("2026/08/16 09:00");
		await expect(
			body.getByRole("textbox", { name: "LLM 开始时间前" }),
		).toHaveValue("2026/08/16 12:00");
	},
};

export const EmptyRange: Story = {
	args: {
		started: {
			from: "",
			to: "",
		},
		finished: {
			from: "",
			to: "",
		},
	},
};

export const MobileDrawer: Story = {
	args: {
		started: {
			from: "2026-08-16T09:00",
			to: "2026-08-16T12:00",
		},
		finished: {
			from: "2026-08-16T09:05",
			to: "2026-08-16T12:10",
		},
	},
	globals: {
		viewport: {
			value: "llmTimeRangeMobile393",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await userEvent.click(
			canvas.getByRole("button", { name: "LLM 开始时间范围" }),
		);
		const drawer = body.getByRole("dialog", {
			name: "LLM 开始时间范围设置",
		});
		const drawerContent = within(drawer);
		await expect(drawer).toBeVisible();
		await expect(drawer).toHaveAttribute("data-slot", "sheet-content");
		await expect(
			drawerContent.getByRole("group", { name: "LLM 开始时间后日历" }),
		).toBeVisible();
		expect(
			drawerContent.queryAllByRole("group", { name: "LLM 开始时间前日历" }),
		).toHaveLength(0);
		await userEvent.click(
			drawerContent.getByRole("textbox", { name: "LLM 开始时间前" }),
		);
		expect(
			drawerContent.queryAllByRole("group", { name: "LLM 开始时间后日历" }),
		).toHaveLength(0);
		await expect(
			drawerContent.getByRole("group", { name: "LLM 开始时间前日历" }),
		).toBeVisible();
		const scrollRegion = drawerContent.getByTestId(
			"llm-time-range-drawer-scroll",
		);
		expect(scrollRegion.scrollHeight).toBeLessThanOrEqual(
			scrollRegion.clientHeight + 1,
		);
	},
};
