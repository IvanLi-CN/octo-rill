import {
	type Locator,
	type Page,
	type Route,
	expect,
	test,
} from "@playwright/test";

import { buildMockMeResponse } from "./mockApi";

function svgDataUrl(label: string, background: string, foreground = "#ffffff") {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="240" height="240" viewBox="0 0 240 240"><rect width="240" height="240" rx="36" fill="${background}"/><text x="120" y="132" font-family="Inter,Arial,sans-serif" font-size="44" font-weight="700" text-anchor="middle" fill="${foreground}">${label}</text></svg>`,
	)}`;
}

function socialPreviewDataUrl(title: string, accent: string, body: string) {
	return `data:image/svg+xml;utf8,${encodeURIComponent(
		`<svg xmlns="http://www.w3.org/2000/svg" width="1280" height="640" viewBox="0 0 1280 640"><defs><linearGradient id="g" x1="0" x2="1" y1="0" y2="1"><stop offset="0%" stop-color="${accent}"/><stop offset="100%" stop-color="#0f172a"/></linearGradient></defs><rect width="1280" height="640" rx="48" fill="url(#g)"/><text x="112" y="224" font-family="Inter,Arial,sans-serif" font-size="82" font-weight="800" fill="#ffffff">${title}</text><text x="112" y="328" font-family="Inter,Arial,sans-serif" font-size="40" font-weight="600" fill="rgba(255,255,255,0.82)">${body}</text></svg>`,
	)}`;
}

type ApiOptions = {
	releaseId: string;
	detailTitle: string;
	translatedTitle: string;
	translatedSummary: string;
	feedTimestamp: string;
	detailPublishedAt: string;
	briefDate: string;
	briefWindowStart: string;
	briefWindowEnd: string;
	briefMarkdown: string;
	briefCreatedAt: string;
	withReactionFeed?: boolean;
	withAutoTranslateFeed?: boolean;
	withAutoTranslateReactions?: boolean;
	aiDisabledFeed?: boolean;
	autoTranslateFeedCount?: number;
	autoTranslateInitialReadyIds?: string[];
	autoTranslateDisabledIds?: string[];
	autoTranslateResolveStatuses?: Record<string, "ready" | "missing" | "error">;
	autoTranslateResolveFailureCount?: number;
	smartFeedCount?: number;
	smartInitialReadyIds?: string[];
	smartInitialErrorIds?: string[];
	smartResolveStatuses?: Record<string, "ready" | "missing" | "error">;
	smartResolveErrors?: Record<string, TranslationErrorPayload>;
	smartResolveDelayMs?: number;
	releaseDetailPendingPolls?: number;
	releaseDetailInitialStatus?: "ready" | "missing" | "error";
	releaseDetailInitialError?: TranslationErrorPayload;
	releaseDetailRequestStatus?: "ready" | "error" | "disabled";
	releaseDetailRequestError?: TranslationErrorPayload;
};

type MockApiTracker = {
	translationResolveRequests: Array<{
		entityIds: string[];
		kinds: string[];
	}>;
	translationBatchEntityIds: string[][];
	translationSingleEntityIds: string[];
};

type TranslationErrorPayload = {
	error?: string | null;
	error_code?: string | null;
	error_summary?: string | null;
	error_detail?: string | null;
};

function json(route: Route, payload: unknown, status = 200) {
	return route.fulfill({
		status,
		contentType: "application/json",
		body: JSON.stringify(payload),
	});
}

async function expectNoHorizontalOverflow(locator: Locator) {
	const metrics = await locator.evaluate((node) => {
		if (!(node instanceof HTMLElement)) {
			throw new Error("expected HTMLElement");
		}
		const firstLink = node.querySelector("a");
		return {
			clientWidth: node.clientWidth,
			scrollWidth: node.scrollWidth,
			firstLinkOverflowWrap:
				firstLink instanceof HTMLElement
					? window.getComputedStyle(firstLink).overflowWrap
					: null,
		};
	});

	expect(metrics.firstLinkOverflowWrap).toBe("anywhere");
	expect(metrics.scrollWidth - metrics.clientWidth).toBeLessThanOrEqual(1);
}

function makeAutoTranslateReleaseId(index: number) {
	return `200${`${index + 1}`.padStart(2, "0")}`;
}

