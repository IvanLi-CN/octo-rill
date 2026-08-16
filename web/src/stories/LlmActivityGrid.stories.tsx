import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";

import { LlmActivityGrid } from "@/admin/LlmActivityGrid";
import type { AdminLlmActivityResponse } from "@/api";

const models = [
	{ model: "gpt-5-mini", priority: 1, configured: true },
	{ model: "gpt-4.1-mini", priority: 2, configured: true },
	{ model: "retired-model", priority: null, configured: false },
];

function activityFixture(): AdminLlmActivityResponse {
	const start = new Date("2026-08-13T09:00:00Z");
	return {
		bucket_minutes: 60,
		bucket_count: 50,
		window_started_at: start.toISOString(),
		window_ended_at: new Date(start.getTime() + 50 * 3_600_000).toISOString(),
		models,
		buckets: Array.from({ length: 50 }, (_, index) => {
			const startedAt = new Date(start.getTime() + index * 3_600_000);
			return {
				started_at: startedAt.toISOString(),
				ended_at: new Date(startedAt.getTime() + 3_600_000).toISOString(),
				counts: [
					{
						model: "gpt-5-mini",
						succeeded: index === 49 ? 8 : index % 4 === 0 ? 4 : 1,
						failed: index === 49 ? 2 : 0,
					},
					{
						model: "gpt-4.1-mini",
						succeeded: index === 49 ? 2 : index % 3 === 0 ? 2 : 0,
						failed: index === 49 ? 1 : 0,
					},
					{
						model: "retired-model",
						succeeded: index === 4 ? 1 : 0,
						failed: 0,
					},
				],
			};
		}),
	};
}

const fixture = activityFixture();

const meta = {
	title: "Admin/LlmActivityGrid",
	component: LlmActivityGrid,
	tags: ["autodocs"],
	parameters: {
		layout: "padded",
		docs: {
			description: {
				component:
					"管理员 LLM 调度的固定 50 小时逐模型活动网格，覆盖列聚合、键盘漫游和独立刷新状态。",
			},
		},
	},
	args: {
		data: fixture,
		loading: false,
		refreshing: false,
		error: null,
	},
} satisfies Meta<typeof LlmActivityGrid>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const page = within(canvasElement.ownerDocument.body);
		const latestCell = canvas.getByRole("button", {
			name: /gpt-5-mini，成功 8，失败 2/,
		});
		await userEvent.hover(latestCell);
		const summary = page.getByTestId("llm-activity-summary");
		await expect(summary).toHaveTextContent("80%");
		await expect(summary).toHaveTextContent("2");
		const summaryRect = summary.getBoundingClientRect();
		canvasElement.ownerDocument.dispatchEvent(
			new PointerEvent("pointermove", {
				bubbles: true,
				clientX: summaryRect.left + 12,
				clientY: summaryRect.top + 12,
			}),
		);
		await expect(summary).toBeVisible();
		await userEvent.click(latestCell);
		await userEvent.keyboard("{ArrowLeft}");
		await expect(page.getByTestId("llm-activity-summary")).toBeVisible();
		await userEvent.keyboard("{Escape}");
		await expect(
			page.queryByTestId("llm-activity-summary"),
		).not.toBeInTheDocument();
	},
};

export const Loading: Story = {
	args: { data: null, loading: true },
};

export const Empty: Story = {
	args: {
		data: {
			...fixture,
			models: [],
			buckets: fixture.buckets.map((bucket) => ({ ...bucket, counts: [] })),
		},
	},
};

export const ErrorState: Story = {
	args: {
		data: null,
		error: "当前无法读取模型活动。",
		onRetry: () => undefined,
	},
};

export const BackgroundRefresh: Story = {
	args: {
		refreshing: true,
		error: "上一次后台更新失败，正在显示旧数据。",
	},
};
