import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";

import { PublicReleasePage } from "@/pages/PublicReleasePage";

const repoAvatarDataUrl =
	"data:image/svg+xml;utf8,%3Csvg xmlns='http://www.w3.org/2000/svg' width='96' height='96' viewBox='0 0 96 96'%3E%3Crect width='96' height='96' rx='48' fill='%234f6a98'/%3E%3Ctext x='48' y='58' font-family='Inter,Arial,sans-serif' font-size='34' font-weight='700' text-anchor='middle' fill='white'%3EOR%3C/text%3E%3C/svg%3E";

const releaseDetail = {
	release_id: "291058027",
	repo_full_name: "octo-rill/example",
	repo_visual: {
		owner_avatar_url: repoAvatarDataUrl,
		open_graph_image_url: null,
		uses_custom_open_graph_image: false,
	},
	tag_name: "v2.7.0",
	previous_tag_name: "v2.6.0",
	name: "v2.7.0 public release endpoints",
	body: "## Changes\n\n- Public release pages\n- REST API for release content\n",
	html_url: "https://github.com/octo-rill/example/releases/tag/v2.7.0",
	published_at: "2026-05-04T08:00:00Z",
	is_prerelease: 0,
	is_draft: 0,
	translated: {
		lang: "zh-CN",
		status: "ready",
		title: "v2.7.0 公开 Release 端点",
		summary: "## 变化\n\n- 公开 Release 页面\n- Release 内容 REST API\n",
	},
	smart: {
		lang: "zh-CN",
		status: "ready",
		title: "公开更新记录入口",
		summary:
			"这次版本把公开仓库的 Release 列表与详情开放为可直接分享的页面，并提供可重试的 REST API。",
	},
};

const longRepoAndTagReleaseDetail = {
	...releaseDetail,
	release_id: "291058099",
	repo_full_name:
		"octo-rill/example-repository-name-that-is-intentionally-long-for-mobile-layout-proof",
	tag_name:
		"v2026.05.04-public-release-endpoints-with-extremely-long-tag-name-for-layout-proof",
	previous_tag_name:
		"v2026.04.28-shared-release-cache-with-long-previous-tag-for-layout-proof",
	name: "公开更新记录入口",
	html_url:
		"https://github.com/octo-rill/example-repository-name-that-is-intentionally-long-for-mobile-layout-proof/releases/tag/v2026.05.04-public-release-endpoints-with-extremely-long-tag-name-for-layout-proof",
};

type StoryWindow = Window & {
	__publicReleaseOriginalFetch?: typeof window.fetch;
};

const releaseItems = [
	{
		release_id: releaseDetail.release_id,
		repo_full_name: releaseDetail.repo_full_name,
		repo_visual: releaseDetail.repo_visual,
		tag_name: releaseDetail.tag_name,
		previous_tag_name: releaseDetail.previous_tag_name,
		name: releaseDetail.name,
		body: releaseDetail.body,
		html_url: releaseDetail.html_url,
		published_at: releaseDetail.published_at,
		is_prerelease: 0,
		is_draft: 0,
		translated: releaseDetail.translated,
		smart: releaseDetail.smart,
	},
	{
		release_id: "291058026",
		repo_full_name: "octo-rill/example",
		repo_visual: releaseDetail.repo_visual,
		tag_name: "v2.6.0",
		previous_tag_name: "v2.5.0",
		name: "v2.6.0 shared release cache",
		body: "## Shared cache\n\n- Public pages and signed-in feed read the same `repo_releases` rows.\n",
		html_url: "https://github.com/octo-rill/example/releases/tag/v2.6.0",
		published_at: "2026-04-28T08:00:00Z",
		is_prerelease: 0,
		is_draft: 0,
		translated: {
			lang: "zh-CN",
			status: "ready",
			title: "v2.6.0 共享 Release 缓存",
			summary: "登录用户视图与公开端点读取同一份仓库级 Release 数据。",
		},
		smart: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
	},
	{
		release_id: "291058025",
		repo_full_name: "octo-rill/example",
		repo_visual: releaseDetail.repo_visual,
		tag_name: "v2.5.0",
		previous_tag_name: "v2.4.0",
		name: "v2.5.0 translation warmup pending",
		body: "## Warmup\n\n- Translation and polish caches can be missing while release data is ready.\n",
		html_url: "https://github.com/octo-rill/example/releases/tag/v2.5.0",
		published_at: "2026-04-21T08:00:00Z",
		is_prerelease: 1,
		is_draft: 0,
		translated: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
		smart: {
			lang: "zh-CN",
			status: "ready",
			title: "翻译预热中的预发布版本",
			summary: "管理后台可以看到 ready 与 missing 的数量差异。",
		},
	},
	{
		release_id: "291058024",
		repo_full_name: "octo-rill/example",
		tag_name: "v2.4.0",
		previous_tag_name: "v2.3.0",
		name: "v2.4.0 very long release notes",
		body:
			"## Notes\n\n" +
			"- ".repeat(1800) +
			"this release deliberately carries a very long body to prove the list still truncates gracefully.\n",
		html_url: "https://github.com/octo-rill/example/releases/tag/v2.4.0",
		published_at: "2026-04-14T08:00:00Z",
		is_prerelease: 0,
		is_draft: 0,
		translated: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
		smart: {
			lang: "zh-CN",
			status: "missing",
			title: null,
			summary: null,
		},
	},
];

