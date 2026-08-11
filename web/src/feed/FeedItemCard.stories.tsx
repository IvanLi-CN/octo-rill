import type { Meta, StoryObj } from "@storybook/react-vite";
import { expect, within } from "storybook/test";

import type { DashboardScope } from "@/dashboard/routeState";
import { FeedItemCard } from "@/feed/FeedItemCard";
import type { AnnouncementFeedItem, FeedLane, FeedViewer } from "@/feed/types";
import type { DashboardTab } from "@/pages/DashboardControlBand";

const STORYBOOK_VIEWER: FeedViewer = {
	login: "story-viewer",
	avatar_url: "https://github.com/story-viewer.png?size=96",
	html_url: "https://github.com/story-viewer",
};

function buildAnnouncementItem(
	overrides?: Partial<AnnouncementFeedItem>,
): AnnouncementFeedItem {
	return {
		kind: "announcement",
		ts: "2026-07-09T12:00:00Z",
		id: "announcement-story-64",
		repo_full_name: "acme/rocket",
		repo_visual: {
			owner_avatar_url: "https://github.com/acme.png?size=96",
			open_graph_image_url: null,
			uses_custom_open_graph_image: false,
		},
		title: "路线图公告：信息流语义订正",
		body: [
			"- 公告卡与 release 卡共享原文 / 翻译 / 润色三 lane",
			"- 标题走站内 discussion 详情页，右上角保留 GitHub 外跳",
		].join("\n"),
		body_truncated: false,
		subtitle: "仓库公告",
		reason: null,
		subject_type: "discussion",
		discussion_number: 64,
		discussion_key: "acme/rocket#64",
		html_url: "https://github.com/acme/rocket/discussions/64",
		unread: null,
		actor: {
			login: "maintainer",
			avatar_url: "https://github.com/maintainer.png?size=96",
			html_url: "https://github.com/maintainer",
		},
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: "路线图公告：信息流语义校正",
			summary: "- 译文 lane 已就绪\n- Discussion 标题保持站内跳转",
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "路线图公告：阅读流对齐",
			summary: "- 公告与 release 统一三 lane 阅读模型\n- 详情页默认打开润色版",
		},
		reactions: null,
		...overrides,
	};
}

function FeedItemCardPreview(props: {
	item?: AnnouncementFeedItem;
	activeLane?: FeedLane;
	sourceTab?: DashboardTab | null;
	currentScope?: DashboardScope | null;
	isTranslating?: boolean;
	isTranslationAutoRetrying?: boolean;
	isSmartGenerating?: boolean;
	isSmartAutoRetrying?: boolean;
}) {
	const {
		item = buildAnnouncementItem(),
		activeLane = "original",
		sourceTab = "all",
		currentScope = null,
		isTranslating = false,
		isTranslationAutoRetrying = false,
		isSmartGenerating = false,
		isSmartAutoRetrying = false,
	} = props;

	return (
		<div className="bg-background min-h-screen px-4 py-8">
			<div className="mx-auto max-w-3xl">
				<FeedItemCard
					item={item}
					currentViewer={STORYBOOK_VIEWER}
					activeLane={activeLane}
					sourceTab={sourceTab}
					currentScope={currentScope}
					isTranslating={isTranslating}
					isTranslationAutoRetrying={isTranslationAutoRetrying}
					isSmartGenerating={isSmartGenerating}
					isSmartAutoRetrying={isSmartAutoRetrying}
					isReactionBusy={false}
					reactionError={null}
					onSelectLane={() => {}}
					onTranslateNow={() => {}}
					onSmartNow={() => {}}
					onToggleReaction={() => {}}
				/>
			</div>
		</div>
	);
}

const meta = {
	title: "Feed/FeedItemCard",
	component: FeedItemCardPreview,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				component:
					"公告卡沿用 release 内容卡阅读语汇，但标题跳转到站内 discussion 详情页；这组 stories 固定公告 lane 的 missing / error / pending 与 deep link 合同。",
			},
		},
	},
} satisfies Meta<typeof FeedItemCardPreview>;

export default meta;

type Story = StoryObj<typeof meta>;