function makeAutoTranslateFeedItems(
	count: number,
	options?: {
		initialReadyIds?: string[];
		smartInitialReadyIds?: string[];
		smartInitialErrorIds?: string[];
		autoTranslateDisabledIds?: string[];
		aiDisabled?: boolean;
		withReactions?: boolean;
	},
) {
	const initialReadyIds = new Set(options?.initialReadyIds ?? []);
	const smartInitialReadyIds = new Set(options?.smartInitialReadyIds ?? []);
	const smartInitialErrorIds = new Set(options?.smartInitialErrorIds ?? []);
	const autoTranslateDisabledIds = new Set(
		options?.autoTranslateDisabledIds ?? [],
	);
	const aiDisabled = options?.aiDisabled === true;
	const withReactions = options?.withReactions === true;
	return Array.from({ length: count }, (_, index) => {
		const releaseId = makeAutoTranslateReleaseId(index);
		const initialReady = initialReadyIds.has(releaseId);
		const smartReady = smartInitialReadyIds.has(releaseId);
		const readyPayload = makeFeedTranslationPayload(releaseId);
		const smartPayload = makeFeedSmartPayload(releaseId);
		return {
			kind: "release",
			ts: `2026-02-22T${`${(index % 10) + 10}`.padStart(2, "0")}:22:33Z`,
			id: releaseId,
			repo_full_name: "owner/repo",
			repo_visual: {
				owner_avatar_url: svgDataUrl("OR", "#2563eb"),
				open_graph_image_url: socialPreviewDataUrl(
					"Owner Repo",
					"#1d4ed8",
					"feed social preview",
				),
				uses_custom_open_graph_image: true,
			},
			title: `Release ${releaseId}`,
			body: [
				`- lane ${index + 1} keeps current-screen work first`,
				"- next-screen requests should submit in parallel",
				"- secondary prefetch continues within token budget",
			].join("\n"),
			body_truncated: false,
			subtitle: null,
			reason: null,
			subject_type: null,
			html_url: `https://github.com/owner/repo/releases/tag/v${releaseId}`,
			unread: null,
			translated: aiDisabled
				? {
						lang: "zh-CN",
						status: "disabled",
						title: null,
						summary: null,
					}
				: initialReady
					? {
							lang: "zh-CN",
							status: "ready",
							title: readyPayload.title_zh,
							summary: readyPayload.summary_md,
							auto_translate: true,
						}
					: {
							lang: "zh-CN",
							status: "missing",
							title: null,
							summary: null,
							...(autoTranslateDisabledIds.has(releaseId)
								? { auto_translate: false }
								: {}),
						},
			smart: aiDisabled
				? {
						lang: "zh-CN",
						status: "disabled",
						title: null,
						summary: null,
					}
				: smartReady
					? {
							lang: "zh-CN",
							status: "ready",
							title: smartPayload.title_zh,
							summary: smartPayload.body_md,
							auto_translate: true,
						}
					: smartInitialErrorIds.has(releaseId)
						? {
								lang: "zh-CN",
								status: "error",
								title: null,
								summary: null,
								auto_translate: false,
							}
						: {
								lang: "zh-CN",
								status: "missing",
								title: null,
								summary: null,
							},
			reactions: withReactions
				? {
						counts: {
							plus1: 2,
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
					}
				: null,
		};
	});
}

function makeFeedTranslationPayload(
	releaseId: string,
	status: "ready" | "missing" | "error" = "ready",
) {
	return {
		producer_ref: `feed.auto_translate:release:${releaseId}`,
		entity_id: releaseId,
		kind: "release_summary",
		variant: "feed_body",
		status,
		title_zh: status === "ready" ? `发布说明 ${releaseId}` : null,
		summary_md:
			status === "ready" ? `这是 release ${releaseId} 的中文摘要。` : null,
		body_md: null,
		error: status === "error" ? `translate returned ${status}` : null,
		work_item_id: `work-feed-${releaseId}`,
		batch_id: `batch-feed-${releaseId}`,
	};
}

function makeFeedSmartPayload(
	releaseId: string,
	status: "ready" | "missing" | "error" = "ready",
	options?: {
		errorPayload?: TranslationErrorPayload;
	},
) {
	const insufficient = status === "missing";
	const errorPayload = options?.errorPayload;
	return {
		producer_ref: `feed.smart:release:${releaseId}`,
		entity_id: releaseId,
		kind: "release_smart",
		variant: "feed_card",
		status,
		title_zh: status === "ready" ? `润色 ${releaseId}` : null,
		summary_md: null,
		body_md:
			status === "ready"
				? `- 润色 release ${releaseId} 的主要版本变化。\n- 方便快速理解这次发布的重点。`
				: null,
		error:
			status === "error"
				? (errorPayload?.error ?? "smart summary failed")
				: insufficient
					? "no_valuable_version_info"
					: null,
		error_code: status === "error" ? (errorPayload?.error_code ?? null) : null,
		error_summary:
			status === "error" ? (errorPayload?.error_summary ?? null) : null,
		error_detail:
			status === "error" ? (errorPayload?.error_detail ?? null) : null,
		work_item_id: `work-smart-${releaseId}`,
		batch_id: `batch-smart-${releaseId}`,
	};
}

function makeReleaseDetailTranslatedPayload(
	cfg: ApiOptions,
	status: "ready" | "missing" | "error",
) {
	if (status === "ready") {
		return {
			lang: "zh-CN",
			status: "ready" as const,
			title: cfg.translatedTitle,
			summary: cfg.translatedSummary,
			error_code: null,
			error_summary: null,
			error_detail: null,
		};
	}
	const errorPayload = cfg.releaseDetailInitialError;
	return {
		lang: "zh-CN",
		status,
		title: null,
		summary: null,
		error_code: status === "error" ? (errorPayload?.error_code ?? null) : null,
		error_summary:
			status === "error" ? (errorPayload?.error_summary ?? null) : null,
		error_detail:
			status === "error" ? (errorPayload?.error_detail ?? null) : null,
	};
}

function makeReleaseDetailRequestResult(
	cfg: ApiOptions,
	status: "queued" | "running" | "ready" | "error" | "disabled",
) {
	const errorPayload = cfg.releaseDetailRequestError;
	return {
		producer_ref: `release_detail:${cfg.releaseId}`,
		entity_id: cfg.releaseId,
		kind: "release_detail",
		variant: "detail_card",
		status,
		title_zh: status === "ready" ? cfg.translatedTitle : null,
		summary_md: null,
		body_md: status === "ready" ? cfg.translatedSummary : null,
		error:
			status === "error"
				? (errorPayload?.error ?? "translate returned error")
				: null,
		error_code: status === "error" ? (errorPayload?.error_code ?? null) : null,
		error_summary:
			status === "error" ? (errorPayload?.error_summary ?? null) : null,
		error_detail:
			status === "error" ? (errorPayload?.error_detail ?? null) : null,
		work_item_id: "work-detail-1",
		batch_id: "batch-detail-1",
	};
}

function makeFeedRequestId(releaseId: string) {
	return `req-feed-${releaseId}`;
}

async function installApiMocks(
	page: Page,
	options?: Partial<ApiOptions>,
): Promise<MockApiTracker> {
	const cfg: ApiOptions = {
		releaseId: "123",
		detailTitle: "Release 123",
		translatedTitle: "发布说明 123",
		translatedSummary: "这是 release 123 的中文详情摘要。",
		feedTimestamp: "2026-02-22T11:22:33Z",
		detailPublishedAt: "2026-02-22T11:22:33Z",
		briefDate: "2026-02-23",
		briefWindowStart: "2026-02-22T00:00:00Z",
		briefWindowEnd: "2026-02-23T00:00:00Z",
		briefMarkdown: `[repo/v1.2.3](/?tab=briefs&release=${options?.releaseId ?? "123"})`,
		briefCreatedAt: "2026-02-23T08:00:00Z",
		releaseDetailPendingPolls: 0,
		releaseDetailInitialStatus: "missing",
		releaseDetailRequestStatus: "ready",
		autoTranslateFeedCount: 18,
		...options,
	};
	const translationRequestPolls = new Map<string, number>();
	let autoTranslateResolveFailureCount =
		cfg.autoTranslateResolveFailureCount ?? 0;
	let reactionTokenConfigured = false;
	let reactionTokenMasked: string | null = null;
	let reactionTokenCheckedAt: string | null = null;
	const tracker: MockApiTracker = {
		translationResolveRequests: [],
		translationBatchEntityIds: [],
		translationSingleEntityIds: [],
	};

	await page.route("**/api/**", async (route) => {
		const req = route.request();
		const url = new URL(req.url());
		const { pathname } = url;

		if (req.method() === "GET" && pathname === "/api/me") {
			return json(
				route,
				buildMockMeResponse({
					id: "2f4k7m9p3x6c8v2a",
					github_user_id: 10,
					login: "octo",
					name: "Octo",
					avatar_url: null,
					email: null,
					is_admin: false,
				}),
			);
		}

		if (req.method() === "GET" && pathname === "/api/feed") {
			const items = cfg.withReactionFeed
				? [
						{
							kind: "release",
							ts: cfg.feedTimestamp,
							id: cfg.releaseId,
							repo_full_name: "owner/repo",
							repo_visual: {
								owner_avatar_url: svgDataUrl("OR", "#2563eb"),
								open_graph_image_url: socialPreviewDataUrl(
									"Owner Repo",
									"#1d4ed8",
									"feed social preview",
								),
								uses_custom_open_graph_image: true,
							},
							title: cfg.detailTitle,
							body: "- fix A\n- fix B",
							body_truncated: false,
							subtitle: null,
							reason: null,
							subject_type: null,
							html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
							unread: null,
							translated: {
								lang: "zh-CN",
								status: "ready",
								title: cfg.translatedTitle,
								summary: cfg.translatedSummary,
							},
							smart: {
								lang: "zh-CN",
								status: "ready",
								title: "润色 123",
								summary:
									"- 这一版主要聚焦 feed 卡片体验与版本阅读效率。\n- 同步补齐反馈按钮与浏览器端交互状态。",
							},
							reactions: {
								counts: {
									plus1: 2,
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
						},
					]
				: cfg.withAutoTranslateFeed
					? makeAutoTranslateFeedItems(cfg.autoTranslateFeedCount ?? 18, {
							initialReadyIds: cfg.autoTranslateInitialReadyIds,
							smartInitialReadyIds: cfg.smartInitialReadyIds,
							smartInitialErrorIds: cfg.smartInitialErrorIds,
							autoTranslateDisabledIds: cfg.autoTranslateDisabledIds,
							aiDisabled: cfg.aiDisabledFeed,
							withReactions: cfg.withAutoTranslateReactions,
						})
					: [];
			return json(route, { items, next_cursor: null });
		}

		if (req.method() === "GET" && pathname === "/api/notifications") {
			return json(route, []);
		}

		if (req.method() === "GET" && pathname === "/api/briefs") {
			return json(route, [
				{
					date: cfg.briefDate,
					window_start: cfg.briefWindowStart,
					window_end: cfg.briefWindowEnd,
					content_markdown: cfg.briefMarkdown,
					created_at: cfg.briefCreatedAt,
				},
			]);
		}

		if (req.method() === "GET" && pathname === "/api/reaction-token/status") {
			return json(route, {
				configured: reactionTokenConfigured,
				masked_token: reactionTokenMasked,
				check: {
					state: reactionTokenConfigured ? "valid" : "idle",
					message: reactionTokenConfigured ? "token is valid" : null,
					checked_at: reactionTokenCheckedAt,
				},
			});
		}

		if (req.method() === "POST" && pathname === "/api/reaction-token/check") {
			const body = req.postDataJSON() as { token?: string };
			const token = String(body.token ?? "");
			const valid = token.startsWith("ghp_") && token.length >= 12;
			return json(route, {
				state: valid ? "valid" : "invalid",
				message: valid ? "token is valid" : "token is invalid",
			});
		}

		if (req.method() === "PUT" && pathname === "/api/reaction-token") {
			const body = req.postDataJSON() as { token?: string };
			const token = String(body.token ?? "");
			reactionTokenConfigured = true;
			reactionTokenMasked = `${token.slice(0, 4)}****${token.slice(-4)}`;
			reactionTokenCheckedAt = "2026-02-21T16:00:00Z";
			return json(route, {
				configured: true,
				masked_token: reactionTokenMasked,
				check: {
					state: "valid",
					message: "token is valid",
					checked_at: reactionTokenCheckedAt,
				},
			});
		}

		if (
			req.method() === "POST" &&
			pathname === "/api/release/reactions/toggle"
		) {
			if (!reactionTokenConfigured) {
				return json(
					route,
					{
						error: {
							code: "pat_required",
							message: "release reactions require a GitHub PAT",
						},
					},
					400,
				);
			}
			return json(route, {
				reactions: {
					counts: {
						plus1: 3,
						laugh: 0,
						heart: 0,
						hooray: 0,
						rocket: 0,
						eyes: 0,
					},
					viewer: {
						plus1: true,
						laugh: false,
						heart: false,
						hooray: false,
						rocket: false,
						eyes: false,
					},
					status: "ready",
				},
			});
		}

		if (
			req.method() === "GET" &&
			pathname === `/api/releases/${cfg.releaseId}/detail`
		) {
			return json(route, {
				release_id: cfg.releaseId,
				repo_full_name: "owner/repo",
				repo_visual: {
					owner_avatar_url: svgDataUrl("OR", "#2563eb"),
					open_graph_image_url: null,
					uses_custom_open_graph_image: false,
				},
				tag_name: "v1.2.3",
				name: cfg.detailTitle,
				body: "- fix A\n- fix B",
				body_truncated: false,
				html_url: "https://github.com/owner/repo/releases/tag/v1.2.3",
				published_at: cfg.detailPublishedAt,
				is_prerelease: 0,
				is_draft: 0,
				translated: makeReleaseDetailTranslatedPayload(
					cfg,
					cfg.releaseDetailInitialStatus ?? "missing",
				),
			});
		}

		if (req.method() === "POST" && pathname === "/api/translate/results") {
			const body = req.postDataJSON() as {
				items?: Array<{ entity_id?: string; kind?: string }>;
			};
			const items = body.items ?? [];
			const entityIds = items.map((item) => item.entity_id ?? "");
			const kinds = items.map((item) => item.kind ?? "");
			tracker.translationResolveRequests.push({ entityIds, kinds });
			if (autoTranslateResolveFailureCount > 0) {
				autoTranslateResolveFailureCount -= 1;
				return json(
					route,
					{ error: { message: "transient translate results failure" } },
					503,
				);
			}
			if (
				cfg.smartResolveDelayMs &&
				items.some((item) => item.kind === "release_smart")
			) {
				await new Promise((resolve) =>
					setTimeout(resolve, cfg.smartResolveDelayMs),
				);
			}
			return json(route, {
				items: items.map((item) => {
					const entityId = item.entity_id ?? "";
					if (item.kind === "release_smart") {
						return makeFeedSmartPayload(
							entityId,
							cfg.smartResolveStatuses?.[entityId] ?? "ready",
							{
								errorPayload: cfg.smartResolveErrors?.[entityId],
							},
						);
					}
					return makeFeedTranslationPayload(
						entityId,
						cfg.autoTranslateResolveStatuses?.[entityId] ?? "ready",
					);
				}),
			});
		}

		if (req.method() === "POST" && pathname === "/api/translate/requests") {
			const body = req.postDataJSON() as {
				mode?: string;
				item?: {
					producer_ref?: string;
					entity_id?: string;
					kind?: string;
					variant?: string;
				};
				items?: Array<{
					producer_ref?: string;
					entity_id?: string;
					kind?: string;
					variant?: string;
				}>;
			};
			if (body.mode === "wait") {
				const item = body.item;
				if (!item || item.entity_id !== cfg.releaseId) {
					return json(
						route,
						{ error: { message: "unexpected translation request" } },
						400,
					);
				}
				const requestId = `req-${item.kind ?? "translation"}-1`;
				const isPendingReleaseDetail =
					item.kind === "release_detail" && cfg.releaseDetailPendingPolls > 0;
				return json(route, {
					request_id: requestId,
					status: isPendingReleaseDetail ? "running" : "completed",
					result: {
						...(item.kind === "release_detail"
							? makeReleaseDetailRequestResult(
									cfg,
									isPendingReleaseDetail
										? "running"
										: (cfg.releaseDetailRequestStatus ?? "ready"),
								)
							: item.kind === "release_smart"
								? {
										...makeFeedSmartPayload(cfg.releaseId),
										status: isPendingReleaseDetail ? "running" : "ready",
										title_zh: isPendingReleaseDetail
											? null
											: `润色 ${cfg.releaseId}`,
										body_md: isPendingReleaseDetail
											? null
											: `- 润色 ${cfg.releaseId} 的版本变化。\n- 方便主人快速读懂 release。`,
									}
								: {
										producer_ref:
											item.producer_ref ??
											`feed.auto_translate:release:${cfg.releaseId}`,
										entity_id: cfg.releaseId,
										kind: item.kind ?? "release_summary",
										variant: item.variant ?? "feed_body",
										status: isPendingReleaseDetail ? "running" : "ready",
										title_zh: isPendingReleaseDetail
											? null
											: cfg.translatedTitle,
										summary_md: isPendingReleaseDetail
											? null
											: cfg.translatedSummary,
										body_md: null,
										error: null,
										error_code: null,
										error_summary: null,
										error_detail: null,
										work_item_id: "work-feed-1",
										batch_id: "batch-feed-1",
									}),
					},
				});
			}
			if (body.mode === "async" && Array.isArray(body.items)) {
				const entityIds = body.items.map((item) => item.entity_id ?? "");
				tracker.translationBatchEntityIds.push(entityIds);
				return json(route, {
					requests: body.items.map((item) => ({
						request_id: makeFeedRequestId(item.entity_id ?? ""),
						status: "queued",
						producer_ref:
							item.producer_ref ??
							(item.kind === "release_smart"
								? `feed.smart:release:${item.entity_id ?? ""}`
								: `feed.auto_translate:release:${item.entity_id ?? ""}`),
						entity_id: item.entity_id ?? "",
						kind: item.kind ?? "release_summary",
						variant:
							item.variant ??
							(item.kind === "release_smart" ? "feed_card" : "feed_body"),
					})),
				});
			}
			if (body.mode === "async" && body.item) {
				const entityId = body.item.entity_id ?? "";
				tracker.translationSingleEntityIds.push(entityId);
				return json(route, {
					request_id: makeFeedRequestId(entityId),
					status: "queued",
					result:
						body.item.kind === "release_detail"
							? {
									producer_ref: `release_detail:${entityId}`,
									entity_id: entityId,
									kind: "release_detail",
									variant: "detail_card",
									status: "queued",
									title_zh: null,
									summary_md: null,
									body_md: null,
									error: null,
									work_item_id: "work-detail-1",
									batch_id: "batch-detail-1",
								}
							: body.item.kind === "release_smart"
								? {
										...makeFeedSmartPayload(entityId),
										status: "queued",
										title_zh: null,
										body_md: null,
									}
								: {
										...makeFeedTranslationPayload(entityId),
										status: "queued",
										title_zh: null,
										summary_md: null,
									},
				});
			}
			return json(
				route,
				{ error: { message: "unsupported translation mode" } },
				400,
			);
		}

		if (
			req.method() === "GET" &&
			pathname.startsWith("/api/translate/requests/")
		) {
			const requestId = pathname.split("/").at(-1);
			if (!requestId) {
				return json(route, { error: { message: "missing request id" } }, 400);
			}
			const polls = translationRequestPolls.get(requestId) ?? 0;
			translationRequestPolls.set(requestId, polls + 1);
			const isPendingReleaseDetail =
				requestId === "req-release_detail-1" &&
				polls < cfg.releaseDetailPendingPolls;
			if (requestId.startsWith("req-feed-")) {
				const releaseId = requestId.replace("req-feed-", "");
				return json(route, {
					request_id: requestId,
					status: "completed",
					result: makeFeedTranslationPayload(releaseId),
				});
			}
			if (requestId.startsWith("req-smart-")) {
				const releaseId = requestId.replace("req-smart-", "");
				return json(route, {
					request_id: requestId,
					status: "completed",
					result: makeFeedSmartPayload(
						releaseId,
						cfg.smartResolveStatuses?.[releaseId] ?? "ready",
					),
				});
			}
			return json(route, {
				request_id: requestId,
				status: isPendingReleaseDetail ? "running" : "completed",
				result: makeReleaseDetailRequestResult(
					cfg,
					isPendingReleaseDetail
						? "running"
						: (cfg.releaseDetailRequestStatus ?? "ready"),
				),
			});
		}

		return json(
			route,
			{ error: { message: `unhandled ${req.method()} ${pathname}` } },
			404,
		);
	});

	return tracker;
}

test("deep link with release id opens briefs tab and loads release detail", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseId: "289513858",
		detailTitle: "Release 289513858",
	});

	await page.goto("/?release=289513858");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).toHaveURL(/release=289513858/);
	const detailDialog = page.getByRole("dialog", { name: "Release 详情" });
	await expect(detailDialog).toBeVisible();
	await expect(detailDialog.getByText("#289513858")).toBeVisible();
	await expect(detailDialog.getByText("owner/repo")).toBeVisible();
	await expect(
		detailDialog.getByRole("heading", { name: "Release 289513858" }),
	).toBeVisible();
	await expect(
		detailDialog.locator('[data-repo-visual-kind="owner_avatar"]').first(),
	).toBeVisible();

	await detailDialog.getByRole("button", { name: "关闭" }).click();
	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).not.toHaveURL(/release=289513858/);
	await expect(detailDialog).toHaveCount(0);
	await expect(page.getByRole("tab", { name: "日报" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
});

test("detail translate button updates card content", async ({ page }) => {
	await installApiMocks(page);

	await page.goto("/?tab=briefs&release=123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "原文" }).click();
	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
});

test("shared markdown compacts raw GitHub links and keeps long links wrapped", async ({
	page,
}) => {
	const rawPrUrl = "https://github.com/CherryHQ/cherry-studio/pull/14247";
	const rawCommitUrl =
		"https://github.com/CherryHQ/cherry-studio/commit/4d8f459e7869d3e0b57fafe1b7a9034cb9b2d999";
	const rawDocsUrl =
		"https://docs.example.com/releases/cherry-studio/2026/04/21/notes/with/a/very/long/path/that/should/wrap/inside/the/dashboard/brief/card?source=dashboard&view=full&lang=zh-CN";
	const rawStatusUrl =
		"https://status.example.com/incidents/cherry-studio/2026/04/21/with/a/very/long/path/that/should/stay/fully-readable/in/the/release/detail/dialog?ref=dashboard&channel=playwright";

	await installApiMocks(page, {
		briefMarkdown: [
			"## 项目更新",
			"",
			"### [CherryHQ/cherry-studio](https://github.com/CherryHQ/cherry-studio)",
			"",
			"- [skills workspace links](/?tab=briefs&release=123) · 2026-04-20T15:18:00Z · [GitHub Release](https://github.com/CherryHQ/cherry-studio/releases/tag/v1.0.0)",
			`  - 原始 GitHub PR autolink 会被压缩成短标签：${rawPrUrl}`,
			"  - 已有短标签保持不变：[#13840](https://github.com/CherryHQ/cherry-studio/pull/13840)",
			`  - 长文档链接继续保留原文并允许换行：${rawDocsUrl}`,
		].join("\n"),
		translatedSummary: [
			`- 原始 commit autolink 会压缩成短 SHA：${rawCommitUrl}`,
			`- 长状态页链接继续保留原文并允许换行：${rawStatusUrl}`,
		].join("\n"),
	});

	await page.goto("/?tab=briefs");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	const briefPanel = page.getByRole("tabpanel", { name: "日报" });
	await expect(briefPanel.getByRole("link", { name: "#14247" })).toBeVisible();
	await expect(briefPanel.getByRole("link", { name: "#13840" })).toBeVisible();
	await expect(
		briefPanel.getByRole("link", { name: "GitHub Release" }),
	).toBeVisible();
	await expect(briefPanel.locator(`a[href="${rawDocsUrl}"]`)).toBeVisible();
	await expect(briefPanel).not.toContainText(rawPrUrl);
	await expectNoHorizontalOverflow(
		briefPanel.locator('[data-markdown-root="true"]').first(),
	);

	await briefPanel
		.getByRole("link", { name: "skills workspace links" })
		.click();
	const detailDialog = page.getByRole("dialog", { name: "Release 详情" });
	await expect(detailDialog).toBeVisible();
	await detailDialog.getByRole("button", { name: "翻译" }).click();
	await expect(
		detailDialog.getByRole("link", { name: "4d8f459" }),
	).toBeVisible();
	await expect(detailDialog.locator(`a[href="${rawStatusUrl}"]`)).toBeVisible();
	await expect(detailDialog).not.toContainText(rawCommitUrl);
	await expectNoHorizontalOverflow(
		detailDialog.locator('[data-markdown-root="true"]').first(),
	);
});

test("detail translate keeps polling an in-flight request until the result is ready", async ({
	page,
}) => {
	await installApiMocks(page, { releaseDetailPendingPolls: 2 });

	await page.goto("/?tab=briefs&release=123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();
	await expect(page.getByText("translation wait timeout")).toHaveCount(0);
});

test("detail translate failure keeps the last ready translation visible and falls back to toast", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseDetailInitialStatus: "ready",
		releaseDetailRequestStatus: "error",
		releaseDetailRequestError: {
			error: "release detail translation failed to preserve markdown structure",
			error_code: "markdown_structure_mismatch",
			error_summary: "Markdown 结构校验失败",
			error_detail:
				"release detail translation failed to preserve markdown structure",
		},
	});

	await page.goto("/?tab=briefs&release=123");
	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();

	await page.getByRole("button", { name: "翻译" }).click();

	await expect(
		page.getByRole("heading", { name: "发布说明 123" }),
	).toBeVisible();
	await expect(
		page.getByText("这是 release 123 的中文详情摘要。", { exact: true }),
	).toBeVisible();
	await expect(page.getByRole("button", { name: "原文" })).toBeVisible();
	await expect(page.getByRole("button", { name: "查看原文" })).toHaveCount(0);
	await expect(page.getByText("翻译失败", { exact: true })).toBeVisible();
	await expect(
		page.getByText("Markdown 结构校验失败", { exact: true }),
	).toBeVisible();
});

