import type { Meta, StoryObj } from "@storybook/react-vite";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import type { DashboardScope } from "@/dashboard/routeState";
import { FeedItemCard } from "@/feed/FeedItemCard";
import type {
	AnnouncementFeedItem,
	FeedLane,
	FeedViewer,
	ReleaseFeedItem,
} from "@/feed/types";
import type { DashboardTab } from "@/pages/DashboardControlBand";

type ReadableFeedItem = AnnouncementFeedItem | ReleaseFeedItem;

const STORYBOOK_VIEWER: FeedViewer = {
	login: "story-viewer",
	avatar_url: "https://github.com/story-viewer.png?size=96",
	html_url: "https://github.com/story-viewer",
};

const FEED_ITEM_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	feedItemDesktop923: {
		name: "Feed item desktop 923x633",
		styles: { width: "923px", height: "633px" },
		type: "desktop",
	},
	feedItemMobile393: {
		name: "Feed item mobile 393x852",
		styles: { width: "393px", height: "852px" },
		type: "mobile",
	},
} as const;

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

function buildReleaseItem(): ReleaseFeedItem {
	return {
		kind: "release",
		ts: "2026-09-26T14:21:38Z",
		id: "release-story-159",
		repo_full_name: "openai/codex",
		repo_visual: null,
		title: "0.159.0-alpha.4",
		body: "0.159.0-alpha.4 版本发布",
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: "https://github.com/openai/codex/releases/tag/0.159.0-alpha.4",
		unread: null,
		actor: null,
		translated: null,
		smart: null,
		reactions: null,
	};
}

function FeedItemCardPreview(props: {
	item?: ReadableFeedItem;
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
		<div
			className="bg-background mx-auto w-full max-w-[816px] px-6 py-8"
			data-visual-evidence-surface
		>
			<div className="w-full max-w-3xl" data-visual-evidence-target>
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
	tags: ["autodocs", "feed-blocked-config"],
	parameters: {
		layout: "fullscreen",
		viewport: { options: FEED_ITEM_VIEWPORTS },
		docs: {
			description: {
				component:
					"Release 与公告卡共享内容卡阅读语汇；Release 类型图标紧邻仓库名，公告标题跳转到站内 discussion 详情页。",
			},
		},
	},
} satisfies Meta<typeof FeedItemCardPreview>;

export default meta;

type Story = StoryObj<typeof meta>;

export const ReleaseTypeIconAligned: Story = {
	render: () => <FeedItemCardPreview item={buildReleaseItem()} />,
	globals: {
		theme: "dark",
		viewport: { value: "feedItemDesktop923" },
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const label = canvasElement.querySelector<HTMLElement>(
			'[data-repo-identity-label="true"]',
		);
		const icon = canvasElement.querySelector<HTMLElement>(
			'[data-feed-item-type-icon="release"]',
		);

		await expect(canvas.getByText("openai/codex")).toBeVisible();
		await expect(icon).toBeVisible();
		if (!label || !icon) {
			throw new Error("Release type icon or repository name is missing.");
		}

		const labelRect = label.getBoundingClientRect();
		const iconRect = icon.getBoundingClientRect();
		expect(iconRect.left - labelRect.right).toBeGreaterThanOrEqual(0);
		expect(iconRect.left - labelRect.right).toBeLessThanOrEqual(8);
		expect(
			Math.abs(
				iconRect.top +
					iconRect.height / 2 -
					labelRect.top -
					labelRect.height / 2,
			),
		).toBeLessThanOrEqual(1);
	},
};

export const ReleaseTypeIconHiddenOnMobile: Story = {
	render: () => <FeedItemCardPreview item={buildReleaseItem()} />,
	globals: {
		theme: "dark",
		viewport: { value: "feedItemMobile393" },
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		const icon = canvasElement.querySelector<HTMLElement>(
			'[data-feed-item-type-icon="release"]',
		);
		await expect(canvas.getByText("openai/codex")).toBeVisible();
		await expect(icon).not.toBeVisible();
	},
};

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

export const AnnouncementTranslatedBlockedConfig: Story = {
	render: () => (
		<FeedItemCardPreview
			activeLane="translated"
			item={buildAnnouncementItem({
				translated: {
					lang: "zh-CN",
					status: "blocked_config",
					title: "保留的译文标题",
					summary: "已有有效译文仍然可读。",
					error_code: "configuration",
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"模型配置恢复前显示可恢复等待状态；已发布的匹配源版本译文继续可读，且不提供手动重试。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("等待模型配置恢复，恢复后会自动继续", {
				exact: true,
			}),
		).toBeVisible();
		await expect(canvas.getByText("已有有效译文仍然可读。")).toBeVisible();
		expect(canvas.queryByRole("button", { name: "重试翻译" })).toBeNull();
	},
};

export const AnnouncementSmartBlockedConfig: Story = {
	globals: {
		viewport: {
			value: "feedItemMobile393",
			isRotated: false,
		},
	},
	render: () => (
		<FeedItemCardPreview
			activeLane="smart"
			item={buildAnnouncementItem({
				smart: {
					lang: "zh-CN",
					status: "blocked_config",
					title: null,
					summary: null,
					error_code: "configuration",
				},
			})}
		/>
	),
	parameters: {
		docs: {
			description: {
				story:
					"润色等待模型配置期间继续显示原文，并由原请求轮询状态；不显示失败或重试入口。",
			},
		},
	},
	play: async ({ canvasElement }) => {
		const canvas = within(canvasElement);
		await expect(
			canvas.getByText("等待模型配置恢复，恢复后会自动继续", {
				exact: true,
			}),
		).toBeVisible();
		await expect(
			canvas.getByText("公告卡与 release 卡共享原文 / 翻译 / 润色三 lane"),
		).toBeVisible();
		expect(canvas.queryByRole("button", { name: "立即润色" })).toBeNull();
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
