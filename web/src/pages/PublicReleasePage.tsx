import {
	ArrowLeft,
	ChevronDown,
	ChevronUp,
	ExternalLink,
	RefreshCcw,
} from "lucide-react";
import { useQuery } from "@tanstack/react-query";
import { useWindowVirtualizer } from "@tanstack/react-virtual";
import {
	lazy,
	Suspense,
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import {
	ApiError,
	apiGetReactionTokenStatus,
	type PublicReleaseHighlight,
	type PublicReleaseGap,
	type PublicReleaseListItem,
	type PublicReleasePendingResponse,
	type PublicReleaseResponse,
	type ReleaseDetailResponse,
	apiGetPublicRepoReleaseDetail,
	apiGetPublicRepoReleaseContent,
	apiGetPublicRepoReleases,
	apiPostJson,
} from "@/api";
import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import { InternalLink } from "@/lib/internalNavigation";
import type {
	FeedLane,
	FeedReactionRefreshResponse,
	ReactionContent,
	ReleaseFeedItem,
	ReleaseReactions,
	ToggleReleaseReactionResponse,
} from "@/feed/types";
import {
	appendPublicReleaseHighlightParams,
	publicReleaseHighlightSearch,
	type PublicReleaseHighlightSelection,
} from "@/publicRelease/routeState";
import {
	DASHBOARD_QUERY_STALE_MS,
	dashboardReactionTokenQueryKey,
} from "@/query/dashboardQueryKeys";
import { isReactionTokenUsable } from "@/settings/reactionTokenEditor";
import { cn } from "@/lib/utils";
import { buildVersionReleaseHref } from "@/version/versionReleaseLink";
import { useVersionMonitor } from "@/version/versionMonitor";

const ReleaseFeedCard = lazy(async () => {
	const module = await import("@/feed/FeedItemCard");
	return { default: module.ReleaseFeedCard };
});

const PUBLIC_RELEASE_LIST_BODY_MAX_CHARS = 2800;
const PUBLIC_RELEASE_PAGE_SIZE = 6;
const PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE = 30;
const PUBLIC_RELEASE_REACTION_BATCH_SIZE = 100;

type PublicReleaseReactionControls = {
	enabled: boolean;
	byReleaseId: Record<string, ReleaseReactions>;
	availableReleaseIds: Set<string>;
	busyReleaseIds: Set<string>;
	errorByReleaseId: Record<string, string>;
	onToggle: (releaseId: string, content: ReactionContent) => void;
};

type LoadState =
	| { status: "loading" }
	| { status: "pending"; pending: PublicReleasePendingResponse }
	| {
			status: "list";
			data: Extract<PublicReleaseResponse, { status: "ready" }>;
	  }
	| { status: "detail"; data: ReleaseDetailResponse }
	| { status: "error"; message: string; code?: string };

function isPendingResponse(
	value: unknown,
): value is PublicReleasePendingResponse {
	return (
		typeof value === "object" &&
		value !== null &&
		"status" in value &&
		(value as { status?: unknown }).status === "pending_sync"
	);
}

function usePublicReleaseReactionControls(
	items: PublicReleaseListItem[],
): PublicReleaseReactionControls {
	const auth = useAuthBootstrap();
	const userId = auth.me?.user.id ?? null;
	const reactionTokenQuery = useQuery({
		queryKey: dashboardReactionTokenQueryKey(userId ?? "anonymous"),
		queryFn: apiGetReactionTokenStatus,
		enabled: auth.isAuthenticated && userId !== null,
		staleTime: DASHBOARD_QUERY_STALE_MS,
		retry: false,
	});
	const [reactionAccessBlocked, setReactionAccessBlocked] = useState(false);
	const [byReleaseId, setByReleaseId] = useState<
		Record<string, ReleaseReactions>
	>({});
	const [availableReleaseIds, setAvailableReleaseIds] = useState<Set<string>>(
		() => new Set(),
	);
	const [busyReleaseIds, setBusyReleaseIds] = useState<Set<string>>(
		() => new Set(),
	);
	const [errorByReleaseId, setErrorByReleaseId] = useState<
		Record<string, string>
	>({});
	const releaseIdSignature = useMemo(
		() =>
			Array.from(new Set(items.map((item) => item.release_id)))
				.sort()
				.join("|"),
		[items],
	);
	const requestedReleaseIdsRef = useRef(new Set<string>());
	const enabled =
		auth.isAuthenticated &&
		!reactionAccessBlocked &&
		isReactionTokenUsable(
			reactionTokenQuery.data ?? {
				configured: false,
				masked_token: null,
				owner: null,
				check: { state: "idle", message: null, checked_at: null },
			},
		);

	useEffect(() => {
		setReactionAccessBlocked(false);
		setByReleaseId({});
		setAvailableReleaseIds(new Set());
		setBusyReleaseIds(new Set());
		setErrorByReleaseId({});
		requestedReleaseIdsRef.current = new Set();
	}, [userId]);

	useEffect(() => {
		if (!enabled || !releaseIdSignature) return;
		let cancelled = false;
		const releaseIds = releaseIdSignature
			.split("|")
			.filter((releaseId) => !requestedReleaseIdsRef.current.has(releaseId));
		if (releaseIds.length === 0) return;
		for (const releaseId of releaseIds) {
			requestedReleaseIdsRef.current.add(releaseId);
		}
		const batches = Array.from(
			{
				length: Math.ceil(
					releaseIds.length / PUBLIC_RELEASE_REACTION_BATCH_SIZE,
				),
			},
			(_, index) =>
				releaseIds.slice(
					index * PUBLIC_RELEASE_REACTION_BATCH_SIZE,
					(index + 1) * PUBLIC_RELEASE_REACTION_BATCH_SIZE,
				),
		);

		void Promise.all(
			batches.map((batch) =>
				apiPostJson<FeedReactionRefreshResponse>(
					"/api/feed/reactions/refresh",
					{ release_ids: batch },
				),
			),
		)
			.then((responses) => {
				if (cancelled) return;
				const refreshed = responses.flatMap((response) => response.items);
				setByReleaseId((current) => ({
					...current,
					...Object.fromEntries(
						refreshed.map((item) => [item.release_id, item.reactions]),
					),
				}));
				setAvailableReleaseIds(
					(current) =>
						new Set([...current, ...refreshed.map((item) => item.release_id)]),
				);
			})
			.catch((error) => {
				for (const releaseId of releaseIds) {
					requestedReleaseIdsRef.current.delete(releaseId);
				}
				if (
					error instanceof ApiError &&
					(error.code === "pat_invalid" || error.code === "pat_required")
				) {
					setReactionAccessBlocked(true);
				}
			});

		return () => {
			cancelled = true;
		};
	}, [enabled, releaseIdSignature]);

	const onToggle = useCallback(
		(releaseId: string, content: ReactionContent) => {
			if (!enabled || busyReleaseIds.has(releaseId)) return;
			setBusyReleaseIds((current) => new Set(current).add(releaseId));
			setErrorByReleaseId((current) => {
				if (!(releaseId in current)) return current;
				const next = { ...current };
				delete next[releaseId];
				return next;
			});

			void apiPostJson<ToggleReleaseReactionResponse>(
				"/api/release/reactions/toggle",
				{ release_id: releaseId, content },
			)
				.then((response) => {
					setByReleaseId((current) => ({
						...current,
						[response.release_id]: response.reactions,
					}));
				})
				.catch((error) => {
					if (
						error instanceof ApiError &&
						(error.code === "pat_invalid" || error.code === "pat_required")
					) {
						setReactionAccessBlocked(true);
						return;
					}
					if (error instanceof ApiError && error.code === "not_found") {
						setAvailableReleaseIds((current) => {
							const next = new Set(current);
							next.delete(releaseId);
							return next;
						});
						return;
					}
					setErrorByReleaseId((current) => ({
						...current,
						[releaseId]:
							error instanceof Error ? error.message : "表情反应更新失败",
					}));
				})
				.finally(() => {
					setBusyReleaseIds((current) => {
						const next = new Set(current);
						next.delete(releaseId);
						return next;
					});
				});
		},
		[busyReleaseIds, enabled],
	);

	return {
		enabled,
		byReleaseId,
		availableReleaseIds,
		busyReleaseIds,
		errorByReleaseId,
		onToggle,
	};
}

function releaseTitle(item: Pick<PublicReleaseListItem, "name" | "tag_name">) {
	return item.name?.trim() || item.tag_name;
}

function activeHighlightSelector(highlight?: PublicReleaseHighlight) {
	const activeReleaseId = highlight?.active_release_id;
	if (!activeReleaseId) return undefined;
	return (
		highlight.resolved.find((target) => target.release_id === activeReleaseId)
			?.selector ?? `id:${activeReleaseId}`
	);
}

function mergePaginatedHighlight(
	current: PublicReleaseHighlight | undefined,
	incoming: PublicReleaseHighlight | undefined,
) {
	if (!current) return incoming;
	if (!incoming) return current;
	return {
		...incoming,
		active_release_id: current.active_release_id,
		active_index: current.active_index,
	};
}

function applyActiveHighlight(
	items: PublicReleaseListItem[],
	highlight: PublicReleaseHighlight | undefined,
) {
	if (!highlight?.active_release_id) return items;
	return items.map((item) => ({
		...item,
		is_active_highlight: item.release_id === highlight.active_release_id,
	}));
}

function PulseBlock(props: { className?: string; rounded?: string }) {
	const { className, rounded = "rounded-2xl" } = props;
	return (
		<div
			className={cn("bg-muted/70 animate-pulse", rounded, className)}
			data-testid="public-release-skeleton-block"
		/>
	);
}

export function PublicReleasePage(props: {
	owner: string;
	repo: string;
	tag?: string | null;
	highlight?: PublicReleaseHighlightSelection;
}) {
	const { owner, repo, tag = null, highlight = null } = props;
	const [state, setState] = useState<LoadState>({ status: "loading" });
	const [loadingMore, setLoadingMore] = useState(false);
	const [loadingNewer, setLoadingNewer] = useState(false);
	const [loadingGap, setLoadingGap] = useState<string | null>(null);
	const [appendError, setAppendError] = useState<string | null>(null);
	const [selectedLane, setSelectedLane] = useState<FeedLane>("smart");
	const initialLoadKeyRef = useRef<string | null>(null);
	const isHighlightMode = highlight !== null;
	const initialLoadKey = JSON.stringify({ owner, repo, tag, highlight });
	const reactionControls = usePublicReleaseReactionControls(
		state.status === "list" ? state.data.items : [],
	);

	const highlightRequest = useMemo(() => {
		if (!highlight) return {};
		if (highlight.mode === "discrete") {
			return {
				highlight: highlight.selectors,
				highlight_active: highlight.active,
			};
		}
		if (highlight.mode === "range") {
			return {
				highlight_start: highlight.start,
				highlight_end: highlight.end,
				highlight_active: highlight.active,
			};
		}
		return {
			highlight: highlight.selectors,
			highlight_start: highlight.start,
			highlight_end: highlight.end,
			highlight_active: highlight.active,
		};
	}, [highlight]);

	const buildHighlightRequest = useCallback(
		(
			direction?: "older" | "newer",
			cursor?: string | null,
			activeSelector?: string,
		) => ({
			owner,
			repo,
			source: "page" as const,
			limit: isHighlightMode
				? PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE
				: PUBLIC_RELEASE_PAGE_SIZE,
			cursor,
			direction,
			content: "polished" as const,
			include_original: true,
			...highlightRequest,
			...(activeSelector ? { highlight_active: activeSelector } : {}),
		}),
		[highlightRequest, isHighlightMode, owner, repo],
	);

	const load = useCallback(async () => {
		try {
			setState((current) =>
				current.status === "error" ? { status: "loading" } : current,
			);
			setAppendError(null);
			const data = tag
				? await apiGetPublicRepoReleaseDetail({
						owner,
						repo,
						tag,
						source: "page",
						content: "all",
						include_original: true,
					})
				: await apiGetPublicRepoReleases({
						...buildHighlightRequest(),
					});
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
			} else if (tag) {
				setState({ status: "detail", data: data as ReleaseDetailResponse });
			} else {
				setState({
					status: "list",
					data: data as Extract<PublicReleaseResponse, { status: "ready" }>,
				});
			}
		} catch (err) {
			if (err instanceof ApiError) {
				setState({ status: "error", message: err.message, code: err.code });
				return;
			}
			setState({ status: "error", message: "公开 Release 加载失败" });
		}
	}, [buildHighlightRequest, owner, repo, tag]);

	const mergeItems = useCallback(
		(current: PublicReleaseListItem[], incoming: PublicReleaseListItem[]) => {
			const byId = new Map(current.map((item) => [item.release_id, item]));
			for (const item of incoming) {
				byId.set(item.release_id, { ...byId.get(item.release_id), ...item });
			}
			return Array.from(byId.values()).sort((left, right) => {
				const ts = (right.published_at ?? "").localeCompare(
					left.published_at ?? "",
				);
				if (ts !== 0) return ts;
				return right.release_id.localeCompare(left.release_id, undefined, {
					numeric: true,
				});
			});
		},
		[],
	);

	const loadMore = useCallback(async () => {
		if (
			tag ||
			loadingMore ||
			state.status !== "list" ||
			!state.data.next_cursor
		) {
			return;
		}
		setLoadingMore(true);
		setAppendError(null);
		try {
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest(
					"older",
					state.data.next_cursor,
					activeHighlightSelector(state.data.highlight),
				),
			});
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (current.status !== "list") {
					return current;
				}
				const highlight = mergePaginatedHighlight(
					current.data.highlight,
					data.highlight,
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: applyActiveHighlight(
							mergeItems(current.data.items, data.items),
							highlight,
						),
						next_cursor: data.next_cursor,
						previous_cursor: current.data.previous_cursor,
						highlight,
					},
				};
			});
		} catch (err) {
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingMore(false);
		}
	}, [buildHighlightRequest, loadingMore, mergeItems, state, tag]);

	const loadNewer = useCallback(async () => {
		if (
			tag ||
			loadingNewer ||
			state.status !== "list" ||
			!state.data.previous_cursor
		) {
			return;
		}
		setLoadingNewer(true);
		setAppendError(null);
		try {
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest(
					"newer",
					state.data.previous_cursor,
					activeHighlightSelector(state.data.highlight),
				),
			});
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (current.status !== "list") return current;
				const highlight = mergePaginatedHighlight(
					current.data.highlight,
					data.highlight,
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: applyActiveHighlight(
							mergeItems(current.data.items, data.items),
							highlight,
						),
						previous_cursor: data.previous_cursor,
						next_cursor: current.data.next_cursor ?? data.next_cursor,
						highlight,
					},
				};
			});
		} catch (err) {
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingNewer(false);
		}
	}, [buildHighlightRequest, loadingNewer, mergeItems, state, tag]);

	const loadGap = useCallback(
		async (gap: PublicReleaseGap) => {
			if (loadingGap || state.status !== "list") return;
			setLoadingGap(gap.newer_cursor);
			setAppendError(null);
			try {
				const data = await apiGetPublicRepoReleases({
					...buildHighlightRequest(
						"older",
						gap.newer_cursor,
						activeHighlightSelector(state.data.highlight),
					),
					until_cursor: gap.older_cursor,
				});
				if (isPendingResponse(data)) return;
				setState((current) => {
					if (current.status !== "list") return current;
					const highlight = mergePaginatedHighlight(
						current.data.highlight,
						data.highlight,
					);
					const existingIds = new Set(
						current.data.items.map((item) => item.release_id),
					);
					const inserted = data.items.filter(
						(item) => !existingIds.has(item.release_id),
					).length;
					const reachedOlderBoundary = data.items.some((item) =>
						gap.older_cursor.endsWith(`|${item.release_id}`),
					);
					const nextGaps = (current.data.gaps ?? []).flatMap((candidate) => {
						if (candidate.newer_cursor !== gap.newer_cursor) return [candidate];
						if (reachedOlderBoundary || !data.next_cursor) return [];
						return [
							{
								...candidate,
								newer_cursor: data.next_cursor,
								remaining_count: Math.max(
									0,
									candidate.remaining_count - inserted,
								),
							},
						];
					});
					return {
						status: "list",
						data: {
							...current.data,
							items: applyActiveHighlight(
								mergeItems(current.data.items, data.items),
								highlight,
							),
							gaps: nextGaps,
							highlight,
						},
					};
				});
			} catch (err) {
				setAppendError(err instanceof Error ? err.message : String(err));
			} finally {
				setLoadingGap(null);
			}
		},
		[buildHighlightRequest, loadingGap, mergeItems, state.status],
	);

	const hydrateItems = useCallback(
		(
			items: Array<Pick<PublicReleaseListItem, "release_id" | "translated">>,
		) => {
			setState((current) => {
				if (current.status !== "list") return current;
				const translatedById = new Map(
					items.map((item) => [item.release_id, item.translated]),
				);
				return {
					status: "list",
					data: {
						...current.data,
						items: current.data.items.map((item) =>
							translatedById.has(item.release_id)
								? {
										...item,
										translated: translatedById.get(item.release_id) ?? null,
									}
								: item,
						),
					},
				};
			});
		},
		[],
	);

	const activateHighlight = useCallback((releaseId: string, index: number) => {
		setState((current) => {
			if (current.status !== "list" || !current.data.highlight) return current;
			return {
				status: "list",
				data: {
					...current.data,
					highlight: {
						...current.data.highlight,
						active_release_id: releaseId,
						active_index: index,
					},
					items: current.data.items.map((item) => ({
						...item,
						is_active_highlight: item.release_id === releaseId,
					})),
				},
			};
		});
	}, []);

	useEffect(() => {
		if (initialLoadKeyRef.current === initialLoadKey) return;
		initialLoadKeyRef.current = initialLoadKey;
		void load();
	}, [initialLoadKey, load]);

	useEffect(() => {
		if (state.status !== "pending") return;
		const delay = Math.max(15, state.pending.retry_after_seconds) * 1000;
		const timer = window.setTimeout(() => void load(), delay);
		return () => window.clearTimeout(timer);
	}, [load, state]);

	const repoFullName = useMemo(() => `${owner}/${repo}`, [owner, repo]);
	const repoVisual =
		state.status === "list" ? state.data.items[0]?.repo_visual : null;
	const highlightedListHref = useMemo(() => {
		if (!tag || !highlight) return null;
		const params = appendPublicReleaseHighlightParams(
			new URLSearchParams(),
			highlight,
		);
		return `/${encodeURIComponent(owner)}/${encodeURIComponent(repo)}/releases?${params.toString()}`;
	}, [highlight, owner, repo, tag]);

	return (
		<main className="min-h-dvh bg-background text-foreground">
			<div className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col px-4 py-5 sm:px-6 lg:px-8">
				<div className="flex min-h-full flex-col">
					<header className="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
						<InternalLink
							href="/"
							to="/"
							className="inline-flex items-center gap-3"
						>
							<BrandLogo variant="wordmark" className="h-7 sm:h-8" />
						</InternalLink>
						<Button asChild variant="outline" size="sm">
							<a
								href={`https://github.com/${owner}/${repo}/releases`}
								target="_blank"
								rel="noreferrer"
							>
								<ExternalLink className="size-4" />
								GitHub
							</a>
						</Button>
					</header>

					{highlightedListHref ? (
						<div className="pt-4">
							<InternalLink
								href={highlightedListHref}
								to="/$owner/$repo/releases"
								params={{ owner, repo }}
								search={publicReleaseHighlightSearch(highlight)}
								className="inline-flex items-center gap-2 text-sm text-muted-foreground underline-offset-4 hover:text-foreground hover:underline"
							>
								<ArrowLeft className="size-4" />
								返回高亮列表
							</InternalLink>
						</div>
					) : null}

					{tag ? null : (
						<section className="py-6" data-testid="public-release-title-band">
							<div className="flex flex-wrap items-center gap-x-6 gap-y-3">
								<RepoIdentity
									repoFullName={repoFullName}
									repoVisual={repoVisual}
									labelAs="h1"
									className="max-w-full shrink-0"
									labelClassName="break-words text-3xl font-semibold tracking-normal"
									visualClassName="size-10"
								/>
								{state.status === "list" ? (
									<div
										className="ml-auto shrink-0"
										data-testid="public-release-page-lane"
									>
										<FeedPageLaneSelector
											value={selectedLane}
											onValueChange={setSelectedLane}
										/>
									</div>
								) : null}
							</div>
						</section>
					)}

					{state.status === "loading" ? (
						<PublicReleaseLoadingSkeleton hasTag={Boolean(tag)} />
					) : null}

					{state.status === "pending" ? (
						<WaitingCard
							title="Release 数据同步中"
							description="这个仓库的 Release 数据还在同步中，稍后会自动重试。"
							retryAfter={state.pending.retry_after_seconds}
							statusLabel="同步中"
							onRetry={load}
						/>
					) : null}

					{state.status === "error" ? (
						<Card>
							<CardHeader>
								<CardTitle>暂时无法展示</CardTitle>
								<CardDescription>
									{state.code ? `${state.code}: ` : ""}
									{state.message || "请求失败，请稍后重试。"}
								</CardDescription>
							</CardHeader>
							<CardContent>
								<Button type="button" onClick={() => void load()}>
									<RefreshCcw className="size-4" />
									重试
								</Button>
							</CardContent>
						</Card>
					) : null}

					{state.status === "list" ? (
						<ReleaseList
							owner={owner}
							repo={repo}
							items={state.data.items}
							highlight={state.data.highlight}
							gaps={state.data.gaps}
							hasMore={Boolean(state.data.next_cursor)}
							hasNewer={Boolean(state.data.previous_cursor)}
							loadingMore={loadingMore}
							loadingNewer={loadingNewer}
							appendError={appendError}
							onLoadMore={loadMore}
							onLoadNewer={loadNewer}
							onLoadGap={loadGap}
							loadingGap={loadingGap}
							highlightSelection={highlight}
							selectedLane={selectedLane}
							onHydrateItems={hydrateItems}
							onActivateHighlight={activateHighlight}
							reactionControls={reactionControls}
						/>
					) : null}

					{state.status === "detail" ? (
						<ReleaseDetail detail={state.data} />
					) : null}

					<PublicReleaseFooter owner={owner} repo={repo} />
				</div>
			</div>
		</main>
	);
}

