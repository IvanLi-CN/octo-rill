import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, fireEvent, fn, userEvent, within } from "storybook/test";
import { INITIAL_VIEWPORTS } from "storybook/viewport";

import { AdminCollectionActivity } from "@/admin/AdminCollectionActivity";
import type {
	AdminCollectionActivityBucket,
	AdminCollectionActivityCell,
	AdminCollectionActivityResponse,
} from "@/api";

const activityStart = new Date("2026-09-20T00:00:00Z");

function makeCell(
	id: string,
	status: AdminCollectionActivityCell["composite_status"],
	index: number,
): AdminCollectionActivityCell {
	const hour = new Date(activityStart.getTime() + (11 - index) * 3_600_000);
	return {
		id,
		title:
			index === 0
				? "Bun v1.4.2 adds the new permission-safe installation flow"
				: `Activity item ${id}`,
		repository: index === 2 ? null : "oven-sh/bun",
		source_time: new Date(hour.getTime() + 15 * 60_000).toISOString(),
		translation_status:
			index === 0 || index === 2
				? "running"
				: index === 1
					? "succeeded"
					: "historical_unknown",
		polish_status:
			status === "exception"
				? "failed"
				: status === "processing"
					? "queued"
					: status === "completed"
						? "not_applicable"
						: "legacy_cached",
		composite_status: status,
	};
}

function makeBuckets(
	cells: AdminCollectionActivityCell[] = [],
): AdminCollectionActivityBucket[] {
	return Array.from({ length: 12 }, (_, index) => {
		const startedAt = new Date(
			activityStart.getTime() + (11 - index) * 3_600_000,
		);
		return {
			started_at: startedAt.toISOString(),
			ended_at: new Date(startedAt.getTime() + 3_600_000).toISOString(),
			cells: cells.filter((cell) => {
				const hour = new Date(cell.source_time).getUTCHours();
				return hour === startedAt.getUTCHours();
			}),
		};
	});
}

function overviewFixture(): AdminCollectionActivityResponse {
	const cells = [
		makeCell("release-104", "exception", 0),
		makeCell("announcement-42", "processing", 1),
		makeCell("brief-2026-09-20", "completed", 2),
		makeCell("notification-32", "neutral", 3),
	];
	return {
		kind: "release",
		bucket_minutes: 60,
		bucket_count: 12,
		window_started_at: activityStart.toISOString(),
		window_ended_at: "2026-09-20T12:00:00Z",
		summary: {
			content_count: 4,
			completed_count: 1,
			processing_count: 1,
			exception_count: 1,
			neutral_count: 1,
		},
		buckets: makeBuckets(cells),
	};
}

function denseFixture(): AdminCollectionActivityResponse {
	const buckets = makeBuckets();
	let completedCount = 0;
	let processingCount = 0;
	let exceptionCount = 0;
	for (let index = 0; index < 8_001; index += 1) {
		const bucketIndex = index % 12;
		const status =
			index % 31 === 0
				? "exception"
				: index % 7 === 0
					? "processing"
					: "completed";
		if (status === "exception") exceptionCount += 1;
		else if (status === "processing") processingCount += 1;
		else completedCount += 1;
		buckets[bucketIndex].cells.push(
			makeCell(`dense-${index}`, status, bucketIndex),
		);
	}
	return {
		...overviewFixture(),
		summary: {
			content_count: 8_001,
			completed_count: completedCount,
			processing_count: processingCount,
			exception_count: exceptionCount,
			neutral_count: 0,
		},
		buckets,
	};
}

const activity = overviewFixture();