test("detail retry failure stays visible after switching to the original text", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseDetailInitialStatus: "error",
		releaseDetailInitialError: {
			error: "release detail translation failed to preserve markdown structure",
			error_code: "markdown_structure_mismatch",
			error_summary: "Markdown 结构校验失败",
			error_detail:
				"release detail translation failed to preserve markdown structure",
		},
		releaseDetailRequestStatus: "error",
		releaseDetailRequestError: {
			error: "release detail translation failed to preserve markdown structure",
			error_code: "markdown_structure_mismatch",
			error_summary: "Markdown 结构校验失败",
			error_detail:
				"release detail translation failed to preserve markdown structure",
		},
	});

	await page.goto("/?tab=briefs&release=123");
	await expect(page.getByText("翻译失败", { exact: true })).toBeVisible();

	await page.getByRole("button", { name: "查看原文" }).click();
	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
	await expect(page.getByText("fix A", { exact: true })).toBeVisible();

	await page.getByRole("button", { name: "翻译" }).click();
	await expect(page.getByText("翻译失败", { exact: true })).toBeVisible();
	await expect(
		page.getByText("Markdown 结构校验失败", { exact: true }),
	).toBeVisible();
});

test("reaction fallback lets users configure PAT inline from the dialog", async ({
	page,
}) => {
	await installApiMocks(page, { withReactionFeed: true });

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.locator('[data-repo-visual-kind="owner_avatar"]').first(),
	).toBeVisible();
	await expect(
		page.locator('[data-repo-visual-kind="social_preview"]'),
	).toHaveCount(0);
	const reactionFooter = page.locator('[data-reaction-footer="true"]').first();
	const plusOneButton = reactionFooter.locator(
		'[data-reaction-trigger="plus1"]',
	);
	await expect(plusOneButton).toBeVisible();
	await plusOneButton.click();

	const patDialog = page.getByRole("dialog", {
		name: "配置 GitHub PAT",
	});
	await expect(patDialog).toBeVisible({ timeout: 10_000 });
	await expect(
		patDialog.getByText(/先补齐 GitHub PAT，才能继续使用站内反馈/),
	).toBeVisible();
	const patInput = patDialog.locator("#dashboard-reaction-pat");
	await expect(patInput).toHaveAttribute("type", "password");
	await expect(patInput).toHaveAttribute("autocomplete", "off");
	await expect(patInput).toHaveAttribute("data-1p-ignore", "true");
	await expect(patInput).toHaveAttribute("data-form-type", "other");
	await expect(patInput).toHaveAttribute("data-secret-visible", "false");
	await expect(patInput).toHaveAttribute(
		"data-secret-mask-mode",
		"native-password",
	);
	await patDialog.getByRole("button", { name: "显示 GitHub PAT" }).click();
	await expect(patInput).toHaveAttribute("type", "text");
	await expect(patInput).toHaveAttribute("data-secret-visible", "true");
	await expect(patInput).toHaveAttribute("data-secret-mask-mode", "plain-text");
	await expect(page.getByRole("link", { name: "去完整设置" })).toHaveAttribute(
		"href",
		"/settings?section=github-pat",
	);
	await patInput.fill("ghp_valid_token_1234");
	await expect(
		patDialog.getByText("GitHub PAT 可用", { exact: true }),
	).toBeVisible();
	await page.getByRole("button", { name: "保存 GitHub PAT" }).click();
	await expect(patDialog).toHaveCount(0);
});

