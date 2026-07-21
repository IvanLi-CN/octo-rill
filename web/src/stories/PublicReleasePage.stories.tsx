import type { Meta, StoryObj } from "@storybook/react-vite";
import { useEffect } from "react";
import { INITIAL_VIEWPORTS } from "storybook/viewport";
import { expect, userEvent, within } from "storybook/test";

import type { ReleaseDetailResponse } from "@/api";
import { AuthBootstrapProvider } from "@/auth/AuthBootstrap";
import { ReleaseFeedCard } from "@/feed/FeedItemCard";
import { PublicReleasePage } from "@/pages/PublicReleasePage";
import { AppQueryProvider } from "@/query/queryClient";
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
} satisfies ReleaseDetailResponse;

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
	| "polished-fallback"
	| "reactions-enabled"
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

type ReleaseCardStoryProps = Parameters<typeof ReleaseFeedCard>[0];
type ReleaseCardGalleryEmphasis = Exclude<
	ReleaseCardStoryProps["emphasis"],
	undefined
>;

const highlightGalleryItem: ReleaseCardStoryProps["item"] = {
	kind: "release",
	ts: releaseDetail.published_at,
	id: releaseDetail.release_id,
	repo_full_name: releaseDetail.repo_full_name,
	repo_visual: releaseDetail.repo_visual,
	title: "公开更新记录入口",
	body: "这次版本把公开仓库的 Release 列表与详情开放为可直接分享的页面，并提供可重试的 REST API。",
	body_truncated: false,
	subtitle: releaseDetail.tag_name,
	reason: null,
	subject_type: null,
	html_url: releaseDetail.html_url,
	unread: null,
	translated: releaseDetail.translated,
	smart: releaseDetail.smart,
	reactions: null,
};

const highlightGalleryStates: Array<{
	id: string;
	label: string;
	note: string;
	emphasis: ReleaseCardGalleryEmphasis;
}> = [
	{
		id: "default",
		label: "普通态",
		note: "普通列表，没有任何高亮上下文。",
		emphasis: "default",
	},
	{
		id: "subdued",
		label: "非高亮弱化态",
		note: "处在高亮上下文里，但自己不是目标卡片；标题、时间、正文整体退后。",
		emphasis: "subdued",
	},
	{
		id: "highlighted",
		label: "高亮态",
		note: "命中的高亮卡片，使用轮廓和阴影抬起，但弱于当前导航目标。",
		emphasis: "highlighted",
	},
	{
		id: "active-highlight",
		label: "当前高亮态",
		note: "当前导航目标，使用最强轮廓和阴影，和其他卡片一眼拉开。",
		emphasis: "active-highlight",
	},
];

