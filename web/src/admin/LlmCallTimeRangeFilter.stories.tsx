import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import {
	LlmCallTimeRangeFilter,
	type LlmCallTimeRangeValue,
} from "@/admin/LlmCallTimeRangeFilter";

function TimeRangeFilterStory(props: { value: LlmCallTimeRangeValue }) {
	const [value, setValue] = useState(props.value);
	return <LlmCallTimeRangeFilter value={value} onValueChange={setValue} />;
}

const meta = {
	title: "Admin/LlmCallTimeRangeFilter",
	component: TimeRangeFilterStory,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
	},
	decorators: [
		(Story) => (
			<div className="bg-muted mx-auto min-h-56 w-full max-w-2xl rounded-md border p-6">
				<Story />
			</div>
		),
	],
} satisfies Meta<typeof TimeRangeFilterStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CombinedRanges: Story = {
	args: {
		value: {
			startedFrom: "2026-08-16T09:00",
			startedTo: "2026-08-16T12:00",
			finishedFrom: "2026-08-16T09:05",
			finishedBefore: "2026-08-16T12:10",
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await userEvent.click(
			canvas.getByRole("button", { name: "LLM 调用时间范围" }),
		);
		await expect(body.getByLabelText("LLM 开始时间后")).toHaveValue(
			"2026-08-16T09:00",
		);
		await expect(body.getByLabelText("LLM 结束时间前（不含）")).toHaveValue(
			"2026-08-16T12:10",
		);
	},
};

export const EmptyRange: Story = {
	args: {
		value: {
			startedFrom: "",
			startedTo: "",
			finishedFrom: "",
			finishedBefore: "",
		},
	},
};
