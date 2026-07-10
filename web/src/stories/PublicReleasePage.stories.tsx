import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, within } from "storybook/test";

import { PublicReleasePage } from "@/pages/PublicReleasePage";
import {
	type VersionMonitorValue,
	VersionMonitorStateProvider,
} from "@/version/versionMonitor";

const PUBLIC_RELEASE_VERSION_HREF =
	"/public/IvanLi-CN/octo-rill/releases/tag/v2.29.0";
const PUBLIC_RELEASE_VIEWPORTS = {
	...INITIAL_VIEWPORTS,
	publicReleaseMobile390: {
		name: "Public Release mobile 390x844",
		styles: {
			height: "844px",
			width: "390px",
		},
		type: "mobile",
	},
} as const;
const publicReleaseVersionState: VersionMonitorValue = {
	loadedVersion: "v2.29.0",
	availableVersion: null,
	hasUpdate: false,
	hasServiceWorkerUpdate: false,
	refreshPage: () => undefined,
};

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
	...Array.from({ length: 8 }, (_, index) => {
		const tag = index < 4 ? `v2.${3 - index}.0` : `v1.${9 - (index - 4)}.0`;
		const previousTag =
			index < 3
				? `v2.${2 - index}.0`
				: index === 3
					? "v1.9.0"
					: `v1.${8 - (index - 4)}.0`;
		return {
			release_id: String(291058023 - index),
			repo_full_name: "octo-rill/example",
			tag_name: tag,
			previous_tag_name: previousTag,
			name: `${tag} cached release page`,
			body: "## Maintenance\n\n- Cached public release entry for pagination verification.\n",
			html_url: `https://github.com/octo-rill/example/releases/tag/${tag}`,
			published_at: `2026-03-${String(24 - index).padStart(2, "0")}T08:00:00Z`,
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

type PublicReleaseStoryMode =
	| "loading"
	| "pending"
	| "list"
	| "owned-public-ready"
	| "highlight-ids"
	| "highlight-small-range"
	| "highlight-large-range"
	| "highlight-partial"
	| "detail"
	| "detail-long"
	| "error";

const ownedPublicReleaseItems = [
	{
		...releaseItems[0],
		repo_full_name: "IvanLi-CN/tuckmark",
		tag_name: "v0.2.0-preview.11",
		previous_tag_name: "v0.1.2-preview.8",
		name: "v0.2.0-preview.11",
		html_url:
			"https://github.com/IvanLi-CN/tuckmark/releases/tag/v0.2.0-preview.11",
		body: "Tuckmark release v0.2.0-preview.11",
	},
	{
		...releaseItems[1],
		repo_full_name: "IvanLi-CN/tuckmark",
		tag_name: "v0.1.2-preview.8",
		previous_tag_name: null,
		name: "v0.1.2-preview.8",
		html_url:
			"https://github.com/IvanLi-CN/tuckmark/releases/tag/v0.1.2-preview.8",
		body: "Tuckmark release v0.1.2-preview.8",
	},
];

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
			if (mode === "loading") {
				return new Promise<Response>(() => {});
			}
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
			if (mode === "error") {
				return new Response("", { status: 554 });
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
			if (mode === "owned-public-ready") {
				return new Response(
					JSON.stringify({
						status: "ready",
						repo_full_name: "IvanLi-CN/tuckmark",
						next_cursor: null,
						items: ownedPublicReleaseItems,
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
			const limit = Number(url.searchParams.get("limit") ?? "6");
			const cursor = url.searchParams.get("cursor");
			const direction = url.searchParams.get("direction") ?? "older";
			const ids = url.searchParams.get("highlight_ids")?.split(",");
			const startId = url.searchParams.get("highlight_start");
			const endId = url.searchParams.get("highlight_end");
			if (ids) {
				const resolvedIds = ids.filter((id) =>
					publicReleaseItems.some((item) => item.release_id === id),
				);
				const items = publicReleaseItems
					.filter((item) => resolvedIds.includes(item.release_id))
					.map((item) => ({ ...item, is_highlighted: true }));
				return new Response(
					JSON.stringify({
						status: "ready",
						repo_full_name: "octo-rill/example",
						next_cursor: null,
						items,
						highlight: {
							mode: "ids",
							requested_ids: ids,
							resolved_ids: resolvedIds,
							unresolved_ids: ids.filter((id) => !resolvedIds.includes(id)),
						},
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
			if (startId !== null || endId !== null) {
				const startIndex = startId
					? publicReleaseItems.findIndex((item) => item.release_id === startId)
					: -1;
				const endIndex = endId
					? publicReleaseItems.findIndex((item) => item.release_id === endId)
					: -1;
				const resolvedIds = [startId, endId].filter(
					(id): id is string =>
						Boolean(id) &&
						publicReleaseItems.some((item) => item.release_id === id),
				);
				const unresolvedIds = [startId, endId].filter(
					(id): id is string =>
						Boolean(id) && !resolvedIds.includes(id as string),
				);
				if (startIndex < 0 || endIndex < 0) {
					const items = publicReleaseItems
						.filter((item) => resolvedIds.includes(item.release_id))
						.map((item) => ({ ...item, is_highlighted: true }));
					return new Response(
						JSON.stringify({
							status: "ready",
							repo_full_name: "octo-rill/example",
							next_cursor: null,
							items,
							highlight: {
								mode: "range",
								requested_ids: [startId, endId].filter((id): id is string =>
									Boolean(id),
								),
								resolved_ids: resolvedIds,
								unresolved_ids: unresolvedIds,
								start_id: startId,
								end_id: endId,
							},
						}),
						{
							status: 200,
							headers: { "content-type": "application/json" },
						},
					);
				}
				const rangeStart = Math.min(startIndex, endIndex);
				const rangeEnd = Math.max(startIndex, endIndex) + 1;
				const rangeItems = publicReleaseItems.slice(rangeStart, rangeEnd);
				const cursorOffset = cursor
					? Number(cursor.split("|").at(-1) ?? rangeStart)
					: rangeStart;
				const offset = cursor ? cursorOffset : rangeStart;
				const pageItems =
					direction === "newer"
						? publicReleaseItems.slice(
								Math.max(rangeStart, offset - limit),
								offset,
							)
						: rangeItems.slice(
								offset - rangeStart,
								offset - rangeStart + limit,
							);
				const items = pageItems.map((item) => ({
					...item,
					is_highlighted: true,
				}));
				const nextOffset = offset - rangeStart + items.length;
				return new Response(
					JSON.stringify({
						status: "ready",
						repo_full_name: "octo-rill/example",
						next_cursor:
							direction === "older" && nextOffset < rangeItems.length
								? `storybook|${rangeStart + nextOffset}`
								: null,
						previous_cursor:
							cursor && direction === "older" ? `storybook|${offset}` : null,
						items,
						highlight: {
							mode: "range",
							requested_ids: [startId, endId],
							resolved_ids: [startId, endId],
							unresolved_ids: [],
							start_id: startId,
							end_id: endId,
						},
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
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
	const highlight =
		props.mode === "highlight-ids"
			? { mode: "ids" as const, ids: ["291058027", "291058025"] }
			: props.mode === "highlight-small-range"
				? {
						mode: "range" as const,
						start: "291058027",
						end: "291058024",
					}
				: props.mode === "highlight-large-range"
					? {
							mode: "range" as const,
							start: "291058027",
							end: "291058020",
						}
					: props.mode === "highlight-partial"
						? {
								mode: "range" as const,
								start: "291058027",
								end: "missing-release",
							}
						: null;

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
		<VersionMonitorStateProvider value={publicReleaseVersionState}>
			<PublicReleasePage
				owner={props.mode === "owned-public-ready" ? "IvanLi-CN" : "octo-rill"}
				repo={
					props.mode === "owned-public-ready"
						? "tuckmark"
						: props.mode === "detail-long"
							? "example-repository-name-that-is-intentionally-long-for-mobile-layout-proof"
							: "example"
				}
				tag={
					props.mode === "detail" || props.mode === "detail-long"
						? "v2.7.0"
						: null
				}
				highlight={highlight}
			/>
		</VersionMonitorStateProvider>
	);
}

async function expectPublicReleaseFooterVersion(canvasElement: HTMLElement) {
	const canvas = within(canvasElement);
	const versionLink = await canvas.findByRole("link", {
		name: "Version v2.29.0",
	});
	await expect(versionLink).toBeVisible();
	await expect(versionLink).toHaveAttribute(
		"href",
		PUBLIC_RELEASE_VERSION_HREF,
	);
	expect(
		canvas.getAllByRole("link", { name: "GitHub" }).length,
	).toBeGreaterThan(0);
	expect(
		canvasElement.ownerDocument.documentElement.scrollWidth -
			canvasElement.ownerDocument.documentElement.clientWidth,
	).toBeLessThanOrEqual(1);
}

const meta = {
	title: "Public/PublicReleasePage",
	component: PublicReleaseStory,
	parameters: {
		layout: "fullscreen",
		viewport: {
			options: PUBLIC_RELEASE_VIEWPORTS,
		},
	},
} satisfies Meta<typeof PublicReleaseStory>;

export default meta;

type Story = StoryObj<typeof meta>;

export const PendingSync: Story = {
	args: { mode: "pending" },
	parameters: {
		viewport: {
			defaultViewport: "publicReleaseMobile390",
		},
	},
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const LoadingKnownData: Story = {
	args: { mode: "loading" },
	play: async ({ canvas, canvasElement }) => {
		await expect(
			canvas.getByTestId("public-release-loading-skeleton"),
		).toBeVisible();
		expect(
			canvas.getAllByTestId("public-release-skeleton-block").length,
		).toBeGreaterThan(0);
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const ReleaseList: Story = {
	args: { mode: "list" },
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const OwnedPublicCacheReady: Story = {
	args: { mode: "owned-public-ready" },
	play: async ({ canvas, canvasElement }) => {
		await expect(
			canvas.getByRole("heading", { name: "IvanLi-CN/tuckmark" }),
		).toBeVisible();
		await expect(
			canvas.getByRole("link", { name: "v0.2.0-preview.11" }),
		).toBeVisible();
		await expect(
			canvas.queryByText("Release 数据同步中"),
		).not.toBeInTheDocument();
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const DiscreteHighlight: Story = {
	args: { mode: "highlight-ids" },
	play: async ({ canvas, canvasElement }) => {
		await expect(
			canvas.getByTestId("public-release-item-291058027"),
		).toHaveAttribute("data-highlighted", "true");
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const SmallRangeHighlight: Story = {
	args: { mode: "highlight-small-range" },
	parameters: {
		viewport: {
			defaultViewport: "publicReleaseMobile390",
		},
	},
	play: async ({ canvas, canvasElement }) => {
		await expect(
			canvas.getByTestId("public-release-item-291058024"),
		).toHaveAttribute("data-highlighted", "true");
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const LargeRangeHighlight: Story = {
	args: { mode: "highlight-large-range" },
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const PartialRangeHighlight: Story = {
	args: { mode: "highlight-partial" },
	play: async ({ canvas, canvasElement }) => {
		await expect(
			canvas.getByTestId("public-release-highlight-unresolved"),
		).toBeVisible();
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const ErrorEdgeTimeout: Story = {
	args: { mode: "error" },
	play: async ({ canvas, canvasElement }) => {
		await expect(canvas.getByText("暂时无法展示")).toBeVisible();
		await expect(
			canvas.getByText("unknown_error: Request failed (HTTP 554)"),
		).toBeVisible();
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const ReleaseDetail: Story = {
	args: { mode: "detail" },
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const LongRepoAndTagDetail: Story = {
	args: { mode: "detail-long" },
	parameters: {
		viewport: {
			defaultViewport: "publicReleaseMobile390",
		},
	},
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};