test("reaction buttons stay circular and render count badges outside the trigger", async ({
	page,
}) => {
	await installApiMocks(page, { withReactionFeed: true });

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);

	const reactionFooter = page.locator('[data-reaction-footer="true"]').first();
	const plusOneButton = reactionFooter.locator(
		'[data-reaction-trigger="plus1"]',
	);
	const plusOneIcon = reactionFooter.locator('[data-reaction-icon="plus1"]');
	const plusOneBadge = reactionFooter.locator(
		'[data-reaction-count-badge="plus1"]',
	);
	const laughButton = reactionFooter.getByRole("button", { name: "笑" });

	await expect(plusOneButton).toBeVisible();
	await expect(plusOneIcon).toBeVisible();
	await expect(plusOneBadge).toHaveText("2");
	await expect(laughButton).toBeVisible();
	await expect(
		reactionFooter.locator('[data-reaction-count-badge="laugh"]'),
	).toHaveCount(0);

	const shape = await reactionFooter.evaluate((element) => {
		const footer = element as HTMLElement;
		const button = footer.querySelector<HTMLButtonElement>(
			'[data-reaction-trigger="plus1"]',
		);
		const badge = footer.querySelector<HTMLElement>(
			'[data-reaction-count-badge="plus1"]',
		);
		const icon = footer.querySelector<HTMLImageElement>(
			'[data-reaction-icon="plus1"]',
		);
		if (!button || !badge || !icon) {
			throw new Error("Expected reaction button, badge, and icon");
		}
		const style = window.getComputedStyle(button);
		const buttonRect = button.getBoundingClientRect();
		const badgeRect = badge.getBoundingClientRect();
		const iconRect = icon.getBoundingClientRect();
		return {
			borderRadius: style.borderRadius,
			buttonHeight: buttonRect.height,
			buttonWidth: buttonRect.width,
			badgeHeight: badgeRect.height,
			badgeWidth: badgeRect.width,
			badgeRight: badgeRect.right,
			badgeTop: badgeRect.top,
			buttonRight: buttonRect.right,
			buttonTop: buttonRect.top,
			iconHeight: iconRect.height,
			iconWidth: iconRect.width,
		};
	});
	expect(shape.buttonWidth).toBeGreaterThanOrEqual(35);
	expect(shape.buttonWidth).toBeLessThanOrEqual(41);
	expect(shape.buttonHeight).toBeGreaterThanOrEqual(35);
	expect(shape.buttonHeight).toBeLessThanOrEqual(41);
	expect(Math.abs(shape.buttonWidth - shape.buttonHeight)).toBeLessThanOrEqual(
		1,
	);
	expect(Number.parseFloat(shape.borderRadius)).toBeGreaterThanOrEqual(999);
	expect(shape.iconWidth).toBeGreaterThanOrEqual(17);
	expect(shape.iconWidth).toBeLessThanOrEqual(19);
	expect(shape.iconHeight).toBeGreaterThanOrEqual(17);
	expect(shape.iconHeight).toBeLessThanOrEqual(19);
	expect(shape.badgeHeight).toBeGreaterThanOrEqual(17);
	expect(shape.badgeHeight).toBeLessThanOrEqual(19.5);
	expect(shape.badgeWidth).toBeLessThanOrEqual(24);
	expect(shape.badgeRight).toBeGreaterThan(shape.buttonRight);
	expect(shape.badgeTop).toBeLessThan(shape.buttonTop + 2);
});