function PublicReleaseLoadingSkeleton(props: { hasTag: boolean }) {
	const { hasTag } = props;

	if (hasTag) {
		return (
			<section
				className="space-y-4"
				aria-label="Release loading skeleton"
				data-testid="public-release-loading-skeleton"
			>
				<div className="rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6">
					<div className="flex flex-wrap items-start justify-between gap-4">
						<div className="min-w-0 flex-1 space-y-3">
							<PulseBlock className="h-5 w-28 rounded-full" />
							<PulseBlock className="h-9 w-3/4 max-w-xl rounded-3xl" />
							<PulseBlock className="h-4 w-48 rounded-full" />
						</div>
						<div className="flex gap-2">
							<PulseBlock className="h-10 w-20 rounded-xl" />
							<PulseBlock className="h-10 w-20 rounded-xl" />
							<PulseBlock className="h-10 w-20 rounded-xl" />
						</div>
					</div>
					<div className="mt-6 space-y-3">
						<PulseBlock className="h-4 w-full rounded-full" />
						<PulseBlock className="h-4 w-[94%] rounded-full" />
						<PulseBlock className="h-4 w-[88%] rounded-full" />
						<PulseBlock className="h-4 w-[76%] rounded-full" />
						<PulseBlock className="h-40 w-full rounded-[24px]" />
					</div>
				</div>
			</section>
		);
	}

	return (
		<section
			className="space-y-4"
			aria-label="Release loading skeleton"
			data-testid="public-release-loading-skeleton"
		>
			<div className="flex flex-wrap items-center justify-between gap-3 rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6">
				<div className="space-y-3">
					<PulseBlock className="h-4 w-24 rounded-full" />
					<PulseBlock className="h-8 w-48 rounded-3xl" />
				</div>
				<div className="flex gap-2">
					<PulseBlock className="h-10 w-20 rounded-xl" />
					<PulseBlock className="h-10 w-20 rounded-xl" />
					<PulseBlock className="h-10 w-20 rounded-xl" />
				</div>
			</div>

			{Array.from({ length: 3 }, (_, index) => (
				<div
					key={`public-release-loading-card-${index}`}
					className="rounded-[28px] border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6"
				>
					<div className="flex flex-wrap items-start justify-between gap-4">
						<div className="min-w-0 flex-1 space-y-3">
							<div className="flex items-center gap-3">
								<PulseBlock className="size-11 rounded-full" />
								<div className="min-w-0 flex-1 space-y-2">
									<PulseBlock className="h-4 w-44 rounded-full" />
									<PulseBlock className="h-3 w-28 rounded-full" />
								</div>
							</div>
							<PulseBlock className="h-8 w-3/4 rounded-3xl" />
						</div>
						<div className="flex gap-2">
							<PulseBlock className="h-9 w-16 rounded-xl" />
							<PulseBlock className="h-9 w-16 rounded-xl" />
							<PulseBlock className="h-9 w-16 rounded-xl" />
						</div>
					</div>
					<div className="mt-6 space-y-3">
						<PulseBlock className="h-4 w-full rounded-full" />
						<PulseBlock className="h-4 w-[95%] rounded-full" />
						<PulseBlock className="h-4 w-[82%] rounded-full" />
						<PulseBlock className="h-24 w-full rounded-[22px]" />
					</div>
				</div>
			))}
		</section>
	);
}

