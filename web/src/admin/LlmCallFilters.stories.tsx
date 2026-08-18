import type { Meta, StoryObj } from "@storybook/react-vite";
import { useState } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, waitFor, within } from "storybook/test";

import { LlmCallFilters, type LlmStatusFilter } from "@/admin/LlmCallFilters";
import type { LlmCallTimeRangeValue } from "@/admin/LlmCallTimeRangeFilter";

const FILTER_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	llmFiltersMobile393: {
		name: "LLM filters mobile 393x852",
		styles: { width: "393px", height: "852px" },
		type: "mobile",
	},
} as const;

function LlmCallFiltersStory() {
	const [status, setStatus] = useState<LlmStatusFilter>("failed");
	const [model, setModel] = useState("gpt-5-mini");
	const [source, setSource] = useState("");
	const [requestedBy, setRequestedBy] = useState("");
	const [started, setStarted] = useState<LlmCallTimeRangeValue>({
		from: "",
		to: "",
	});
	const [finished, setFinished] = useState<LlmCallTimeRangeValue>({
		from: "2026-08-16T09:00",
		to: "2026-08-16T12:00",
	});
	const hasFilters =
		status !== "all" ||
		model !== "" ||
		source !== "" ||
		requestedBy !== "" ||
		started.from !== "" ||
		started.to !== "" ||
		finished.from !== "" ||
		finished.to !== "";

	return (
		<LlmCallFilters
			status={status}
			onStatusChange={setStatus}
			model={model}
			onModelChange={setModel}
			source={source}
			onSourceChange={setSource}
			requestedBy={requestedBy}
			onRequestedByChange={setRequestedBy}
			started={started}
			onStartedChange={setStarted}
			finished={finished}
			onFinishedChange={setFinished}
			hasFilters={hasFilters}
			onClear={() => {
				setStatus("all");
				setModel("");
				setSource("");
				setRequestedBy("");
				setStarted({ from: "", to: "" });
				setFinished({ from: "", to: "" });
			}}
		/>
	);
}

const meta = {
	title: "Admin/LlmCallFilters",
	component: LlmCallFiltersStory,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
		viewport: {
			options: FILTER_VIEWPORTS,
		},
	},
	decorators: [
		(Story) => (
			<div className="bg-slate-900 -m-4 min-h-screen p-8">
				<div className="rounded-md border bg-background p-4">
					<Story />
				</div>
			</div>
		),
	],
} satisfies Meta<typeof LlmCallFiltersStory>;

export default meta;
type Story = StoryObj<typeof meta>;

export const DesktopToolbar: Story = {};

export const MobileFiltersOpen: Story = {
	globals: {
		viewport: {
			value: "llmFiltersMobile393",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await userEvent.click(
			canvas.getByRole("button", { name: "打开 LLM 调用筛选" }),
		);
		const drawer = body.getByRole("dialog", { name: "筛选调用记录" });
		await expect(drawer).toBeVisible();
		await expect(drawer).toHaveClass(/right-0/);
		await expect(
			within(drawer).getByRole("textbox", { name: "LLM 调用模型筛选" }),
		).toHaveValue("gpt-5-mini");
	},
};

export const MobileTimePanelInFilterDrawer: Story = {
	globals: {
		viewport: {
			value: "llmFiltersMobile393",
			isRotated: false,
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const body = within(canvasElement.ownerDocument.body);
		await userEvent.click(
			canvas.getByRole("button", { name: "打开 LLM 调用筛选" }),
		);
		const filters = body.getByRole("dialog", { name: "筛选调用记录" });
		await userEvent.click(
			within(filters).getByRole("button", { name: "LLM 开始时间范围" }),
		);
		await waitFor(() => {
			expect(
				within(filters).getByRole("region", {
					name: "LLM 开始时间范围设置",
				}),
			).toBeVisible();
		});
		await expect(filters).toBeVisible();
		expect(body.getAllByRole("dialog")).toHaveLength(1);
		await userEvent.click(
			within(filters).getByRole("button", { name: "返回筛选条件" }),
		);
		expect(
			within(filters).queryByRole("region", {
				name: "LLM 开始时间范围设置",
			}),
		).not.toBeInTheDocument();
		await expect(
			within(filters).getByRole("textbox", { name: "LLM 调用模型筛选" }),
		).toBeVisible();
	},
};