test("deep link with zero-padded release id still resolves detail", async ({
	page,
}) => {
	await installApiMocks(page, {
		releaseId: "123",
		detailTitle: "Release 123",
	});

	await page.goto("/?tab=briefs&release=00123");
	await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);

	await expect(page).toHaveURL(/tab=briefs/);
	await expect(page).toHaveURL(/release=123/);
	await expect(
		page.getByRole("heading", { name: "Release 123" }),
	).toBeVisible();
	await expect(page.getByText("#123")).toBeVisible();
});

test("feed smart lane auto generates for visible cards by default", async ({
	page,
}) => {
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 12,
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect
		.poll(
			() =>
				tracker.translationResolveRequests.filter((request) =>
					request.kinds.every((kind) => kind === "release_smart"),
				).length,
		)
		.toBeGreaterThan(0);

	expect(
		tracker.translationResolveRequests.some(
			(request) =>
				request.kinds.every((kind) => kind === "release_smart") &&
				request.entityIds.includes(makeAutoTranslateReleaseId(0)),
		),
	).toBe(true);
	await expect(page.getByRole("heading", { name: "润色 20001" })).toBeVisible();
	await expect(
		page.getByText("润色 release 20001 的主要版本变化。", {
			exact: true,
		}),
	).toBeVisible();
});

