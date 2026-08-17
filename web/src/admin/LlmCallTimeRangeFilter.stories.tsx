import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { expect, userEvent, within } from "storybook/test";

import {
	LlmCallTimeRangeFilter,
	type LlmCallTimeRangeValue,
} from "@/admin/LlmCallTimeRangeFilter";

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
		await expect(body.getByLabelText("LLM 结束时间后")).toHaveValue(
			"2026-08-16T09:05",
		);
		await expect(body.getByLabelText("LLM 结束时间前（不含）")).toHaveValue(
			"2026-08-16T12:10",
		);
		await userEvent.click(
			canvas.getByRole("button", { name: "LLM 开始时间范围" }),
		);
		await expect(body.getByLabelText("LLM 开始时间后")).toHaveValue(
			"2026-08-16T09:00",
		);
		await expect(body.getByLabelText("LLM 开始时间前")).toHaveValue(
			"2026-08-16T12:00",
		);
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