function HighlightCardStateGallery() {
	return (
		<div className="min-h-screen bg-[linear-gradient(180deg,#f7f1e5_0%,#f4ebdc_100%)] px-4 py-6 sm:px-6 sm:py-8">
			<div className="mx-auto max-w-6xl space-y-5">
				<section className="max-w-3xl rounded-[28px] border border-black/5 bg-white/80 p-5 shadow-[0_16px_30px_-34px_rgba(15,23,42,0.16)] backdrop-blur-sm sm:p-6">
					<p className="text-[11px] font-semibold uppercase tracking-[0.22em] text-foreground/38">
						State gallery
					</p>
					<h2 className="mt-2 text-2xl font-semibold tracking-tight text-foreground sm:text-[2rem]">
						公开 Release 卡片高亮层级
					</h2>
					<p className="mt-2 max-w-2xl text-sm leading-6 text-foreground/64">
						同一张卡片，同一个内容，只切换状态层级。
						这里专门用来审阅普通态、弱化态、高亮态和当前高亮态是否真的一眼可分。
					</p>
				</section>

				<div className="grid gap-4 xl:grid-cols-2">
					{highlightGalleryStates.map((state) => (
						<section
							key={state.id}
							data-testid={`public-release-gallery-state-${state.id}`}
							className="rounded-[28px] border border-black/5 bg-white/68 p-4 backdrop-blur-sm sm:p-5"
						>
							<div className="mb-4 space-y-2">
								<div className="inline-flex rounded-full border border-black/6 bg-black/[0.025] px-3 py-1">
									<p className="text-[11px] font-semibold uppercase tracking-[0.18em] text-foreground/40">
										{state.label}
									</p>
								</div>
								<p className="text-sm leading-6 text-foreground/62">
									{state.note}
								</p>
							</div>

							<div className="rounded-[24px] border border-black/4 bg-[#f8f1e4] p-4 sm:p-5">
								<ReleaseFeedCard
									item={highlightGalleryItem}
									activeLane="smart"
									emphasis={state.emphasis}
									isTranslating={false}
									isTranslationAutoRetrying={false}
									isSmartGenerating={false}
									isSmartAutoRetrying={false}
									isReactionBusy={false}
									reactionError={null}
									showReactions={false}
									showRepoIdentity={false}
									showHeaderActions={false}
									onSelectLane={() => undefined}
									onTranslateNow={() => undefined}
									onSmartNow={() => undefined}
									onToggleReaction={() => undefined}
								/>
							</div>
						</section>
					))}
				</div>
			</div>
		</div>
	);
}

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
		if (url.pathname === "/api/me") {
			if (mode !== "reactions-enabled") {
				return new Response(
					JSON.stringify({
						error: { code: "unauthorized", message: "unauthorized" },
					}),
					{ status: 401, headers: { "content-type": "application/json" } },
				);
			}
			return new Response(
				JSON.stringify({
					user: {
						id: "storybook-reaction-user",
						github_user_id: 4242,
						login: "octo",
						name: "Octo",
						avatar_url: null,
						email: null,
						is_admin: false,
					},
					dashboard: {
						daily_boundary_local: "09:00",
						daily_boundary_time_zone: "Asia/Shanghai",
						daily_boundary_utc_offset_minutes: 480,
						include_own_releases: false,
					},
				}),
				{ headers: { "content-type": "application/json" } },
			);
		}
		if (url.pathname === "/api/reaction-token/status") {
			return new Response(
				JSON.stringify({
					configured: mode === "reactions-enabled",
					masked_token:
						mode === "reactions-enabled" ? "ghp_****_storybook" : null,
					check: {
						state: mode === "reactions-enabled" ? "valid" : "idle",
						message: null,
						checked_at:
							mode === "reactions-enabled" ? "2026-07-11T00:00:00Z" : null,
					},
					owner: null,
				}),
				{ headers: { "content-type": "application/json" } },
			);
		}
		if (url.pathname === "/api/feed/reactions/refresh") {
			return new Response(
				JSON.stringify({
					items: publicReleaseItems.map((item) => ({
						release_id: item.release_id,
						reactions: {
							counts: {
								plus1: 0,
								laugh: 0,
								heart: 0,
								hooray: 0,
								rocket: 0,
								eyes: 0,
							},
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
					})),
				}),
				{ headers: { "content-type": "application/json" } },
			);
		}
		if (url.pathname === "/api/release/reactions/toggle") {
			const payload = (await req.json()) as {
				release_id: string;
				content: string;
			};
			return new Response(
				JSON.stringify({
					release_id: payload.release_id,
					reactions: {
						counts: {
							plus1: payload.content === "plus1" ? 1 : 0,
							laugh: 0,
							heart: 0,
							hooray: 0,
							rocket: 0,
							eyes: 0,
						},
						viewer: {
							plus1: payload.content === "plus1",
							laugh: false,
							heart: false,
							hooray: false,
							rocket: false,
							eyes: false,
						},
						status: "ready",
					},
				}),
				{ headers: { "content-type": "application/json" } },
			);
		}
		if (url.pathname.startsWith("/api/public/repos/")) {
			if (url.pathname.endsWith("/releases/content")) {
				const releaseIds = new Set(
					(url.searchParams.get("release_ids") ?? "").split(","),
				);
				return new Response(
					JSON.stringify({
						items: publicReleaseItems
							.filter((item) => releaseIds.has(item.release_id))
							.map((item) => ({
								release_id: item.release_id,
								translated: item.translated,
								smart: item.smart,
							})),
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
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
			if (mode === "polished-fallback") {
				return new Response(
					JSON.stringify({
						status: "ready",
						repo_full_name: "octo-rill/example",
						next_cursor: null,
						items: [{ ...releaseItems[1], is_highlighted: false }],
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
			const selectors = url.searchParams.getAll("highlight");
			const startSelector = url.searchParams.get("highlight_start");
			const endSelector = url.searchParams.get("highlight_end");
			const activeSelector = url.searchParams.get("highlight_active");
			const resolveSelector = (selector: string | null) => {
				if (!selector) return undefined;
				if (selector.startsWith("id:")) {
					return publicReleaseItems.find(
						(item) => item.release_id === selector.slice(3),
					);
				}
				if (selector.startsWith("tag:")) {
					return publicReleaseItems.find(
						(item) => item.tag_name === selector.slice(4),
					);
				}
				return undefined;
			};
			const target = (
				selector: string,
				item: (typeof publicReleaseItems)[number],
			) => ({
				selector,
				release_id: item.release_id,
				tag_name: item.tag_name,
				ordinal: publicReleaseItems.indexOf(item) + 1,
			});
			if (selectors.length > 0) {
				const resolved = selectors.flatMap((selector) => {
					const item = resolveSelector(selector);
					return item ? [target(selector, item)] : [];
				});
				const resolvedIds = resolved.map((item) => item.release_id);
				const active =
					resolved.find((item) => item.selector === activeSelector) ??
					resolved[0];
				const items = publicReleaseItems.map((item) => ({
					...item,
					is_highlighted: resolvedIds.includes(item.release_id),
					is_active_highlight: item.release_id === active?.release_id,
				}));
				return new Response(
					JSON.stringify({
						status: "ready",
						repo_full_name: "octo-rill/example",
						next_cursor: null,
						items,
						highlight: {
							mode: "discrete",
							status:
								resolved.length === selectors.length ? "complete" : "partial",
							requested: selectors,
							resolved,
							unresolved: selectors.filter(
								(selector) => !resolveSelector(selector),
							),
							total: resolved.length,
							active_release_id: active?.release_id ?? null,
							active_index: active ? resolved.indexOf(active) + 1 : null,
						},
						segments:
							items.length > 0
								? [
										{
											first_release_id: items[0].release_id,
											last_release_id: items[items.length - 1].release_id,
										},
									]
								: [],
						gaps: [],
					}),
					{
						status: 200,
						headers: { "content-type": "application/json" },
					},
				);
			}
			if (startSelector !== null || endSelector !== null) {
				const startItem = resolveSelector(startSelector);
				const endItem = resolveSelector(endSelector);
				const startIndex = startItem
					? publicReleaseItems.indexOf(startItem)
					: -1;
				const endIndex = endItem ? publicReleaseItems.indexOf(endItem) : -1;
				const unresolved = [startSelector, endSelector].filter(
					(selector): selector is string =>
						Boolean(selector) && !resolveSelector(selector),
				);
				if (startIndex < 0 || endIndex < 0) {
					const items = publicReleaseItems.slice(0, limit);
					const resolved = [startSelector, endSelector].flatMap((selector) => {
						const item = resolveSelector(selector);
						return selector && item ? [target(selector, item)] : [];
					});
					return new Response(
						JSON.stringify({
							status: "ready",
							repo_full_name: "octo-rill/example",
							next_cursor: null,
							items,
							highlight: {
								mode: "range",
								status: "partial",
								requested: [startSelector, endSelector].filter(
									(value): value is string => Boolean(value),
								),
								resolved,
								unresolved,
								total: 0,
								active_release_id: null,
								active_index: null,
								message: "连续范围的端点未全部命中，已显示普通最新列表",
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
				const activeItem = (() => {
					const candidate = resolveSelector(activeSelector);
					if (!candidate) return startItem;
					const candidateIndex = publicReleaseItems.indexOf(candidate);
					return candidateIndex >= rangeStart && candidateIndex < rangeEnd
						? candidate
						: startItem;
				})();
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
					is_active_highlight: item.release_id === activeItem?.release_id,
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
							status: "complete",
							requested: [startSelector, endSelector],
							resolved: [
								target(startSelector ?? "", startItem!),
								target(endSelector ?? "", endItem!),
							],
							unresolved: [],
							total: rangeItems.length,
							active_release_id: activeItem?.release_id ?? null,
							active_index: activeItem
								? rangeItems.findIndex(
										(item) => item.release_id === activeItem.release_id,
									) + 1
								: null,
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
			? {
					mode: "discrete" as const,
					selectors: ["tag:v2.7.0", "id:291058025", "tag:v1.9.0"],
					active: "id:291058025",
				}
			: props.mode === "highlight-small-range"
				? {
						mode: "range" as const,
						start: "tag:v2.7.0",
						end: "id:291058024",
						active: "id:291058025",
					}
				: props.mode === "highlight-large-range"
					? {
							mode: "range" as const,
							start: "tag:v2.7.0",
							end: "id:291058016",
							active: "id:291058020",
						}
					: props.mode === "highlight-partial"
						? {
								mode: "range" as const,
								start: "tag:v2.7.0",
								end: "tag:missing-release",
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
		<AppQueryProvider>
			<AuthBootstrapProvider>
				<VersionMonitorStateProvider value={publicReleaseVersionState}>
					<PublicReleasePage
						owner={
							props.mode === "owned-public-ready" ? "IvanLi-CN" : "octo-rill"
						}
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
			</AuthBootstrapProvider>
		</AppQueryProvider>
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
	play: async ({ canvas, canvasElement }) => {
		await expect(canvas.getByTestId("public-release-page-lane")).toBeVisible();
		await expect(
			canvas.getByRole("heading", { name: "octo-rill/example" }),
		).toBeVisible();
		expect(
			canvasElement.querySelectorAll("[data-feed-lane-trigger]").length,
		).toBe(0);
		expect(
			canvasElement.querySelectorAll("[data-feed-mobile-github-link]").length,
		).toBe(0);
		expect(
			canvasElement.querySelectorAll(
				"[data-release-id] [data-repo-visual-slot]",
			).length,
		).toBe(0);
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const ReactionsEnabled: Story = {
	args: { mode: "reactions-enabled" },
	play: async ({ canvasElement }) => {
		const firstRelease = canvasElement.querySelector(
			"[data-release-id='291058027']",
		);
		expect(firstRelease).toBeTruthy();
		const plusOne = firstRelease?.querySelector<HTMLButtonElement>(
			"[data-reaction-trigger='plus1']",
		);
		expect(plusOne).toBeTruthy();
		await userEvent.click(plusOne!);
		await expect(plusOne!).toHaveAttribute("aria-pressed", "true");
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
		await expect(
			canvas.getByTestId("public-release-item-291058019"),
		).toHaveAttribute("data-highlighted", "true");
		await expect(
			canvas.getByTestId("public-release-item-291058025"),
		).toHaveAttribute("data-active-highlight", "true");
		await expect(
			canvas.getByTestId("public-release-highlight-navigation"),
		).toHaveTextContent("2 / 3");
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const HighlightStateGallery: Story = {
	name: "Highlight state gallery",
	args: { mode: "list" },
	render: () => <HighlightCardStateGallery />,
	parameters: {
		layout: "fullscreen",
		docs: {
			description: {
				story:
					"把公开 Release 卡片的普通态、非高亮弱化态、高亮态和当前高亮态放进同一审阅面，专门用于确认高亮层级是否足够清晰。",
			},
		},
	},
	play: async ({ canvas, canvasElement }) => {
		await expect(canvas.getByText("公开 Release 卡片高亮层级")).toBeVisible();
		await expect(
			canvas.getByTestId("public-release-gallery-state-default"),
		).toBeVisible();
		await expect(
			canvas.getByTestId("public-release-gallery-state-subdued"),
		).toBeVisible();
		await expect(
			canvas.getByTestId("public-release-gallery-state-highlighted"),
		).toBeVisible();
		await expect(
			canvas.getByTestId("public-release-gallery-state-active-highlight"),
		).toBeVisible();
		expect(
			canvasElement.ownerDocument.documentElement.scrollWidth -
				canvasElement.ownerDocument.documentElement.clientWidth,
		).toBeLessThanOrEqual(1);
	},
};

export const PolishedFallbackToOriginal: Story = {
	args: { mode: "polished-fallback" },
	play: async ({ canvas, canvasElement }) => {
		await expect(canvas.getByText("Shared cache")).toBeVisible();
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
	parameters: {
		viewport: {
			defaultViewport: "publicReleaseMobile390",
		},
	},
	play: async ({ canvasElement }) => {
		await expectPublicReleaseFooterVersion(canvasElement);
	},
};

export const PartialRangeHighlight: Story = {
	args: { mode: "highlight-partial" },
	parameters: {
		viewport: {
			defaultViewport: "publicReleaseMobile390",
		},
	},
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