test("feed page default selector switches current cards and persists after reload", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		autoTranslateInitialReadyIds: [releaseId],
		smartInitialReadyIds: [releaseId],
	});

	await page.goto("/?tab=releases");
	await expect(
		page.getByRole("heading", { name: `润色 ${releaseId}` }),
	).toBeVisible();

	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();

	await page.reload();
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();
});

test("feed persisted translated page default replays on-demand translation after reload", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		autoTranslateDisabledIds: [releaseId],
		smartInitialReadyIds: [releaseId],
	});

	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.dashboard.releaseDefaultLane",
			"translated",
		);
	});

	await page.goto("/?tab=releases");
	await expect
		.poll(() =>
			tracker.translationResolveRequests.some(
				(request) =>
					request.kinds.every((kind) => kind === "release_summary") &&
					request.entityIds.includes(releaseId),
			),
		)
		.toBe(true);
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();
});

test("feed page default selector triggers on-demand translation for manual lanes", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		autoTranslateDisabledIds: [releaseId],
		smartInitialReadyIds: [releaseId],
	});

	await page.goto("/?tab=releases");
	await expect(
		page.getByRole("heading", { name: `润色 ${releaseId}` }),
	).toBeVisible();

	await page.getByRole("button", { name: "翻译" }).click();
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();
	expect(
		tracker.translationResolveRequests.some(
			(request) =>
				request.kinds.every((kind) => kind === "release_summary") &&
				request.entityIds.includes(releaseId),
		),
	).toBe(true);
});