function PublicReleaseFooter(props: { owner: string; repo: string }) {
	const year = new Date().getFullYear();
	const repositoryHref = `https://github.com/${props.owner}/${props.repo}`;
	const { loadedVersion } = useVersionMonitor();
	const versionReleaseHref = buildVersionReleaseHref(loadedVersion);

	return (
		<footer className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t pt-4 pb-1 font-mono text-[11px] text-muted-foreground">
			<span>© {year} Ivan Li</span>
			<div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
				<a
					href={repositoryHref}
					target="_blank"
					rel="noreferrer"
					className="inline-flex items-center gap-1.5 underline-offset-4 hover:text-foreground hover:underline"
				>
					<AuthProviderIcon provider="github" className="size-3" />
					GitHub
				</a>
				{versionReleaseHref ? (
					<a
						href={versionReleaseHref}
						className="underline-offset-4 hover:text-foreground hover:underline"
					>
						Version {loadedVersion}
					</a>
				) : (
					<span>Version {loadedVersion}</span>
				)}
			</div>
		</footer>
	);
}

function WaitingCard(props: {
	title: string;
	description?: string;
	retryAfter?: number;
	statusLabel?: string;
	onRetry: () => void;
}) {
	return (
		<Card>
			<CardHeader>
				<CardTitle>{props.title}</CardTitle>
				<CardDescription>
					{props.description ??
						"正在读取这个仓库的已知 Release 数据；若本地已有共享缓存，页面会直接显示结果。"}
				</CardDescription>
			</CardHeader>
			<CardContent className="flex flex-wrap items-center gap-3">
				<Badge
					variant="secondary"
					className="max-w-full shrink flex-wrap justify-start whitespace-normal text-left"
				>
					<span>{props.statusLabel ?? "正在读取"}</span>
					{props.retryAfter ? (
						<span className="shrink-0 whitespace-nowrap">
							· 约 {props.retryAfter}s 后重试
						</span>
					) : null}
				</Badge>
				<Button type="button" variant="outline" onClick={props.onRetry}>
					<RefreshCcw className="size-4" />
					立即重试
				</Button>
			</CardContent>
		</Card>
	);
}

