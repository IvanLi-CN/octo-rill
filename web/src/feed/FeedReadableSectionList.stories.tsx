import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, userEvent, waitFor, within } from "storybook/test";
import { useEffect, useState } from "react";

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

const appendFailure: ReadableSectionsError = {
	phase: "append",
	message: "更多可读动态加载失败，请稍后重试。",
	kind: "network",
	detail: null,
	at: 0,
};

const feedbackLoopDelayMs = 2_400;
const loadingChipWidth = 56;

type FeedbackLoopPhase = "load-more" | "loading" | "error";

const feedbackLoopPhases: FeedbackLoopPhase[] = [
	"load-more",
	"loading",
	"error",
];

function Preview(props: {
	loading?: boolean;
	mainLoading?: boolean;
	mainHasMore?: boolean;
	autoLoadMore?: boolean;
	withDetails?: boolean;
	error?: ReadableSectionsError | null;
	onLoadMore?: () => void;
}) {
	const {
		loading = false,
		mainLoading = false,
		mainHasMore = mainLoading,
		autoLoadMore = true,
		withDetails = false,
		error = null,
		onLoadMore = () => {},
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
		<div
			className="mx-auto w-full max-w-3xl bg-card p-4 text-foreground sm:p-6"
			data-visual-evidence-surface="true"
		>
			<div
				className="w-full"
				data-readable-evidence-surface="true"
				data-visual-evidence-target="true"
			>
				<FeedReadableSectionList
					sections={[briefSection]}
					details={details}
					error={error}
					loadingInitial={false}
					loadingMore={mainLoading}
					hasMore={mainHasMore}
					autoLoadMore={autoLoadMore}
					onLoadMore={onLoadMore}
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

function LoadingErrorLoadingLoopPreview() {
	const [phase, setPhase] = useState<"loading" | "error">("loading");

	useEffect(() => {
		const timeout = window.setTimeout(() => {
			setPhase((current) => (current === "loading" ? "error" : "loading"));
		}, feedbackLoopDelayMs);
		return () => window.clearTimeout(timeout);
	}, [phase]);

	return (
		<div
			data-feed-pagination-cycle="true"
			data-feed-pagination-cycle-phase={phase}
		>
			<Preview
				mainLoading={phase === "loading"}
				error={phase === "error" ? appendFailure : null}
			/>
		</div>
	);
}

function LoadMoreLoadingErrorLoopPreview() {
	const [phase, setPhase] = useState<FeedbackLoopPhase>("load-more");

	useEffect(() => {
		const timeout = window.setTimeout(() => {
			setPhase((current) => {
				const currentIndex = feedbackLoopPhases.indexOf(current);
				return feedbackLoopPhases[
					(currentIndex + 1) % feedbackLoopPhases.length
				];
			});
		}, feedbackLoopDelayMs);
		return () => window.clearTimeout(timeout);
	}, [phase]);

	return (
		<div
			data-feed-pagination-cycle="true"
			data-feed-pagination-cycle-phase={phase}
		>
			<Preview
				mainHasMore
				mainLoading={phase === "loading"}
				autoLoadMore={false}
				error={phase === "error" ? appendFailure : null}
				onLoadMore={() => setPhase("loading")}
			/>
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
	render: () => <Preview />,
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
		await expect(
			canvas.getByRole("status").getAttribute("data-feed-pagination-chip"),
		).toBe("true");
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
		await expect(
			canvas.getByRole("status").getAttribute("data-feed-pagination-chip"),
		).toBe("true");
	},
};

export const AppendFailureRetry: Story = {
	render: () => <Preview error={appendFailure} />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const retry = canvas.getByRole("button", { name: /重试加载/ });
		await expect(retry).toBeVisible();
		await expect(retry.getAttribute("data-feed-pagination-error-chip")).toBe(
			"true",
		);
		await expect(canvas.queryByText(/已到尽头/)).not.toBeInTheDocument();
	},
};

export const LoadingErrorLoadingLoop: Story = {
	tags: ["feed-pagination-feedback"],
	render: () => <LoadingErrorLoadingLoopPreview />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const feedbackChip = () => {
			const chip = canvasElement.querySelector<HTMLElement>(
				"[data-feed-pagination-chip='true']",
			);
			if (!chip) throw new Error("Expected pagination feedback chip");
			return chip;
		};
		const feedbackSlot = canvasElement.querySelector<HTMLElement>(
			"[data-feed-pagination-feedback='true']",
		);
		if (!feedbackSlot) throw new Error("Expected pagination feedback slot");

		await waitFor(
			() => expect(feedbackChip().dataset.feedPaginationState).toBe("loading"),
			{
				timeout: feedbackLoopDelayMs + 1_000,
			},
		);
		await waitFor(() => expect(canvas.getByRole("status")).toBeVisible(), {
			timeout: feedbackLoopDelayMs + 1_000,
		});
		const loading = canvas.getByRole("status");
		const loadingHeight = loading.offsetHeight;
		const loadingWidth = loading.offsetWidth;
		const slotHeight = feedbackSlot.offsetHeight;
		await expect(loadingHeight).toBe(24);
		await expect(loadingWidth).toBe(56);
		await expect(slotHeight).toBe(24);

		await waitFor(
			() => expect(feedbackChip().dataset.feedPaginationState).toBe("error"),
			{ timeout: feedbackLoopDelayMs + 1_000 },
		);
		await waitFor(
			() =>
				expect(canvas.getByRole("button", { name: /重试加载/ })).toBeVisible(),
			{ timeout: 1_000 },
		);
		const retry = canvas.getByRole("button", { name: /重试加载/ });
		await expect(retry.offsetHeight).toBe(loadingHeight);
		await waitFor(
			() => expect(retry.offsetWidth).toBeGreaterThan(loadingWidth),
			{ timeout: 1_000 },
		);
		await expect(feedbackSlot.offsetHeight).toBe(slotHeight);

		await waitFor(
			() => expect(feedbackChip().dataset.feedPaginationState).toBe("loading"),
			{
				timeout: feedbackLoopDelayMs + 1_000,
			},
		);
		await waitFor(() => expect(canvas.getByRole("status")).toBeVisible(), {
			timeout: feedbackLoopDelayMs + 1_000,
		});
		const reloading = canvas.getByRole("status");
		await expect(reloading.offsetHeight).toBe(loadingHeight);
		await waitFor(() => expect(reloading.offsetWidth).toBe(loadingWidth), {
			timeout: 1_000,
		});
		await expect(feedbackSlot.offsetHeight).toBe(slotHeight);
	},
};