test("feed page default selector shows original when AI lanes are unavailable", async ({
	page,
}) => {
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		aiDisabledFeed: true,
	});

	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.dashboard.releaseDefaultLane",
			"smart",
		);
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("button", { name: "原文" })).toHaveAttribute(
		"aria-pressed",
		"true",
	);
	await expect(page.getByRole("button", { name: "润色" })).toHaveAttribute(
		"aria-pressed",
		"false",
	);
	await expect(page.getByText("AI 未配置，将只显示原文")).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Release 20001" }),
	).toBeVisible();
});

test("feed page default prefetch stays idle outside release tabs", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		autoTranslateDisabledIds: [releaseId],
	});

	await page.addInitScript(() => {
		window.localStorage.setItem(
			"octo-rill.dashboard.releaseDefaultLane",
			"translated",
		);
	});

	await page.goto("/?tab=inbox");
	await expect(page.getByRole("tab", { name: "收件箱" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await page.waitForTimeout(500);
	expect(
		tracker.translationResolveRequests.some((request) =>
			request.entityIds.includes(releaseId),
		),
	).toBe(false);
});

test("feed smart loading keeps original body visible while the smart option pulses", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
	});
	let releaseSmartResults: (() => void) | null = null;
	const releaseSmartResultsReady = new Promise<void>((resolve) => {
		releaseSmartResults = resolve;
	});
	await page.route("**/api/translate/results", async (route) => {
		const body = route.request().postDataJSON() as {
			items?: Array<{ kind?: string }>;
		};
		if (body.items?.some((item) => item.kind === "release_smart")) {
			await releaseSmartResultsReady;
		}
		await route.fallback();
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByText("lane 1 keeps current-screen work first", { exact: false }),
	).toBeVisible();
	await expect(
		page.locator('[data-feed-lane-trigger="smart"]').first(),
	).toHaveAttribute("data-feed-lane-loading", "true");
	releaseSmartResults?.();
	await expect(
		page.getByRole("heading", { name: `润色 ${releaseId}` }),
	).toBeVisible();
});

test("feed translated tab triggers on-demand translation when data was not preheated", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 6,
		autoTranslateDisabledIds: [releaseId],
		smartInitialReadyIds: [releaseId],
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(
		page.getByRole("heading", { name: `润色 ${releaseId}` }),
	).toBeVisible();

	await page.getByRole("tab", { name: "翻译" }).first().click();
	await expect(
		page.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();
	await expect(
		page.getByText(`这是 release ${releaseId} 的中文摘要。`, { exact: true }),
	).toBeVisible();
	expect(
		tracker.translationResolveRequests.some(
			(request) =>
				request.kinds.every((kind) => kind === "release_summary") &&
				request.entityIds.includes(releaseId),
		),
	).toBe(true);
});

test("mobile release cards hide per-card lane tabs and keep GitHub as a top-right icon link", async ({
	page,
}) => {
	await page.setViewportSize({ width: 390, height: 844 });
	const releaseId = makeAutoTranslateReleaseId(0);
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		autoTranslateInitialReadyIds: [releaseId],
		smartInitialReadyIds: [releaseId],
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);

	const releaseCard = page.locator('[data-slot="card"]').first();
	await expect(releaseCard).toBeVisible();
	await expect(
		releaseCard.getByRole("heading", { name: `润色 ${releaseId}` }),
	).toBeVisible();
	await expect(releaseCard.getByRole("tab", { name: "翻译" })).toHaveCount(0);
	await expect(releaseCard.getByRole("button", { name: "GitHub" })).toHaveCount(
		0,
	);

	const mobileGithubLink = releaseCard.locator(
		'[data-feed-mobile-github-link="true"]',
	);
	await expect(mobileGithubLink).toBeVisible();
	const githubBox = await mobileGithubLink.boundingBox();
	expect(githubBox?.width ?? 0).toBeLessThanOrEqual(36);

	await page.locator("[data-dashboard-mobile-lane-menu-trigger]").click();
	await page.getByRole("menuitemradio", { name: "翻译" }).click();
	await expect(
		releaseCard.getByRole("heading", { name: `发布说明 ${releaseId}` }),
	).toBeVisible();
});

test("feed smart insufficient result collapses the card to version-only mode", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		withAutoTranslateReactions: true,
		autoTranslateFeedCount: 1,
		autoTranslateDisabledIds: [releaseId],
		smartResolveStatuses: {
			[releaseId]: "missing",
		},
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect
		.poll(() =>
			tracker.translationResolveRequests.some(
				(request) =>
					request.kinds.every((kind) => kind === "release_smart") &&
					request.entityIds.includes(releaseId),
			),
		)
		.toBe(true);

	await expect(
		page.getByRole("heading", { name: `Release ${releaseId}` }),
	).toBeVisible();
	const releaseCard = page
		.locator('[data-slot="card"]')
		.filter({
			has: page.getByRole("heading", { name: `Release ${releaseId}` }),
		})
		.first();
	await expect(page.getByRole("tab", { name: "润色" })).toHaveCount(0);
	await expect(
		page.getByText("润色 release 20001 的主要版本变化。"),
	).toHaveCount(0);
	await expect(releaseCard.getByRole("link", { name: "GitHub" })).toBeVisible();
	await expect(releaseCard.getByRole("button", { name: /赞/ })).toBeVisible();
});