function ReleaseList(props: {
	owner: string;
	repo: string;
	items: PublicReleaseListItem[];
	highlight?: PublicReleaseHighlight;
	gaps?: PublicReleaseGap[];
	highlightSelection: PublicReleaseHighlightSelection;
	selectedLane: FeedLane;
	reactionControls: PublicReleaseReactionControls;
	hasMore: boolean;
	hasNewer: boolean;
	loadingMore: boolean;
	loadingNewer: boolean;
	loadingGap: string | null;
	appendError: string | null;
	onLoadMore: () => Promise<void>;
	onLoadNewer: () => Promise<void>;
	onLoadGap: (gap: PublicReleaseGap) => Promise<void>;
	onHydrateItems: (
		items: Array<Pick<PublicReleaseListItem, "release_id" | "translated">>,
	) => void;
	onActivateHighlight: (releaseId: string, index: number) => void;
}) {
	const listRef = useRef<HTMLDivElement | null>(null);
	const focusedHighlightSignatureRef = useRef<string | null>(null);
	const hydratedTranslatedRef = useRef(new Set<string>());

	type VirtualRow =
		| { kind: "release"; item: PublicReleaseListItem }
		| { kind: "gap"; gap: PublicReleaseGap };

	const rows = useMemo<VirtualRow[]>(() => {
		const gapByNewerId = new Map<string, PublicReleaseGap>();
		for (const gap of props.gaps ?? []) {
			gapByNewerId.set(gap.newer_cursor.split("|").at(-1) ?? "", gap);
		}
		const result: VirtualRow[] = [];
		for (const item of props.items) {
			result.push({ kind: "release", item });
			const gap = gapByNewerId.get(item.release_id);
			if (gap) result.push({ kind: "gap", gap });
		}
		return result;
	}, [props.gaps, props.items]);

	const virtualizer = useWindowVirtualizer({
		count: rows.length,
		estimateSize: (index) => (rows[index]?.kind === "gap" ? 72 : 420),
		overscan: 4,
		scrollMargin: listRef.current?.offsetTop ?? 0,
		getItemKey: (index) => {
			const row = rows[index];
			return row?.kind === "release"
				? `release-${row.item.release_id}`
				: `gap-${row?.gap.newer_cursor ?? index}`;
		},
	});

	const virtualItems = virtualizer.getVirtualItems();
	const visibleReleaseIds = virtualItems
		.map((virtualItem) => rows[virtualItem.index])
		.filter(
			(row): row is Extract<VirtualRow, { kind: "release" }> =>
				row?.kind === "release",
		)
		.map((row) => row.item.release_id);
	const visibleReleaseSignature = visibleReleaseIds.join(",");

	useEffect(() => {
		if (props.selectedLane !== "translated" || !visibleReleaseSignature) return;
		const ids = visibleReleaseIds.filter(
			(id) => !hydratedTranslatedRef.current.has(id),
		);
		if (ids.length === 0) return;
		for (const id of ids) hydratedTranslatedRef.current.add(id);
		void apiGetPublicRepoReleaseContent({
			owner: props.owner,
			repo: props.repo,
			release_ids: ids.slice(0, 30),
			content: "translated",
		})
			.then((response) =>
				props.onHydrateItems(
					response.items.map((item) => ({
						release_id: item.release_id,
						translated: item.translated,
					})),
				),
			)
			.catch(() => {
				for (const id of ids) hydratedTranslatedRef.current.delete(id);
			});
	}, [
		props.onHydrateItems,
		props.owner,
		props.repo,
		props.selectedLane,
		visibleReleaseIds,
		visibleReleaseSignature,
	]);

	useLayoutEffect(() => {
		if (!props.highlight || props.items.length === 0) return;
		const signature = [
			props.highlight.mode,
			...props.highlight.requested,
			props.highlight.active_release_id ?? "",
		].join(":");
		if (focusedHighlightSignatureRef.current === signature) return;
		const activeId =
			props.highlight.active_release_id ??
			props.items.find((item) => item.is_highlighted)?.release_id;
		const index = rows.findIndex(
			(row) => row.kind === "release" && row.item.release_id === activeId,
		);
		if (index < 0) return;
		virtualizer.scrollToIndex(index, { align: "start", behavior: "auto" });
		focusedHighlightSignatureRef.current = signature;
	}, [props.highlight, props.items, rows, virtualizer]);

	const replaceActiveInUrl = useCallback((selector: string) => {
		const url = new URL(window.location.href);
		url.searchParams.set("highlight_active", selector);
		window.history.replaceState(window.history.state, "", url);
	}, []);

	const activateTarget = useCallback(
		(target: { selector: string; release_id: string }, focus: boolean) => {
			replaceActiveInUrl(target.selector);
			const targetIds =
				props.highlight?.mode === "discrete"
					? props.highlight.resolved.map((candidate) => candidate.release_id)
					: props.items
							.filter((item) => item.is_highlighted)
							.map((item) => item.release_id);
			const navigationIndex = targetIds.indexOf(target.release_id);
			const currentNavigationIndex = targetIds.indexOf(
				props.highlight?.active_release_id ?? "",
			);
			const absoluteIndex =
				props.highlight?.mode === "range" &&
				props.highlight.active_index !== null &&
				currentNavigationIndex >= 0
					? props.highlight.active_index +
						(navigationIndex - currentNavigationIndex)
					: navigationIndex + 1;
			props.onActivateHighlight(
				target.release_id,
				Math.min(
					props.highlight?.total ?? absoluteIndex,
					Math.max(1, absoluteIndex),
				),
			);
			const index = rows.findIndex(
				(row) =>
					row.kind === "release" && row.item.release_id === target.release_id,
			);
			if (index < 0) return;
			virtualizer.scrollToIndex(index, { align: "start", behavior: "auto" });
			if (focus) {
				window.requestAnimationFrame(() => {
					document
						.querySelector<HTMLElement>(
							`[data-release-id="${CSS.escape(target.release_id)}"]`,
						)
						?.focus({ preventScroll: true });
				});
			}
		},
		[
			props.highlight,
			props.items,
			props.onActivateHighlight,
			replaceActiveInUrl,
			rows,
			virtualizer,
		],
	);

	const navigationTargets = useMemo(() => {
		if (!props.highlight) return [];
		if (props.highlight.mode === "discrete") return props.highlight.resolved;
		return props.items
			.filter((item) => item.is_highlighted)
			.map((item, index) => ({
				selector: `id:${item.release_id}`,
				release_id: item.release_id,
				tag_name: item.tag_name,
				ordinal: index + 1,
			}));
	}, [props.highlight, props.items]);
	const activeTargetIndex = Math.max(
		0,
		navigationTargets.findIndex(
			(target) => target.release_id === props.highlight?.active_release_id,
		),
	);

	const detailHref = useCallback(
		(item: PublicReleaseListItem) => {
			const params = appendPublicReleaseHighlightParams(
				new URLSearchParams(),
				props.highlightSelection,
			);
			if (props.highlight) {
				const selector =
					props.highlight.resolved.find(
						(target) => target.release_id === item.release_id,
					)?.selector ?? `id:${item.release_id}`;
				params.set("highlight_active", selector);
			}
			const query = params.toString();
			const path = `/public/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
			return query ? `${path}?${query}` : path;
		},
		[props.highlight, props.highlightSelection, props.owner, props.repo],
	);

	if (props.items.length === 0) {
		return (
			<Card>
				<CardHeader>
					<CardTitle>还没有缓存到 Release</CardTitle>
					<CardDescription>
						仓库已同步完成，但当前共享缓存里没有可展示的 Release。
					</CardDescription>
				</CardHeader>
			</Card>
		);
	}

	return (
		<div className="space-y-3 sm:space-y-4">
			<AutoLoadSentinel
				enabled={props.hasNewer && !props.loadingNewer && !props.appendError}
				onVisible={props.onLoadNewer}
			/>
			{props.highlight &&
			(props.highlight.unresolved.length > 0 || props.highlight.message) ? (
				<p
					className="font-mono text-xs text-muted-foreground"
					data-testid="public-release-highlight-unresolved"
					role="status"
				>
					{props.highlight.message ??
						`${props.highlight.unresolved.length} 个高亮目标暂时未找到`}
				</p>
			) : null}
			<div
				ref={listRef}
				className="relative w-full"
				style={{ height: `${virtualizer.getTotalSize()}px` }}
				data-testid="public-release-virtual-list"
			>
				{virtualItems.map((virtualItem) => {
					const row = rows[virtualItem.index];
					if (!row) return null;
					return (
						<div
							key={virtualItem.key}
							ref={virtualizer.measureElement}
							data-index={virtualItem.index}
							className="absolute top-0 left-0 w-full pb-3 sm:pb-4"
							style={{
								transform: `translateY(${virtualItem.start - virtualizer.options.scrollMargin}px)`,
							}}
						>
							{row.kind === "gap" ? (
								<GapLoader
									gap={row.gap}
									loading={props.loadingGap === row.gap.newer_cursor}
									onVisible={props.onLoadGap}
								/>
							) : (
								<ReleaseVirtualRow
									item={row.item}
									lane={props.selectedLane}
									detailHref={detailHref(row.item)}
									reactionControls={props.reactionControls}
								/>
							)}
						</div>
					);
				})}
			</div>
			<AutoLoadSentinel
				enabled={props.hasMore && !props.loadingMore && !props.appendError}
				onVisible={props.onLoadMore}
			/>
			{props.loadingMore ? (
				<p className="font-mono text-xs text-muted-foreground">加载中...</p>
			) : null}
			{props.appendError ? (
				<div className="flex justify-center pt-1">
					<Button
						type="button"
						variant="outline"
						size="sm"
						className="font-mono text-xs"
						onClick={props.onLoadMore}
					>
						继续加载
					</Button>
				</div>
			) : null}
			{props.hasMore && !props.loadingMore && !props.appendError ? (
				<div className="flex justify-center pt-1">
					<Button
						type="button"
						variant="ghost"
						size="sm"
						className="font-mono text-xs"
						onClick={props.onLoadMore}
					>
						更多
					</Button>
				</div>
			) : null}
			{props.highlight && props.highlight.total > 0 ? (
				<nav
					className="fixed right-4 z-30 flex items-center gap-1 rounded-xl border bg-background/95 p-1 shadow-sm supports-[backdrop-filter]:backdrop-blur-sm"
					style={{ bottom: "calc(env(safe-area-inset-bottom) + 1rem)" }}
					aria-label="高亮记录导航"
					data-testid="public-release-highlight-navigation"
				>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="上一条高亮记录"
						disabled={activeTargetIndex <= 0}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex - 1];
							if (target) activateTarget(target, true);
						}}
					>
						<ChevronUp className="size-4" />
					</Button>
					<span className="min-w-16 text-center font-mono text-xs tabular-nums">
						{props.highlight.active_index ?? activeTargetIndex + 1} /{" "}
						{props.highlight.total}
					</span>
					<Button
						type="button"
						variant="ghost"
						size="icon"
						title="下一条高亮记录"
						disabled={
							activeTargetIndex >= navigationTargets.length - 1 &&
							!props.hasMore
						}
						onClick={() => {
							const target = navigationTargets[activeTargetIndex + 1];
							if (target) {
								activateTarget(target, true);
							} else {
								void props.onLoadMore();
							}
						}}
					>
						<ChevronDown className="size-4" />
					</Button>
				</nav>
			) : null}
		</div>
	);
}

function AutoLoadSentinel(props: {
	enabled: boolean;
	onVisible: () => Promise<void>;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	const visibleRef = useRef(false);
	useEffect(() => {
		if (!props.enabled || !ref.current) return;
		const observer = new IntersectionObserver(
			(entries) => {
				const visible = entries.some((entry) => entry.isIntersecting);
				if (visible && !visibleRef.current) {
					visibleRef.current = true;
					void props.onVisible();
				} else if (!visible) {
					visibleRef.current = false;
				}
			},
			{ rootMargin: "900px 0px", threshold: 0.01 },
		);
		observer.observe(ref.current);
		return () => observer.disconnect();
	}, [props.enabled, props.onVisible]);
	return <div ref={ref} className="h-px" aria-hidden="true" />;
}

function GapLoader(props: {
	gap: PublicReleaseGap;
	loading: boolean;
	onVisible: (gap: PublicReleaseGap) => Promise<void>;
}) {
	const ref = useRef<HTMLDivElement | null>(null);
	useEffect(() => {
		if (props.loading || !ref.current) return;
		const observer = new IntersectionObserver(
			(entries) => {
				if (entries.some((entry) => entry.isIntersecting)) {
					void props.onVisible(props.gap);
				}
			},
			{ rootMargin: "700px 0px", threshold: 0.01 },
		);
		observer.observe(ref.current);
		return () => observer.disconnect();
	}, [props.gap, props.loading, props.onVisible]);
	return (
		<div
			ref={ref}
			className="flex min-h-14 items-center justify-center rounded-xl border border-dashed bg-muted/20 px-4 py-3 font-mono text-xs text-muted-foreground"
			role="status"
		>
			{props.loading
				? "正在补齐中间记录..."
				: `省略 ${props.gap.remaining_count} 条，滚动后自动加载`}
		</div>
	);
}

function ReleaseVirtualRow(props: {
	item: PublicReleaseListItem;
	lane: FeedLane;
	detailHref: string;
	reactionControls: PublicReleaseReactionControls;
}) {
	const showReactions =
		props.reactionControls.enabled &&
		props.reactionControls.availableReleaseIds.has(props.item.release_id);
	const reactions = showReactions
		? (props.reactionControls.byReleaseId[props.item.release_id] ?? null)
		: null;
	const feedItem = publicReleaseToFeedItem(props.item, reactions);
	return (
		<div
			tabIndex={props.item.is_active_highlight ? -1 : undefined}
			className={cn(
				"scroll-mt-5 rounded-xl outline-none transition-[background-color,box-shadow] duration-200 motion-reduce:transition-none",
				props.item.is_highlighted && "bg-primary/[0.04] ring-1 ring-primary/30",
				props.item.is_active_highlight &&
					"bg-primary/[0.07] ring-2 ring-primary/65 ring-offset-2 ring-offset-background",
			)}
			data-highlighted={props.item.is_highlighted ? "true" : "false"}
			data-active-highlight={props.item.is_active_highlight ? "true" : "false"}
			data-release-id={props.item.release_id}
			data-testid={`public-release-item-${props.item.release_id}`}
		>
			<Suspense
				fallback={<ReleaseCardFallback title={releaseTitle(props.item)} />}
			>
				<ReleaseFeedCard
					item={feedItem}
					activeLane={props.lane}
					isTranslating={false}
					isTranslationAutoRetrying={false}
					isSmartGenerating={false}
					isSmartAutoRetrying={false}
					isReactionBusy={props.reactionControls.busyReleaseIds.has(
						props.item.release_id,
					)}
					reactionError={
						props.reactionControls.errorByReleaseId[props.item.release_id] ??
						null
					}
					showReactions={showReactions}
					showRepoIdentity={false}
					showHeaderActions={false}
					titleHref={props.detailHref}
					onSelectLane={() => undefined}
					onTranslateNow={() => undefined}
					onSmartNow={() => undefined}
					onToggleReaction={(content) =>
						props.reactionControls.onToggle(props.item.release_id, content)
					}
				/>
			</Suspense>
		</div>
	);
}

function publicReleaseToFeedItem(
	item: PublicReleaseListItem,
	reactions: ReleaseReactions | null = null,
): ReleaseFeedItem {
	const body = truncatePublicReleaseListBody(item.body);
	return {
		kind: "release",
		ts: item.published_at ?? "",
		id: item.release_id,
		repo_full_name: item.repo_full_name,
		repo_visual: item.repo_visual,
		title: releaseTitle(item),
		body,
		body_truncated: body !== item.body,
		subtitle: item.tag_name,
		reason: null,
		subject_type: null,
		html_url: item.html_url,
		unread: null,
		translated: item.translated,
		smart: item.smart,
		reactions,
	};
}

function truncatePublicReleaseListBody(body: string | null) {
	if (!body || body.length <= PUBLIC_RELEASE_LIST_BODY_MAX_CHARS) {
		return body;
	}
	return `${body.slice(0, PUBLIC_RELEASE_LIST_BODY_MAX_CHARS).trimEnd()}\n\n...`;
}

function ReleaseDetail({ detail }: { detail: ReleaseDetailResponse }) {
	const initialLane =
		detail.smart?.status === "ready"
			? "smart"
			: detail.translated?.status === "ready"
				? "translated"
				: "original";
	const [selectedLane, setSelectedLane] = useState<FeedLane>(initialLane);
	const feedItem = publicReleaseDetailToFeedItem(detail);

	return (
		<div className="py-6">
			<Suspense fallback={<ReleaseCardFallback title={releaseTitle(detail)} />}>
				<ReleaseFeedCard
					item={feedItem}
					activeLane={selectedLane}
					isTranslating={false}
					isTranslationAutoRetrying={false}
					isSmartGenerating={false}
					isSmartAutoRetrying={false}
					isReactionBusy={false}
					reactionError={null}
					showReactions={false}
					surface="article"
					onSelectLane={setSelectedLane}
					onTranslateNow={() => undefined}
					onSmartNow={() => undefined}
					onToggleReaction={() => undefined}
				/>
			</Suspense>
		</div>
	);
}

function ReleaseCardFallback(props: { title: string }) {
	return (
		<Card>
			<CardHeader>
				<CardTitle>{props.title}</CardTitle>
				<CardDescription>正在加载 Release 卡片</CardDescription>
			</CardHeader>
		</Card>
	);
}

function publicReleaseDetailToFeedItem(
	detail: ReleaseDetailResponse,
): ReleaseFeedItem {
	return {
		kind: "release",
		ts: detail.published_at ?? "",
		id: detail.release_id,
		repo_full_name: detail.repo_full_name,
		repo_visual: detail.repo_visual,
		title: releaseTitle(detail),
		body: detail.body,
		body_truncated: false,
		subtitle: detail.tag_name,
		reason: null,
		subject_type: null,
		html_url: detail.html_url,
		unread: null,
		translated: detail.translated,
		smart: detail.smart,
		reactions: null,
	};
}
