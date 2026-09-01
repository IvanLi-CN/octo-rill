import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, within } from "storybook/test";
import { useState } from "react";

import { FeedReadableSectionList } from "@/feed/FeedReadableSectionList";
import type {
	DashboardReadableSection,
	FeedItem,
	FeedLane,
} from "@/feed/types";
import type {
	ReadableSectionDetails,
	ReadableSectionsError,
} from "@/feed/useDashboardReadableSections";

const item: FeedItem = {
	kind: "release",
	ts: "2026-04-30T10:00:00Z",
	id: "readable-release-1",
	repo_full_name: "IvanLi-CN/octo-rill",
	repo_visual: null,
	title: "v2.58.3",
	body: "- 修复折叠历史分页\n- 保留完整日报正文",
	body_truncated: false,
	subtitle: null,
	reason: null,
	subject_type: null,
	html_url: "https://github.com/IvanLi-CN/octo-rill/releases/tag/v2.58.3",
	unread: null,
	actor: null,
	translated: { lang: "zh-CN", status: "disabled", title: null, summary: null },
	smart: { lang: "zh-CN", status: "disabled", title: null, summary: null },
	reactions: {
		counts: { plus1: 0, laugh: 0, heart: 0, hooray: 0, rocket: 0, eyes: 0 },
		viewer: {
			plus1: false,
			laugh: false,
			heart: false,
			hooray: false,
			rocket: false,
			eyes: false,
		},
		status: "ready",
	},
};

const briefSection: DashboardReadableSection = {
	id: "2026-04-30",
	date: "2026-04-30",
	kind: "brief",
	brief: {
		id: "brief-readable",
		date: "2026-04-30",
		window_start: "2026-04-30T00:00:00Z",
		window_end: "2026-04-30T23:59:59Z",
		effective_time_zone: "UTC",
		effective_local_boundary: "00:00",
		release_count: 1,
		release_ids: [item.id],
		preview_markdown: "不应显示的摘要",
		content_markdown:
			"## 完整日报\n\n这是随区块返回的完整日报正文。\n\n- 已覆盖发布：v2.58.3",
		covers_repo_stars: false,
		covers_followers: false,
		created_at: "2026-04-30T23:00:00Z",
		updated_at: "2026-04-30T23:00:00Z",
	},
	items: [],
	supplemental_items: [
		{ ...item, id: "supplemental-readable", title: "未覆盖补充动态" },
	],
	items_next_cursor: null,
	item_count: 2,
};

function Preview(props: {
	loading?: boolean;
	mainLoading?: boolean;
	withDetails?: boolean;
	error?: ReadableSectionsError | null;
}) {
	const {
		loading = false,
		mainLoading = false,
		withDetails = false,
		error = null,
	} = props;
	const [details, setDetails] = useState<
		Record<string, ReadableSectionDetails>
	>(
		withDetails
			? {
					"2026-04-30": {
						items: [item],
						nextCursor: null,
						loading: false,
						error: null,
					},
				}
			: {},
	);
	const feedCardProps = {
		currentViewer: null,
		sourceTab: "all" as const,
		currentScope: null,
		translationInFlightKeys: new Set<string>(),
		translationAutoRetryingKeys: new Set<string>(),
		smartInFlightKeys: new Set<string>(),
		smartAutoRetryingKeys: new Set<string>(),
		registerItemRef: () => () => {},
		selectedLaneByKey: {} as Record<string, FeedLane>,
		onSelectLane: () => {},
		onTranslateNow: () => {},
		onSmartNow: () => {},
		reactionBusyKeys: new Set<string>(),
		reactionErrorByKey: {},
		onToggleReaction: () => {},
	};
	return (
		<div className="min-h-0 overflow-hidden bg-slate-900 p-8 text-foreground sm:p-10">
			<div
				className="mx-auto max-w-3xl bg-slate-100 p-6 text-slate-900 sm:p-8"
				data-readable-evidence-surface="true"
			>
				<FeedReadableSectionList
					sections={[briefSection]}
					details={details}
					error={error}
					loadingInitial={false}
					loadingMore={mainLoading}
					hasMore={mainLoading}
					onLoadMore={() => {}}
					onRetry={() => {}}
					onLoadSectionItems={(sectionId) => {
						setDetails({
							[sectionId]: {
								items: withDetails ? [item] : [],
								nextCursor: null,
								loading,
								error: null,
							},
						});
					}}
					feedCardProps={feedCardProps}
				/>
			</div>
		</div>
	);
}

const meta = {
	title: "Feed/FeedReadableSectionList",
	component: Preview,
	parameters: { layout: "fullscreen" },
} satisfies Meta<typeof FeedReadableSectionList>;

export default meta;
type Story = StoryObj<typeof meta>;

export const CompleteBriefAndSupplemental: Story = {
	render: () => <Preview withDetails />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("这是随区块返回的完整日报正文。"),
		).toBeVisible();
		await expect(canvas.getByText("未覆盖补充动态")).toBeVisible();
		await expect(canvas.queryByText("不应显示的摘要")).not.toBeInTheDocument();
	},
};

export const ListLoadsCoveredRelease: Story = {
	render: () => <Preview withDetails />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByRole("button", { name: "列表" }));
		await expect(canvas.getByText("v2.58.3")).toBeVisible();
	},
};

export const ListLoadingWaveCapsule: Story = {
	render: () => <Preview loading />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await userEvent.click(canvas.getByRole("button", { name: "列表" }));
		await expect(canvas.getByRole("status")).toHaveAttribute(
			"aria-label",
			"加载中",
		);
		await expect(
			canvas
				.getByRole("status")
				.querySelectorAll("[data-feed-pagination-wave-dot='true']").length,
		).toBe(3);
	},
};

export const MainLoadingWaveCapsule: Story = {
	render: () => <Preview mainLoading />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByRole("status")).toHaveAttribute(
			"aria-label",
			"加载中",
		);
		await expect(
			canvas
				.getByRole("status")
				.querySelectorAll("[data-feed-pagination-wave-dot='true']").length,
		).toBe(3);
	},
};

export const AppendFailureRetry: Story = {
	render: () => (
		<Preview
			error={{
				phase: "append",
				message: "更多可读动态加载失败，请稍后重试。",
				kind: "network",
				detail: null,
				at: 0,
			}}
		/>
	),
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("button", { name: "重试加载" }),
		).toBeVisible();
	},
};

export const NarrowViewport: Story = {
	name: "Narrow viewport",
	globals: {
		viewport: { value: "mobile1", isRotated: false },
	},
	render: () => <Preview withDetails />,
};