test("feed smart localized retryable error falls back to the original card instead of terminal error", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		smartResolveStatuses: {
			[releaseId]: "error",
		},
		smartResolveErrors: {
			[releaseId]: {
				error: "运行时租约失效",
				error_summary: "运行时租约失效",
				error_detail: "runtime_lease_expired",
			},
		},
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect
		.poll(() =>
			tracker.translationResolveRequests.some(
				(request) =>
					request.kinds.every((kind) => kind === "release_smart") &&
					request.entityIds.includes(releaseId),
			),
		)
		.toBe(true);
	await expect(
		page.getByRole("heading", { name: `Release ${releaseId}` }),
	).toBeVisible();
	await expect(page.getByText("润色失败", { exact: true })).toHaveCount(0);
	await expect(page.getByRole("button", { name: "重试润色" })).toHaveCount(0);
});

test("feed smart retry treats insufficient result as a successful collapse", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		smartInitialErrorIds: [releaseId],
		smartResolveStatuses: {
			[releaseId]: "missing",
		},
	});

	await page.goto("/?tab=releases");
	await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
		"aria-selected",
		"true",
	);
	await expect(page.getByText("润色失败", { exact: true })).toBeVisible();

	await page.getByRole("button", { name: "重试润色" }).click();

	await expect(page.getByText("no_valuable_version_info")).toHaveCount(0);
	await expect(page.getByText("润色失败", { exact: true })).toHaveCount(0);
	await expect(
		page.getByRole("heading", { name: `Release ${releaseId}` }),
	).toBeVisible();
	await expect(page.getByRole("tab", { name: "润色" })).toHaveCount(0);
});

test("feed smart retry button spins and disables while request is in flight", async ({
	page,
}) => {
	const releaseId = makeAutoTranslateReleaseId(0);
	const tracker = await installApiMocks(page, {
		withAutoTranslateFeed: true,
		autoTranslateFeedCount: 1,
		smartInitialErrorIds: [releaseId],
		smartResolveStatuses: {
			[releaseId]: "ready",
		},
		smartResolveDelayMs: 3000,
	});

	await page.goto("/?tab=releases");
	const retryButton = page.getByRole("button", { name: "重试润色" });
	await expect(retryButton).toBeVisible();
	await expect(retryButton).toBeEnabled();

	await retryButton.dblclick();

	await expect
		.poll(
			() =>
				tracker.translationResolveRequests.filter(
					(request) =>
						request.kinds.includes("release_smart") &&
						request.entityIds.includes(releaseId),
				).length,
		)
		.toBe(1);
	await expect(retryButton).toBeDisabled();
	await expect(retryButton).toHaveAttribute("aria-busy", "true");
	await expect(retryButton.locator("svg").first()).toHaveClass(/animate-spin/);
	await expect(retryButton).toHaveText("重试润色");

	await expect(page.getByText("润色失败", { exact: true })).toHaveCount(0);
	await expect(
		page.getByText(`润色 release ${releaseId} 的主要版本变化。`),
	).toBeVisible();
});

test.describe("localized timestamps", () => {
	test.describe("non-DST browser timezone", () => {
		test.use({ timezoneId: "Asia/Shanghai" });

		test("release feed cards render timestamps in the browser timezone", async ({
			page,
		}) => {
			await installApiMocks(page, { withReactionFeed: true });

			await page.goto("/?tab=releases");
			await expect(page.getByRole("tab", { name: "发布" })).toHaveAttribute(
				"aria-selected",
				"true",
			);
			await expect(
				page.getByText("2026-02-22 19:22:33", { exact: true }),
			).toBeVisible();
		});

		test("release detail cards render published_at in the browser timezone", async ({
			page,
		}) => {
			await installApiMocks(page);

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			await expect(
				page.getByText("#123 · 2026-02-22 19:22:33", { exact: true }),
			).toBeVisible();
		});

		test("brief windows and markdown timestamps stay correct in a non-DST timezone", async ({
			page,
		}) => {
			await installApiMocks(page, {
				briefDate: "2026-07-23",
				briefWindowStart: "2026-07-22T00:00:00Z",
				briefWindowEnd: "2026-07-23T00:00:00Z",
				briefMarkdown:
					"## 项目更新\n\n### [owner/repo](https://github.com/owner/repo)\n\n- [repo/v1.2.3](/?tab=briefs&release=123) · 2026-07-22T11:22:33Z · [GitHub Release](https://github.com/owner/repo/releases/tag/v1.2.3)\n  - 校验浏览器时区下的 brief 时间戳展示。\n\n## 获星与关注\n\n### 获星\n\n- [owner/repo](https://github.com/owner/repo)：[@alice](https://github.com/alice)\n\n### 关注\n\n- [@bob](https://github.com/bob)",
				briefCreatedAt: "2026-07-23T08:00:00Z",
			});

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			const briefPanel = page.getByRole("tabpanel", { name: "日报" });
			await expect(
				page.getByText(
					"#2026-07-23 · 2026-07-22 08:00:00 → 2026-07-23 08:00:00",
					{
						exact: true,
					},
				),
			).toBeVisible();
			await page.getByRole("button", { name: "关闭" }).click();
			await expect(briefPanel).toContainText("2026-07-22 19:22:33");
			await expect(briefPanel).not.toContainText("2026-07-22T11:22:33Z");
		});
	});

	test.describe("DST browser timezone", () => {
		test.use({ timezoneId: "America/New_York" });

		test("brief windows and markdown timestamps use the browser DST offset", async ({
			page,
		}) => {
			await installApiMocks(page, {
				briefDate: "2026-07-23",
				briefWindowStart: "2026-07-22T00:00:00Z",
				briefWindowEnd: "2026-07-23T00:00:00Z",
				briefMarkdown:
					"## 项目更新\n\n### [owner/repo](https://github.com/owner/repo)\n\n- [repo/v1.2.3](/?tab=briefs&release=123) · 2026-07-22T11:22:33Z · [GitHub Release](https://github.com/owner/repo/releases/tag/v1.2.3)\n  - 校验浏览器时区下的 brief 时间戳展示。\n\n## 获星与关注\n\n### 获星\n\n- [owner/repo](https://github.com/owner/repo)：[@alice](https://github.com/alice)\n\n### 关注\n\n- [@bob](https://github.com/bob)",
				briefCreatedAt: "2026-07-23T08:00:00Z",
			});

			await page.goto("/?tab=briefs&release=123");
			await expect(page.getByText(/Cannot read properties/i)).toHaveCount(0);
			const briefPanel = page.getByRole("tabpanel", { name: "日报" });
			await expect(
				page.getByText(
					"#2026-07-23 · 2026-07-21 20:00:00 → 2026-07-22 20:00:00",
					{
						exact: true,
					},
				),
			).toBeVisible();
			await page.getByRole("button", { name: "关闭" }).click();
			await expect(briefPanel).toContainText("2026-07-22 07:22:33");
			await expect(briefPanel).not.toContainText("2026-07-22T11:22:33Z");
		});
	});
});