export const AnnouncementTranslatedMissing: Story = {
	render: () => (
		<FeedItemCardPreview
			activeLane="translated"
			item={buildAnnouncementItem({
				translated: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"公告翻译 lane 缺数据时，继续回退到原文内容卡，不让空态把正文打断。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "路线图公告：信息流语义订正" }),
		).toBeVisible();
		await expect(
			canvas.getByText("公告卡与 release 卡共享原文 / 翻译 / 润色三 lane"),
		).toBeVisible();
	},
};

export const AnnouncementTranslatedError: Story = {
	render: () => (
		<FeedItemCardPreview
			activeLane="translated"
			item={buildAnnouncementItem({
				translated: {
					lang: "zh-CN",
					status: "error",
					title: null,
					summary: null,
					error_code: "upstream_timeout",
					error_summary: "翻译服务暂时超时",
					error_detail: "upstream gateway timeout",
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story: "公告翻译失败时沿用 release card 的紧凑错误面与立即重试入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(canvas.getByText("翻译失败", { exact: true })).toBeVisible();
		await expect(
			canvas.getByText("翻译服务暂时超时", { exact: true }),
		).toBeVisible();
		await expect(
			canvas.getByRole("button", { name: "重试翻译" }),
		).toBeVisible();
	},
};

export const AnnouncementSmartPending: Story = {
	render: () => (
		<FeedItemCardPreview
			activeLane="smart"
			isSmartGenerating
			item={buildAnnouncementItem({
				smart: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"公告润色 lane 正在前台生成时，正文继续显示原文，loading 仅通过 lane trigger 的呼吸态表达。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "路线图公告：信息流语义订正" }),
		).toBeVisible();
		const smartTrigger = canvasElement.querySelector(
			'[data-feed-lane-trigger="smart"][data-feed-lane-loading="true"]',
		);
		if (!(smartTrigger instanceof HTMLElement)) {
			throw new Error("Expected a loading smart lane trigger");
		}
		expect(smartTrigger).toHaveClass("ring-2");
		expect(smartTrigger).not.toHaveClass("animate-pulse");
		const smartIcon = smartTrigger.querySelector("svg");
		expect(smartIcon).not.toBeNull();
		expect(smartIcon?.parentElement).toHaveClass("motion-safe:animate-pulse");
	},
};

export const AnnouncementTranslatedPendingInactive: Story = {
	render: () => (
		<FeedItemCardPreview
			activeLane="original"
			isTranslating
			item={buildAnnouncementItem({
				translated: {
					lang: "zh-CN",
					status: "missing",
					title: null,
					summary: null,
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"当前仍在查看原文时，后台翻译等待只让翻译图标呼吸，不为未选中 lane 增加外框。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByRole("heading", { name: "路线图公告：信息流语义订正" }),
		).toBeVisible();
		const translatedTrigger = canvasElement.querySelector(
			'[data-feed-lane-trigger="translated"][data-feed-lane-loading="true"]',
		);
		if (!(translatedTrigger instanceof HTMLElement)) {
			throw new Error("Expected a loading translated lane trigger");
		}
		expect(translatedTrigger).not.toHaveClass("animate-pulse");
		expect(translatedTrigger).not.toHaveClass("ring-1");
		expect(translatedTrigger).not.toHaveClass("ring-2");
		const translatedIcon = translatedTrigger.querySelector("svg");
		expect(translatedIcon).not.toBeNull();
		expect(translatedIcon?.parentElement).toHaveClass(
			"motion-safe:animate-pulse",
		);
	},
};

export const AnnouncementTitleDeepLink: Story = {
	render: () => (
		<FeedItemCardPreview activeLane="original" currentScope={null} />
	),
	parameters: {
		docs: {
			description: {
				story:
					"从全局 `全部` feed 打开公告标题时，只写 canonical discussion path 与 `from=all`，不再伪造 repo scope。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const titleLink = canvas.getByRole("link", {
			name: "路线图公告：信息流语义订正",
		});
		await expect(titleLink).toHaveAttribute(
			"href",
			"/acme/rocket/discussions/64?from=all",
		);
	},
};