const publicReleaseItems = [
	...releaseItems,
	...Array.from({ length: 4 }, (_, index) => {
		const minor = 3 - index;
		const previousTag = minor > 0 ? `v2.${minor - 1}.0` : "v1.9.0";
		return {
			release_id: String(291058023 - index),
			repo_full_name: "octo-rill/example",
			tag_name: `v2.${minor}.0`,
			previous_tag_name: previousTag,
			name: `v2.${minor}.0 cached release page`,
			body: "## Maintenance\n\n- Cached public release entry for pagination verification.\n",
			html_url: `https://github.com/octo-rill/example/releases/tag/v2.${minor}.0`,
			published_at: `2026-03-${String(24 - index * 7).padStart(2, "0")}T08:00:00Z`,
			is_prerelease: 0,
			is_draft: 0,
			translated: {
				lang: "zh-CN",
				status: "missing",
				title: null,
				summary: null,
			},
			smart: {
				lang: "zh-CN",
				status: "missing",
				title: null,
				summary: null,
			},
		};
	}),
];

type PublicReleaseStoryMode = "pending" | "list" | "detail" | "detail-long";

function installPublicReleaseMock(mode: PublicReleaseStoryMode) {
	const storyWindow = window as StoryWindow;
	if (!storyWindow.__publicReleaseOriginalFetch) {
		storyWindow.__publicReleaseOriginalFetch = window.fetch.bind(window);
	}
	window.fetch = async (input, init) => {
		const req =
			typeof input === "string" || input instanceof URL
				? new Request(input, init)
				: input;
		const url = new URL(req.url, window.location.origin);
		if (url.pathname.startsWith("/api/public/repos/")) {
			if (mode === "pending") {
				return new Response(
					JSON.stringify({
						status: "pending_sync",
						message:
							"Release data is being prepared. Retry after the suggested delay.",
						reason: "repository_registered_release_sync_pending",
						retry_after_seconds: 60,
						repo_full_name: "octo-rill/example",
						last_requested_at: "2026-05-04T08:05:00Z",
					}),
					{
						status: 202,
						headers: { "content-type": "application/json" },
					},
				);
			}
			if (mode === "detail" || mode === "detail-long") {
				return new Response(
					JSON.stringify(
						mode === "detail-long"
							? longRepoAndTagReleaseDetail
							: releaseDetail,
					),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
			const limit = Number(url.searchParams.get("limit") ?? "6");
			const cursor = url.searchParams.get("cursor");
			const start = cursor ? Number(cursor.split("|").at(-1) ?? "0") : 0;
			const items = publicReleaseItems.slice(start, start + limit);
			const nextStart = start + items.length;
			return new Response(
				JSON.stringify({
					status: "ready",
					repo_full_name: "octo-rill/example",
					next_cursor:
						nextStart < publicReleaseItems.length
							? `storybook|${nextStart}`
							: null,
					items,
				}),
				{
					status: 200,
					headers: { "content-type": "application/json" },
				},
			);
		}
		return storyWindow.__publicReleaseOriginalFetch?.(req) ?? fetch(req);
	};
}

function PublicReleaseStory(props: { mode: PublicReleaseStoryMode }) {
	installPublicReleaseMock(props.mode);

	useEffect(() => {
		return () => {
			const storyWindow = window as StoryWindow;
			if (storyWindow.__publicReleaseOriginalFetch) {
				window.fetch = storyWindow.__publicReleaseOriginalFetch;
				delete storyWindow.__publicReleaseOriginalFetch;
			}
		};
	}, []);

	return (
		<PublicReleasePage
			owner="octo-rill"
			repo={
				props.mode === "detail-long"
					? "example-repository-name-that-is-intentionally-long-for-mobile-layout-proof"
					: "example"
			}
			tag={
				props.mode === "detail" || props.mode === "detail-long"
					? "v2.7.0"
					: null
			}
		/>
	);
}

const meta = {
	title: "Public/PublicReleasePage",
	component: PublicReleaseStory,
	parameters: {
		layout: "fullscreen",
	},
} satisfies Meta<typeof PublicReleaseStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const PendingSync: Story = {
	args: { mode: "pending" },
};

export const ReleaseList: Story = {
	args: { mode: "list" },
};

export const ReleaseDetail: Story = {
	args: { mode: "detail" },
};

export const LongRepoAndTagDetail: Story = {
	args: { mode: "detail-long" },
};