const meta = {
	title: "Admin/AdminCollectionActivity",
	component: AdminCollectionActivity,
	tags: ["autodocs", "admin-collection-activity"],
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: {
				...INITIAL_VIEWPORTS,
				adminActivityDesktop1440: {
					name: "Admin activity desktop 1440x1000",
					styles: { width: "1440px", height: "1000px" },
				},
				adminMobile: {
					name: "Admin mobile",
					styles: { width: "393px", height: "852px" },
				},
			},
		},
		docs: {
			description: {
				component:
					"按采集来源时间展示最近十二个 UTC 自然小时内的内容处理统计与逐条活动格。",
			},
		},
	},
	globals: {
		viewport: { value: "adminActivityDesktop1440" },
	},
	decorators: [
		(Story) => (
			<div
				data-visual-evidence-surface
				className="mx-auto w-full max-w-[1072px] bg-background p-6"
			>
				<div data-visual-evidence-target>
					<Story />
				</div>
			</div>
		),
	],
	args: {
		data: activity,
		loading: false,
		error: null,
		onRetry: fn(),
		onOpenRecord: fn(),
	},
} satisfies Meta<typeof AdminCollectionActivity>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CurrentWindowOverview: Story = {
	play: async ({ canvasElement, args }) => {
		const canvas = within(canvasElement);
		const firstCell = canvas.getByRole("button", {
			name: /Bun v1\.4\.2/,
		});
		const target = firstCell.getBoundingClientRect();
		await expect(target.width).toBeGreaterThanOrEqual(24);
		await expect(target.height).toBeGreaterThanOrEqual(24);
		await userEvent.hover(firstCell);
		await expect(canvas.getByRole("tooltip")).toHaveTextContent("oven-sh/bun");
		await userEvent.click(firstCell);
		await expect(args.onOpenRecord).toHaveBeenCalledWith(
			"release",
			"release-104",
		);
	},
};

export const MobileOverview: Story = {
	globals: {
		viewport: { value: "adminMobile" },
	},
	parameters: {
		viewport: {
			options: meta.parameters.viewport.options,
		},
	},
};

export const EmptyBrief: Story = {
	args: {
		data: {
			...activity,
			kind: "brief",
			buckets: makeBuckets(),
			summary: {
				content_count: 0,
				completed_count: 0,
				processing_count: 0,
				exception_count: 0,
				neutral_count: 0,
			},
		},
	},
};

export const ReadFailure: Story = {
	args: { data: null, error: "读取活动数据失败，请稍后重试。" },
	play: async ({ canvasElement, args }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "最近 12 小时活动暂不可用" }),
		).toBeVisible();
		await userEvent.click(canvas.getByRole("button", { name: "重试" }));
		await expect(args.onRetry).toHaveBeenCalledOnce();
	},
};

export const CachedDataAfterReadFailure: Story = {
	args: {
		data: activity,
		error: "读取服务正忙，请稍后重试。",
	},
	play: async ({ canvasElement, args }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("status")).toHaveTextContent(
			"继续显示上次读取的数据",
		);
		await userEvent.click(canvas.getByRole("button", { name: "重试" }));
		await expect(args.onRetry).toHaveBeenCalledOnce();
	},
};

export const DenseCanvas: Story = {
	args: { data: denseFixture() },
	play: async ({ canvasElement, args }) => {
		const canvas = within(canvasElement);
		const grid = canvas.getByRole("grid", { name: "最近十二小时内容活动" });
		const scroll = grid.parentElement;
		const drawingSurface = grid.querySelector("canvas");
		await expect(
			canvas.getByTestId("collection-activity-canvas-grid"),
		).toBeVisible();
		if (!scroll || !drawingSurface)
			throw new Error("Expected the virtualized activity canvas");

		const rect = drawingSurface.getBoundingClientRect();
		fireEvent.click(drawingSurface, {
			clientX: rect.left + 95,
			clientY: rect.top + 28,
		});
		await expect(args.onOpenRecord).not.toHaveBeenCalled();

		const columns = Math.max(1, Math.floor((rect.width - 70 - 8) / 27));
		const firstHourCellCount = Math.ceil(8_001 / 12);
		const firstHourHeight =
			23 + Math.ceil(firstHourCellCount / columns) * 27 + 8;
		scroll.scrollTop = firstHourHeight;
		fireEvent.scroll(scroll);
		fireEvent.pointerMove(drawingSurface, {
			clientX: rect.left + 75,
			clientY: rect.top + 28,
		});
		await expect(grid).toHaveAttribute(
			"aria-activedescendant",
			"collection-activity-grid-cell-667",
		);
		grid.focus();
		await userEvent.keyboard("{ArrowUp}");
		const previousHourLastLine =
			Math.floor((firstHourCellCount - 1) / columns) * columns;
		await expect(grid).toHaveAttribute(
			"aria-activedescendant",
			`collection-activity-grid-cell-${previousHourLastLine}`,
		);
		await userEvent.keyboard("{ArrowDown}");
		await expect(grid).toHaveAttribute(
			"aria-activedescendant",
			"collection-activity-grid-cell-667",
		);
		grid.focus();
		await userEvent.keyboard("{Enter}");
		await expect(args.onOpenRecord).toHaveBeenCalledWith("release", "dense-1");
		await userEvent.keyboard("{Escape}");
		scroll.scrollTop = 0;
		fireEvent.scroll(scroll);
		grid.blur();
	},
};