export const LoadMoreLoadingErrorLoop: Story = {
	tags: ["feed-pagination-feedback"],
	render: () => <LoadMoreLoadingErrorLoopPreview />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const feedbackChip = () => {
			const chip = canvasElement.querySelector<HTMLElement>(
				"[data-feed-pagination-chip='true']",
			);
			if (!chip) throw new Error("Expected pagination feedback chip");
			return chip;
		};
		const feedbackSlot = canvasElement.querySelector<HTMLElement>(
			"[data-feed-pagination-feedback='true']",
		);
		if (!feedbackSlot) throw new Error("Expected pagination feedback slot");

		await waitFor(
			() =>
				expect(feedbackChip().dataset.feedPaginationState).toBe("load-more"),
			{ timeout: feedbackLoopDelayMs + 1_000 },
		);
		const loadMore = canvas.getByRole("button", { name: "加载更多" });
		const loadMoreHeight = loadMore.offsetHeight;
		const slotHeight = feedbackSlot.offsetHeight;
		await expect(loadMoreHeight).toBe(24);
		await expect(loadMore.offsetWidth).toBeGreaterThan(loadingChipWidth);
		await expect(slotHeight).toBe(24);

		await userEvent.click(loadMore);
		await waitFor(
			() => expect(feedbackChip().dataset.feedPaginationState).toBe("loading"),
			{ timeout: 1_000 },
		);
		const loading = canvas.getByRole("status");
		await expect(loading.offsetHeight).toBe(loadMoreHeight);
		await waitFor(() => expect(loading.offsetWidth).toBe(loadingChipWidth), {
			timeout: 1_000,
		});
		await expect(feedbackSlot.offsetHeight).toBe(slotHeight);

		await waitFor(
			() => expect(feedbackChip().dataset.feedPaginationState).toBe("error"),
			{ timeout: feedbackLoopDelayMs + 1_000 },
		);
		const retry = canvas.getByRole("button", { name: /重试加载/ });
		await expect(retry.offsetHeight).toBe(loadMoreHeight);
		await waitFor(
			() => expect(retry.offsetWidth).toBeGreaterThan(loadingChipWidth),
			{ timeout: 1_000 },
		);
		await expect(retry.scrollWidth).toBe(retry.clientWidth);
		await expect(feedbackSlot.offsetHeight).toBe(slotHeight);

		await waitFor(
			() =>
				expect(feedbackChip().dataset.feedPaginationState).toBe("load-more"),
			{ timeout: feedbackLoopDelayMs + 1_000 },
		);
		const nextLoadMore = canvas.getByRole("button", { name: "加载更多" });
		await expect(nextLoadMore.offsetHeight).toBe(loadMoreHeight);
		await waitFor(
			() => expect(nextLoadMore.offsetWidth).toBeGreaterThan(loadingChipWidth),
			{
				timeout: 1_000,
			},
		);
		await expect(feedbackSlot.offsetHeight).toBe(slotHeight);
	},
};

export const NarrowViewport: Story = {
	name: "Narrow viewport error chip",
	globals: {
		viewport: { value: "mobile1", isRotated: false },
	},
	render: () => <Preview error={appendFailure} />,
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const retry = canvas.getByRole("button", { name: /重试加载/ });
		await expect(retry).toBeVisible();
		const retryBounds = retry.getBoundingClientRect();
		expect(retryBounds.left).toBeGreaterThanOrEqual(0);
		expect(retryBounds.right).toBeLessThanOrEqual(
			canvasElement.ownerDocument.documentElement.clientWidth,
		);
		expect(
			canvasElement.ownerDocument.documentElement.scrollWidth -
				canvasElement.ownerDocument.documentElement.clientWidth,
		).toBeLessThanOrEqual(1);
	},
};
