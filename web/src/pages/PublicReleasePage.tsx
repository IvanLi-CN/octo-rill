import { ExternalLink, RefreshCcw } from "lucide-react";
import {
	lazy,
	Suspense,
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import {
	ApiError,
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
import type { FeedLane, ReleaseFeedItem } from "@/feed/types";
import { buildVersionReleaseHref } from "@/version/versionReleaseLink";
import { useVersionMonitor } from "@/version/versionMonitor";

const ReleaseFeedCard = lazy(async () => {
	const module = await import("@/feed/FeedItemCard");
	return { default: module.ReleaseFeedCard };
});

const PUBLIC_RELEASE_LIST_BODY_MAX_CHARS = 2800;
const PUBLIC_RELEASE_PAGE_SIZE = 6;

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

export function PublicReleasePage(props: {
	owner: string;
	repo: string;
	tag?: string | null;
}) {
	const { owner, repo, tag = null } = props;
	const [state, setState] = useState<LoadState>({ status: "loading" });
	const [loadingMore, setLoadingMore] = useState(false);
	const [appendError, setAppendError] = useState<string | null>(null);

	const load = useCallback(async () => {
		try {
			setAppendError(null);
			const data = tag
				? await apiGetPublicRepoReleaseDetail({
						owner,
						repo,
						tag,
						source: "page",
					})
				: await apiGetPublicRepoReleases({
						owner,
						repo,
						source: "page",
						limit: PUBLIC_RELEASE_PAGE_SIZE,
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
	}, [owner, repo, tag]);

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
				owner,
				repo,
				source: "page",
				limit: PUBLIC_RELEASE_PAGE_SIZE,
				cursor: state.data.next_cursor,
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
					},
				};
			});
		} catch (err) {
			setAppendError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoadingMore(false);
		}
	}, [loadingMore, owner, repo, state, tag]);

	useEffect(() => {
		void load();
	}, [load]);

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
				<header className="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
					<a href="/" className="inline-flex items-center gap-3">
						<BrandLogo variant="wordmark" className="h-6 sm:h-5" />
					</a>
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
					<WaitingCard title="正在读取公开 Release" onRetry={load} />
				) : null}

				{state.status === "pending" ? (
					<WaitingCard
						title="正在准备 Release 数据"
						description="这是这个仓库第一次通过公开入口访问。OctoRill 已经登记请求，会随全局同步补齐 Release 缓存。"
						retryAfter={state.pending.retry_after_seconds}
						statusLabel="同步排队中"
						onRetry={load}
					/>
				) : null}

				{state.status === "error" ? (
					<Card>
						<CardHeader>
							<CardTitle>暂时无法展示</CardTitle>
							<CardDescription>
								{state.code ? `${state.code}: ` : ""}
								{state.message}
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
						hasMore={Boolean(state.data.next_cursor)}
						loadingMore={loadingMore}
						appendError={appendError}
						onLoadMore={loadMore}
					/>
				) : null}

				{state.status === "detail" ? (
					<ReleaseDetail detail={state.data} />
				) : null}

				<PublicReleaseFooter owner={owner} repo={repo} />
			</div>
		</main>
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
						"首次访问会先登记仓库，Release 数据会随全局订阅同步更新。"}
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
	hasMore: boolean;
	loadingMore: boolean;
	appendError: string | null;
	onLoadMore: () => void;
}) {
	const [selectedLane, setSelectedLane] = useState<FeedLane>("original");
	const [selectedLaneByRelease, setSelectedLaneByRelease] = useState<
		Record<string, FeedLane>
	>({});
	const sentinelRef = useRef<HTMLDivElement | null>(null);
	const sentinelVisibleRef = useRef(false);

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
			<div className="flex flex-wrap items-center justify-between gap-3">
				<div className="flex w-full items-center justify-end">
					<FeedPageLaneSelector
						value={selectedLane}
						onValueChange={selectAllLane}
					/>
				</div>
			</div>
			{props.items.map((item) => {
				const feedItem = publicReleaseToFeedItem(item);
				const detailHref = `/${encodeURIComponent(props.owner)}/${encodeURIComponent(props.repo)}/releases/tag/${encodeURIComponent(item.tag_name)}`;
				const itemLane = selectedLaneByRelease[item.release_id] ?? selectedLane;
				return (
					<Suspense
						key={item.release_id}
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
							onSelectLane={(lane) => selectReleaseLane(item.release_id, lane)}
							onTranslateNow={() => undefined}
							onSmartNow={() => undefined}
							onToggleReaction={() => undefined}
						/>
					</Suspense>
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
	const [selectedLane, setSelectedLane] = useState<FeedLane>("smart");
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
