import { ExternalLink, RefreshCcw } from "lucide-react";
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
	type PublicReleaseHighlight,
	type PublicReleaseListItem,
	type PublicReleasePendingResponse,
	type PublicReleaseResponse,
	type ReleaseDetailResponse,
	apiGetPublicRepoReleaseDetail,
	apiGetPublicRepoReleases,
} from "@/api";
import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
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
import type { FeedLane, ReleaseFeedItem } from "@/feed/types";
import type { PublicReleaseHighlightSelection } from "@/publicRelease/routeState";
import { cn } from "@/lib/utils";
import { buildVersionReleaseHref } from "@/version/versionReleaseLink";
import { useVersionMonitor } from "@/version/versionMonitor";

const ReleaseFeedCard = lazy(async () => {
	const module = await import("@/feed/FeedItemCard");
	return { default: module.ReleaseFeedCard };
});

const PUBLIC_RELEASE_LIST_BODY_MAX_CHARS = 2800;
const PUBLIC_RELEASE_PAGE_SIZE = 6;
const PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE = 12;

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

function releaseTitle(item: Pick<PublicReleaseListItem, "name" | "tag_name">) {
	return item.name?.trim() || item.tag_name;
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
	const [appendError, setAppendError] = useState<string | null>(null);
	const initialLoadKeyRef = useRef<string | null>(null);
	const prependAnchorRef = useRef<{ releaseId: string; top: number } | null>(
		null,
	);
	const highlightIds =
		highlight?.mode === "ids" || highlight?.mode === "invalid"
			? highlight.ids?.join(",")
			: undefined;
	const highlightStart =
		highlight?.mode === "range" || highlight?.mode === "invalid"
			? highlight.start
			: undefined;
	const highlightEnd =
		highlight?.mode === "range" || highlight?.mode === "invalid"
			? highlight.end
			: undefined;
	const isHighlightMode = highlight !== null;
	const initialLoadKey = [
		owner,
		repo,
		tag ?? "",
		highlightIds ?? "",
		highlightStart ?? "",
		highlightEnd ?? "",
	].join("\u0000");

	const buildHighlightRequest = useCallback(
		(direction?: "older" | "newer", cursor?: string | null) => ({
			owner,
			repo,
			source: "page" as const,
			limit: isHighlightMode
				? PUBLIC_RELEASE_HIGHLIGHT_PAGE_SIZE
				: PUBLIC_RELEASE_PAGE_SIZE,
			cursor,
			direction,
			highlight_ids: highlightIds,
			highlight_start: highlightStart,
			highlight_end: highlightEnd,
		}),
		[highlightEnd, highlightIds, highlightStart, isHighlightMode, owner, repo],
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
				...buildHighlightRequest("older", state.data.next_cursor),
			});
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (current.status !== "list") {
					return current;
				}
				const seen = new Set(current.data.items.map((item) => item.release_id));
				const incoming = data.items.filter(
					(item) => !seen.has(item.release_id),
				);
				return {
					status: "list",
					data: {
						...data,
						items: [...current.data.items, ...incoming],
						previous_cursor:
							data.previous_cursor ?? current.data.previous_cursor,
						highlight: data.highlight ?? current.data.highlight,
					},
				};
			});
		} catch (err) {
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingMore(false);
		}
	}, [buildHighlightRequest, loadingMore, state, tag]);

	const loadNewer = useCallback(async () => {
		if (
			tag ||
			loadingNewer ||
			state.status !== "list" ||
			!state.data.previous_cursor
		) {
			return;
		}
		const firstItem = document.querySelector<HTMLElement>(
			`[data-release-id="${CSS.escape(state.data.items[0]?.release_id ?? "")}"]`,
		);
		if (firstItem) {
			prependAnchorRef.current = {
				releaseId: state.data.items[0]?.release_id ?? "",
				top: firstItem.getBoundingClientRect().top,
			};
		}
		setLoadingNewer(true);
		setAppendError(null);
		try {
			const data = await apiGetPublicRepoReleases({
				...buildHighlightRequest("newer", state.data.previous_cursor),
			});
			if (isPendingResponse(data)) {
				setState({ status: "pending", pending: data });
				return;
			}
			setState((current) => {
				if (current.status !== "list") return current;
				const seen = new Set(current.data.items.map((item) => item.release_id));
				const incoming = data.items.filter(
					(item) => !seen.has(item.release_id),
				);
				return {
					status: "list",
					data: {
						...data,
						items: [...incoming, ...current.data.items],
						next_cursor: current.data.next_cursor ?? data.next_cursor,
						highlight: data.highlight ?? current.data.highlight,
					},
				};
			});
		} catch (err) {
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingNewer(false);
		}
	}, [buildHighlightRequest, loadingNewer, state, tag]);

	useLayoutEffect(() => {
		const anchor = prependAnchorRef.current;
		if (!anchor || state.status !== "list") return;
		const element = document.querySelector<HTMLElement>(
			`[data-release-id="${CSS.escape(anchor.releaseId)}"]`,
		);
		if (element) {
			window.scrollBy({
				top: element.getBoundingClientRect().top - anchor.top,
				behavior: "auto",
			});
		}
		prependAnchorRef.current = null;
	}, [state]);

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
							<BrandLogo variant="wordmark" className="h-6 sm:h-5" />
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

					{tag ? null : (
						<section className="py-6">
							<h1 className="break-words text-3xl font-semibold tracking-normal">
								{repoFullName}
							</h1>
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
							hasMore={Boolean(state.data.next_cursor)}
							hasNewer={Boolean(state.data.previous_cursor)}
							loadingMore={loadingMore}
							loadingNewer={loadingNewer}
							appendError={appendError}
							onLoadMore={loadMore}
							onLoadNewer={loadNewer}
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
	hasMore: boolean;
	hasNewer: boolean;
	loadingMore: boolean;
	loadingNewer: boolean;
	appendError: string | null;
	onLoadMore: () => void;
	onLoadNewer: () => void;
}) {
	const [selectedLane, setSelectedLane] = useState<FeedLane>("original");
	const [selectedLaneByRelease, setSelectedLaneByRelease] = useState<
		Record<string, FeedLane>
	>({});
	const sentinelRef = useRef<HTMLDivElement | null>(null);
	const sentinelVisibleRef = useRef(false);
	const newerSentinelRef = useRef<HTMLDivElement | null>(null);
	const newerSentinelVisibleRef = useRef(false);
	const releaseElementRefs = useRef(new Map<string, HTMLDivElement>());
	const focusedHighlightSignatureRef = useRef<string | null>(null);

	const selectAllLane = useCallback((lane: FeedLane) => {
		setSelectedLane(lane);
		setSelectedLaneByRelease({});
	}, []);

	const selectReleaseLane = useCallback((releaseId: string, lane: FeedLane) => {
		setSelectedLaneByRelease((current) => ({
			...current,
			[releaseId]: lane,
		}));
	}, []);

	useEffect(() => {
		if (
			!props.hasMore ||
			props.loadingMore ||
			props.appendError ||
			props.items.length === 0
		) {
			return;
		}
		const el = sentinelRef.current;
		if (!el) return;
		const obs = new IntersectionObserver(
			(entries) => {
				const isIntersecting = entries.some((entry) => entry.isIntersecting);
				if (isIntersecting && !sentinelVisibleRef.current) {
					sentinelVisibleRef.current = true;
					props.onLoadMore();
					return;
				}
				if (!isIntersecting) {
					sentinelVisibleRef.current = false;
				}
			},
			{ rootMargin: "900px 0px", threshold: 0.01 },
		);
		obs.observe(el);
		return () => obs.disconnect();
	}, [
		props.appendError,
		props.hasMore,
		props.items.length,
		props.loadingMore,
		props.onLoadMore,
	]);

	useEffect(() => {
		if (
			!props.hasNewer ||
			props.loadingNewer ||
			props.appendError ||
			props.items.length === 0
		) {
			return;
		}
		const el = newerSentinelRef.current;
		if (!el) return;
		const obs = new IntersectionObserver(
			(entries) => {
				const isIntersecting = entries.some((entry) => entry.isIntersecting);
				if (isIntersecting && !newerSentinelVisibleRef.current) {
					newerSentinelVisibleRef.current = true;
					props.onLoadNewer();
					return;
				}
				if (!isIntersecting) newerSentinelVisibleRef.current = false;
			},
			{ rootMargin: "900px 0px", threshold: 0.01 },
		);
		obs.observe(el);
		return () => obs.disconnect();
	}, [
		props.appendError,
		props.hasNewer,
		props.items.length,
		props.loadingNewer,
		props.onLoadNewer,
	]);

	useEffect(() => {
		if (!props.highlight || props.items.length === 0) return;
		const signature = [
			props.highlight.mode,
			...props.highlight.requested_ids,
			...props.highlight.resolved_ids,
		].join(":");
		if (focusedHighlightSignatureRef.current === signature) return;
		const targets = props.items
			.filter((item) => item.is_highlighted)
			.map((item) => releaseElementRefs.current.get(item.release_id))
			.filter((element): element is HTMLDivElement => Boolean(element));
		if (targets.length === 0) return;

		const frame = window.requestAnimationFrame(() => {
			const first = targets[0].getBoundingClientRect();
			const last = targets.at(-1)?.getBoundingClientRect() ?? first;
			const viewportHeight = window.innerHeight;
			const currentTop = window.scrollY;
			const firstAlignedTop = currentTop + first.top - 16;
			const lastAlignedBottom = currentTop + last.bottom - viewportHeight + 16;
			const spanHeight = last.bottom - first.top;
			const targetTop =
				spanHeight <= viewportHeight - 32
					? currentTop + (first.top + last.bottom) / 2 - viewportHeight / 2
					: Math.abs(firstAlignedTop - currentTop) <=
							Math.abs(lastAlignedBottom - currentTop)
						? firstAlignedTop
						: lastAlignedBottom;
			window.scrollTo({ top: Math.max(0, targetTop), behavior: "auto" });
			focusedHighlightSignatureRef.current = signature;
		});
		return () => window.cancelAnimationFrame(frame);
	}, [props.highlight, props.items]);

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
			<div ref={newerSentinelRef} />
			<div className="flex flex-wrap items-center justify-between gap-3">
				<div className="flex w-full items-center justify-end">
					<FeedPageLaneSelector
						value={selectedLane}
						onValueChange={selectAllLane}
					/>
				</div>
			</div>
			{props.highlight?.unresolved_ids.length ? (
				<p
					className="font-mono text-xs text-muted-foreground"
					data-testid="public-release-highlight-unresolved"
					role="status"
				>
					{props.highlight.unresolved_ids.length} 个高亮目标暂时未找到
				</p>
			) : null}
			{props.items.map((item) => {
				const feedItem = publicReleaseToFeedItem(item);
				const detailHref = `/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
				const itemLane = selectedLaneByRelease[item.release_id] ?? selectedLane;
				return (
					<div
						key={item.release_id}
						ref={(element) => {
							if (element) {
								releaseElementRefs.current.set(item.release_id, element);
							} else {
								releaseElementRefs.current.delete(item.release_id);
							}
						}}
						className={
							item.is_highlighted
								? "scroll-mt-5 rounded-[30px] bg-primary/[0.04] ring-2 ring-primary/35 ring-offset-2 ring-offset-background transition-[box-shadow,background-color] duration-200"
								: undefined
						}
						data-highlighted={item.is_highlighted ? "true" : "false"}
						data-release-id={item.release_id}
						data-testid={`public-release-item-${item.release_id}`}
					>
						<Suspense
							fallback={<ReleaseCardFallback title={releaseTitle(item)} />}
						>
							<ReleaseFeedCard
								item={feedItem}
								activeLane={itemLane}
								isTranslating={false}
								isTranslationAutoRetrying={false}
								isSmartGenerating={false}
								isSmartAutoRetrying={false}
								isReactionBusy={false}
								reactionError={null}
								showReactions={false}
								titleHref={detailHref}
								onSelectLane={(lane) =>
									selectReleaseLane(item.release_id, lane)
								}
								onTranslateNow={() => undefined}
								onSmartNow={() => undefined}
								onToggleReaction={() => undefined}
							/>
						</Suspense>
					</div>
				);
			})}
			<div ref={sentinelRef} />
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
		</div>
	);
}

function publicReleaseToFeedItem(item: PublicReleaseListItem): ReleaseFeedItem {
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
		reactions: null,
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
