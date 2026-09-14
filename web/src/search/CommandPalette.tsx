import {
	Archive,
	ArrowRight,
	BookOpen,
	Bookmark,
	BookmarkCheck,
	CalendarDays,
	Check,
	Command,
	FileText,
	Inbox,
	LoaderCircle,
	RefreshCcw,
	Search,
	Settings,
	ShieldCheck,
	Sparkles,
	X,
	type LucideIcon,
} from "lucide-react";
import {
	useCallback,
	useEffect,
	useMemo,
	useRef,
	useState,
	type KeyboardEvent,
	type RefObject,
} from "react";

import {
	apiSearch,
	apiFollowRepo,
	apiUnfollowRepo,
	ApiError,
	type SearchLane,
	type SearchIndexStatus,
	type SearchResponse,
	type SearchResult,
	type SearchResultType,
} from "@/api";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { parseDashboardRouteStateFromLocation } from "@/dashboard/routeState";
import { Button } from "@/components/ui/button";
import { preserveCurrentDemoSearchInHref } from "@/demo/registry";
import { resolveNotificationHref } from "@/inbox/notificationLink";
import { useInternalNavigate } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";

export type CommandPaletteProps = {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	portalContainer?: HTMLElement | null;
	initialQuery?: string;
	isAdmin?: boolean;
	busy?: string | null;
	onSyncAll?: () => void;
	onSyncInbox?: () => void;
	onGenerateBrief?: () => void | Promise<void>;
	searchTransport?: (
		query: string,
		signal?: AbortSignal,
	) => Promise<SearchResponse>;
	restoreFocusRef?: RefObject<HTMLElement | null>;
};

type ActionId =
	| "dashboard"
	| "mine"
	| "following"
	| "briefs"
	| "inbox"
	| "settings"
	| "admin"
	| "sync-all"
	| "sync-inbox"
	| "generate-brief";

type PaletteAction = {
	id: ActionId;
	label: string;
	description: string;
	href?: string;
	icon: LucideIcon;
};

const BASE_ACTIONS: PaletteAction[] = [
	{
		id: "dashboard",
		label: "打开动态",
		description: "回到全部 GitHub 动态",
		href: "/",
		icon: BookOpen,
	},
	{
		id: "mine",
		label: "我的仓库动态",
		description: "查看自己仓库的发布与反馈",
		href: "/focus/mine",
		icon: Archive,
	},
	{
		id: "following",
		label: "关注仓库",
		description: "只看关注仓库的动态",
		href: "/focus/following",
		icon: Check,
	},
	{
		id: "briefs",
		label: "打开日报",
		description: "查看最近生成的日报",
		href: "/briefs",
		icon: CalendarDays,
	},
	{
		id: "inbox",
		label: "打开 Inbox",
		description: "处理 GitHub Inbox 通知",
		href: "/inbox",
		icon: Inbox,
	},
	{
		id: "settings",
		label: "打开设置",
		description: "账户、阅读模式与 GitHub 连接",
		href: "/settings",
		icon: Settings,
	},
];

function formatResultType(type: SearchResultType) {
	switch (type) {
		case "release":
			return "Release";
		case "announcement":
			return "公告";
		case "brief":
			return "日报";
		case "notification":
			return "通知";
		case "repository":
			return "仓库";
	}
}

function formatLane(lane: SearchLane) {
	switch (lane) {
		case "translated":
			return "翻译命中";
		case "smart":
			return "润色命中";
		default:
			return "原文命中";
	}
}

function formatResetAt(value: string | null | undefined) {
	if (!value) return null;
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return null;
	return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}

function formatIndexStatus(status: SearchIndexStatus | null) {
	switch (status) {
		case "building":
			return "正在建立本地索引，当前只显示已完成的内容。";
		case "paused_low_disk":
			return "本地索引等待磁盘空间，当前只显示已完成的内容。";
		default:
			return null;
	}
}

function buildSearchResultTarget(result: SearchResult) {
	const targetPath =
		result.target_path ?? result.target?.href ?? result.target_url ?? "/";
	const lane =
		result.matched_lane ??
		result.matched_lanes?.[0] ??
		result.target?.lane ??
		null;
	const resourceType = result.resource_type ?? result.type ?? "release";
	if (resourceType === "notification") {
		const threadId = result.id.replace(/^notification:(?:[^:]+:)?/, "");
		return resolveNotificationHref({
			thread_id: threadId,
			html_url: result.target_url ?? result.target?.href ?? null,
			repo_full_name:
				result.repo_full_name ?? result.repository?.full_name ?? null,
		});
	}
	if (
		!lane ||
		(resourceType !== "release" && resourceType !== "announcement")
	) {
		return targetPath;
	}
	try {
		const target = new URL(targetPath, window.location.origin);
		const isDetail =
			target.pathname.includes("/releases/tag/") ||
			target.pathname.includes("/discussions/");
		if (isDetail) {
			const current = new URL(window.location.href);
			const currentPath = current.pathname.replace(/\/+$/, "") || "/";
			const currentRouteState = parseDashboardRouteStateFromLocation(
				current.pathname,
				current.search,
			);
			const currentFrom = currentRouteState.scope
				? currentRouteState.tab
				: (current.searchParams.get("from") ??
					(currentPath === "/"
						? (current.searchParams.get("tab") ?? "all")
						: currentPath === "/releases"
							? "releases"
							: currentPath === "/briefs"
								? "briefs"
								: currentPath === "/inbox"
									? "inbox"
									: null));
			if (currentFrom) target.searchParams.set("from", currentFrom);
			for (const key of ["brief"]) {
				const value = current.searchParams.get(key);
				if (value) target.searchParams.set(key, value);
			}
			const scope = currentRouteState.scope;
			if (scope) {
				target.searchParams.set("scope", scope.kind);
				switch (scope.kind) {
					case "repo":
						target.searchParams.set("items", `${scope.owner}/${scope.repo}`);
						break;
					case "repos":
						if (scope.items.length > 0) {
							target.searchParams.set("items", scope.items.join(","));
						}
						break;
					case "org":
						target.searchParams.set("org", scope.org);
						break;
				}
			}
		}
		if (lane) target.searchParams.set("lane", lane);
		return `${target.pathname}${target.search}${target.hash}`;
	} catch {
		return targetPath;
	}
}

function resultType(result: SearchResult): SearchResultType {
	return result.resource_type ?? result.type ?? "release";
}

function resultLane(result: SearchResult) {
	return result.matched_lane ?? result.matched_lanes?.[0] ?? null;
}

function resultSnippet(result: SearchResult) {
	return result.snippet ?? result.excerpt ?? null;
}

function resultRepoName(result: SearchResult) {
	return result.repo_full_name ?? result.repository?.full_name ?? null;
}

function getRateLimitMessage(error: unknown) {
	if (!(error instanceof ApiError) || error.code !== "search_rate_limited") {
		return null;
	}
	const payload = error.payload;
	const details =
		typeof payload === "object" && payload !== null && "error" in payload
			? (
					payload as {
						error?: { retry_after_seconds?: number; reset_at?: string };
					}
				).error
			: null;
	const root =
		typeof payload === "object" && payload !== null
			? (payload as { retry_after_seconds?: number; reset_at?: string })
			: null;
	const retryAfter = details?.retry_after_seconds ?? root?.retry_after_seconds;
	const resetAt = formatResetAt(details?.reset_at ?? root?.reset_at);
	return retryAfter
		? `搜索额度已用完，请 ${retryAfter} 秒后再试${resetAt ? `（${resetAt} 重置）` : ""}。`
		: "搜索额度已用完，请稍后再试。";
}

function SearchResultRow(props: {
	id: string;
	result: SearchResult;
	active: boolean;
	onSelect: () => void;
	onOpen: () => void;
}) {
	const { id, result, active, onSelect, onOpen } = props;
	const repository = resultType(result) === "repository";
	return (
		<button
			type="button"
			id={id}
			tabIndex={-1}
			role="option"
			aria-selected={active}
			className={cn(
				"group flex w-full items-start gap-3 px-4 py-2 text-left outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset",
				active ? "bg-muted/60" : "hover:bg-muted/35",
			)}
			onMouseEnter={onSelect}
			onClick={onOpen}
		>
			<span className="mt-0.5 flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-background text-muted-foreground">
				{repository ? (
					<Archive className="size-4" />
				) : resultType(result) === "notification" ? (
					<Inbox className="size-4" />
				) : resultType(result) === "brief" ? (
					<CalendarDays className="size-4" />
				) : (
					<FileText className="size-4" />
				)}
			</span>
			<span className="min-w-0 flex-1">
				<span className="flex min-w-0 items-center gap-2">
					<span className="truncate text-sm font-medium text-foreground">
						{result.title}
					</span>
					<span className="shrink-0 rounded-full border border-border/70 px-1.5 py-0.5 font-mono text-[10px] text-muted-foreground">
						{formatResultType(resultType(result))}
					</span>
				</span>
				<span className="mt-1 flex min-w-0 flex-wrap items-center gap-x-2 gap-y-1 text-xs text-muted-foreground">
					{resultRepoName(result) ? (
						<span className="font-mono">{resultRepoName(result)}</span>
					) : null}
					{result.unread ? <span className="text-foreground">未读</span> : null}
					{resultLane(result) ? (
						<span>{formatLane(resultLane(result) as SearchLane)}</span>
					) : null}
				</span>
				{resultSnippet(result) ? (
					<span className="mt-1 block line-clamp-2 text-xs leading-5 text-muted-foreground">
						{resultSnippet(result)}
					</span>
				) : null}
			</span>
			<ArrowRight className="mt-2 size-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
		</button>
	);
}

function ActionRow(props: {
	id: string;
	action: PaletteAction;
	active: boolean;
	onSelect: () => void;
	onOpen: () => void;
	disabled?: boolean;
}) {
	const { id, action, active, onSelect, onOpen, disabled } = props;
	const Icon = action.icon;
	return (
		<button
			type="button"
			id={id}
			role="option"
			aria-selected={active}
			disabled={disabled}
			className={cn(
				"group flex w-full items-center gap-3 px-4 py-3 text-left transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring/50 focus-visible:ring-inset disabled:cursor-not-allowed disabled:opacity-50",
				active ? "bg-muted/60" : "hover:bg-muted/35",
			)}
			onMouseEnter={onSelect}
			onClick={onOpen}
		>
			<span className="flex size-8 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-background text-muted-foreground">
				<Icon className="size-4" />
			</span>
			<span className="min-w-0 flex-1">
				<span className="block text-sm font-medium text-foreground">
					{action.label}
				</span>
				<span className="mt-0.5 block truncate text-xs text-muted-foreground">
					{action.description}
				</span>
			</span>
			{disabled ? (
				<LoaderCircle className="size-4 shrink-0 animate-spin text-muted-foreground" />
			) : (
				<ArrowRight className="size-4 shrink-0 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100 group-focus-visible:opacity-100" />
			)}
		</button>
	);
}

export function CommandPalette({
	open,
	onOpenChange,
	portalContainer,
	initialQuery = "",
	isAdmin = false,
	busy = null,
	onSyncAll,
	onSyncInbox,
	onGenerateBrief,
	searchTransport = apiSearch,
	restoreFocusRef,
}: CommandPaletteProps) {
	const navigate = useInternalNavigate();
	const inputRef = useRef<HTMLInputElement>(null);
	const abortRef = useRef<AbortController | null>(null);
	const debounceRef = useRef<number | null>(null);
	const composingRef = useRef(false);
	const [composing, setComposing] = useState(false);
	const [query, setQuery] = useState(initialQuery);
	const [results, setResults] = useState<SearchResult[]>([]);
	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [remaining, setRemaining] = useState<number | null>(null);
	const [resetAt, setResetAt] = useState<string | null>(null);
	const [indexStatus, setIndexStatus] = useState<SearchIndexStatus | null>(
		null,
	);
	const [activeIndex, setActiveIndex] = useState(0);
	const [confirmGenerate, setConfirmGenerate] = useState(false);
	const [confirmFollow, setConfirmFollow] = useState<SearchResult | null>(null);
	const [followBusyId, setFollowBusyId] = useState<string | null>(null);
	const confirmBackRef = useRef<HTMLButtonElement | null>(null);

	const actions = useMemo(() => {
		if (isAdmin) {
			return [
				{
					id: "admin",
					label: "打开管理员面板",
					description: "进入后台任务与用户治理",
					href: "/admin",
					icon: ShieldCheck,
				},
			] satisfies PaletteAction[];
		}
		const next = [...BASE_ACTIONS];
		if (onSyncAll) {
			next.push({
				id: "sync-all",
				label: "全量同步",
				description: "刷新仓库、Release、社交动态与 Inbox",
				icon: RefreshCcw,
			});
		}
		if (onSyncInbox) {
			next.push({
				id: "sync-inbox",
				label: "同步 Inbox",
				description: "只刷新 GitHub Inbox 通知",
				icon: Inbox,
			});
		}
		if (onGenerateBrief) {
			next.push({
				id: "generate-brief",
				label: "生成日报",
				description: "生成最近已结束本地自然日的日报",
				icon: Sparkles,
			});
		}
		return next;
	}, [isAdmin, onGenerateBrief, onSyncAll, onSyncInbox]);

	const actionMode = query.trimStart().startsWith(">");
	const visibleActions = useMemo(() => {
		if (!query.trim()) return actions;
		const normalized = (actionMode ? query.trimStart().slice(1) : query)
			.trim()
			.toLocaleLowerCase();
		if (!normalized) return actions;
		return actions.filter(
			(action) =>
				action.label.toLocaleLowerCase().includes(normalized) ||
				action.description.toLocaleLowerCase().includes(normalized),
		);
	}, [actionMode, actions, query]);
	const isActionDisabled = useCallback(
		(action: PaletteAction) =>
			Boolean(busy) &&
			(action.id === "sync-all" ||
				action.id === "sync-inbox" ||
				action.id === "generate-brief"),
		[busy],
	);
	const entries =
		loading || error || confirmGenerate || confirmFollow
			? []
			: actionMode || !query.trim()
				? visibleActions
				: results;
	const hasListEntries = entries.length > 0;
	const contentRegionProps =
		confirmGenerate || confirmFollow
			? ({ role: "region", "aria-label": "命令面板确认" } as const)
			: hasListEntries
				? ({
						role: "listbox",
						"aria-label": "命令面板结果",
						"aria-live": "polite",
					} as const)
				: ({
						role: "region",
						"aria-label": "命令面板状态",
						"aria-live": "polite",
					} as const);
	const activeEntryId =
		entries.length > 0 ? `command-palette-entry-${activeIndex}` : undefined;
	const activeRepository =
		!actionMode &&
		query.trim() &&
		!loading &&
		!error &&
		!confirmGenerate &&
		!confirmFollow &&
		entries[activeIndex] &&
		resultType(entries[activeIndex] as SearchResult) === "repository"
			? (entries[activeIndex] as SearchResult)
			: null;

	useEffect(() => {
		if (!open) return;
		setQuery(initialQuery);
		setResults([]);
		setError(null);
		setRemaining(null);
		setResetAt(null);
		setIndexStatus(null);
		setActiveIndex(0);
		setConfirmGenerate(false);
		setConfirmFollow(null);
		setFollowBusyId(null);
		const timer = window.setTimeout(() => inputRef.current?.focus(), 0);
		return () => window.clearTimeout(timer);
	}, [initialQuery, open]);

	useEffect(() => {
		if (
			!open ||
			actionMode ||
			!query.trim() ||
			composing ||
			composingRef.current
		) {
			abortRef.current?.abort();
			if (debounceRef.current !== null) {
				window.clearTimeout(debounceRef.current);
				debounceRef.current = null;
			}
			if (!query.trim() || actionMode) {
				setResults([]);
				setLoading(false);
				setError(null);
			}
			return;
		}

		abortRef.current?.abort();
		if (debounceRef.current !== null) window.clearTimeout(debounceRef.current);
		const controller = new AbortController();
		abortRef.current = controller;
		setLoading(true);
		setError(null);
		setResults([]);
		debounceRef.current = window.setTimeout(() => {
			void searchTransport(query.trim(), controller.signal)
				.then((response) => {
					if (controller.signal.aborted) return;
					setResults(response.items ?? []);
					setRemaining(
						response.remaining ?? response.remaining_requests ?? null,
					);
					setResetAt(response.reset_at ?? null);
					setIndexStatus(response.index_status ?? "ready");
					setActiveIndex(0);
				})
				.catch((reason: unknown) => {
					if (controller.signal.aborted) return;
					setResults([]);
					setError(
						getRateLimitMessage(reason) ??
							(reason instanceof Error
								? reason.message
								: "搜索暂时不可用，请稍后重试。"),
					);
				})
				.finally(() => {
					if (!controller.signal.aborted) setLoading(false);
				});
		}, 250);

		return () => {
			controller.abort();
			if (debounceRef.current !== null) {
				window.clearTimeout(debounceRef.current);
				debounceRef.current = null;
			}
		};
	}, [actionMode, composing, open, query, searchTransport]);

	useEffect(() => {
		setActiveIndex(0);
	}, [actionMode, query]);

	useEffect(() => {
		if (!confirmGenerate && !confirmFollow) return;
		const timer = window.setTimeout(() => confirmBackRef.current?.focus(), 0);
		return () => window.clearTimeout(timer);
	}, [confirmFollow, confirmGenerate]);

	useEffect(() => {
		if (confirmGenerate || confirmFollow || !activeEntryId) return;
		document
			.getElementById(activeEntryId)
			?.scrollIntoView({ block: "nearest" });
	}, [activeEntryId, confirmFollow, confirmGenerate]);

	useEffect(
		() => () => {
			abortRef.current?.abort();
			if (debounceRef.current !== null)
				window.clearTimeout(debounceRef.current);
		},
		[],
	);

	const close = useCallback(() => {
		onOpenChange(false);
	}, [onOpenChange]);

	const openTarget = useCallback(
		async (target: string) => {
			close();
			try {
				const resolved = new URL(target, window.location.origin);
				if (resolved.origin !== window.location.origin) {
					window.open(resolved.toString(), "_blank", "noopener,noreferrer");
					return;
				}
				const preservedHref = preserveCurrentDemoSearchInHref(
					`${resolved.pathname}${resolved.search}${resolved.hash}`,
				);
				const preserved = new URL(preservedHref, window.location.origin);
				const search = Object.fromEntries(preserved.searchParams.entries());
				await navigate({
					href: preservedHref,
					to: preserved.pathname,
					search,
				});
				return;
			} catch {
				// Fall through to the router for relative targets.
			}
			await navigate({ href: target, to: target });
		},
		[close, navigate],
	);

	const executeAction = useCallback(
		async (action: PaletteAction) => {
			if (isActionDisabled(action)) return;
			if (action.id === "generate-brief") {
				setConfirmGenerate(true);
				return;
			}
			if (action.href) {
				await openTarget(action.href);
				return;
			}
			if (action.id === "sync-all") onSyncAll?.();
			if (action.id === "sync-inbox") onSyncInbox?.();
			close();
		},
		[close, isActionDisabled, onSyncAll, onSyncInbox, openTarget],
	);

	const confirmBrief = useCallback(async () => {
		if (!onGenerateBrief) return;
		if (busy) return;
		await onGenerateBrief();
		close();
	}, [busy, close, onGenerateBrief]);

	const confirmRepositoryFollow = useCallback(async () => {
		if (!confirmFollow) return;
		const fullName = resultRepoName(confirmFollow);
		const [owner, repo] = fullName?.split("/", 2) ?? [];
		if (!owner || !repo) return;
		const nextFollowing = confirmFollow.is_following !== true;
		setFollowBusyId(confirmFollow.id);
		setError(null);
		try {
			if (nextFollowing) {
				await apiFollowRepo({ owner, repo });
			} else {
				await apiUnfollowRepo({ owner, repo });
			}
			setResults((current) =>
				current.map((item) =>
					item.id === confirmFollow.id
						? { ...item, is_following: nextFollowing }
						: item,
				),
			);
			setError(null);
			setConfirmFollow(null);
			window.setTimeout(() => inputRef.current?.focus(), 0);
		} catch (reason: unknown) {
			setError(reason instanceof Error ? reason.message : "仓库关注操作失败");
		} finally {
			setFollowBusyId(null);
		}
	}, [confirmFollow]);

	const handleKeyDown = useCallback(
		(event: KeyboardEvent<HTMLInputElement>) => {
			if (event.nativeEvent.isComposing || composingRef.current) return;
			if (event.key === "ArrowDown") {
				event.preventDefault();
				setActiveIndex((index) =>
					Math.min(index + 1, Math.max(entries.length - 1, 0)),
				);
				return;
			}
			if (event.key === "ArrowUp") {
				event.preventDefault();
				setActiveIndex((index) => Math.max(index - 1, 0));
				return;
			}
			if (event.key === "Enter") {
				event.preventDefault();
				const selected = entries[activeIndex];
				if (!selected) return;
				if (actionMode || !query.trim()) {
					if (isActionDisabled(selected as PaletteAction)) return;
					void executeAction(selected as PaletteAction);
				} else {
					void openTarget(buildSearchResultTarget(selected as SearchResult));
				}
			}
		},
		[
			activeIndex,
			actionMode,
			entries,
			executeAction,
			isActionDisabled,
			openTarget,
			query,
		],
	);

	return (
		<Dialog open={open} onOpenChange={onOpenChange}>
			<DialogContent
				container={portalContainer}
				className="top-[10vh] max-w-[calc(100%-2rem)] translate-y-0 overflow-hidden p-0 sm:top-[14vh] sm:max-w-2xl"
				onCloseAutoFocus={(event) => {
					if (restoreFocusRef?.current) {
						event.preventDefault();
						restoreFocusRef.current.focus();
					}
				}}
				showCloseButton={false}
				data-command-palette
				data-visual-evidence-target
			>
				<DialogHeader className="sr-only">
					<DialogTitle>命令面板</DialogTitle>
					<DialogDescription>
						搜索本地缓存内容，或执行工作区动作。
					</DialogDescription>
				</DialogHeader>
				<div className="flex items-center gap-3 border-b border-border/70 px-4">
					{actionMode ? (
						<Command className="size-5 shrink-0 text-muted-foreground" />
					) : (
						<Search className="size-5 shrink-0 text-muted-foreground" />
					)}
					<input
						ref={inputRef}
						value={query}
						onChange={(event) => {
							const nextQuery = event.target.value;
							setQuery(nextQuery);
							setResults([]);
							setError(null);
							setRemaining(null);
							setResetAt(null);
							setIndexStatus(null);
							setLoading(
								Boolean(
									nextQuery.trim() && !nextQuery.trimStart().startsWith(">"),
								),
							);
						}}
						onKeyDown={handleKeyDown}
						onCompositionStart={() => {
							composingRef.current = true;
							setComposing(true);
						}}
						onCompositionEnd={() => {
							composingRef.current = false;
							setComposing(false);
						}}
						className="h-14 min-w-0 flex-1 bg-transparent text-base outline-none placeholder:text-muted-foreground"
						placeholder={
							actionMode ? "搜索动作" : "搜索本地内容，或输入 > 执行动作"
						}
						aria-label="搜索内容或执行动作"
						role="combobox"
						aria-autocomplete="list"
						aria-expanded="true"
						aria-controls={
							confirmGenerate || confirmFollow || !hasListEntries
								? undefined
								: "command-palette-results"
						}
						aria-activedescendant={
							confirmGenerate || confirmFollow ? undefined : activeEntryId
						}
						data-command-palette-input
					/>
					{query ? (
						<Button
							type="button"
							variant="ghost"
							size="icon"
							className="size-8 shrink-0 rounded-full"
							aria-label="清除搜索"
							onClick={() => {
								setQuery("");
								setRemaining(null);
								setResetAt(null);
								setIndexStatus(null);
								inputRef.current?.focus();
							}}
						>
							<X className="size-4" />
						</Button>
					) : null}
					<kbd className="hidden shrink-0 items-center gap-1 rounded-md border border-border/70 bg-muted/30 px-2 py-1 font-mono text-[10px] text-muted-foreground sm:inline-flex">
						<span>Esc</span>
					</kbd>
				</div>

				<div className="max-h-[min(60vh,34rem)] overflow-y-auto py-2">
					{query.trim() && formatIndexStatus(indexStatus) ? (
						<p
							role="status"
							aria-live="polite"
							className="px-5 pt-2 text-xs text-muted-foreground"
						>
							{formatIndexStatus(indexStatus)}
						</p>
					) : null}
					{confirmGenerate ? (
						<div {...contentRegionProps} className="space-y-4 px-5 py-6">
							<div className="flex items-start gap-3">
								<span className="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border/70 bg-muted/30">
									<Sparkles className="size-4" />
								</span>
								<div>
									<p className="text-sm font-semibold">确认生成日报？</p>
									<p className="mt-1 text-xs leading-5 text-muted-foreground">
										将使用最近已结束的本地自然日，并提交一个后台任务。
									</p>
									{busy ? (
										<p
											role="status"
											className="mt-2 text-xs text-muted-foreground"
										>
											已有任务进行中，请完成后再生成日报。
										</p>
									) : null}
								</div>
							</div>
							<div className="flex justify-end gap-2">
								<Button
									type="button"
									variant="outline"
									ref={confirmBackRef}
									onClick={() => {
										setConfirmGenerate(false);
										window.setTimeout(() => inputRef.current?.focus(), 0);
									}}
								>
									返回
								</Button>
								<Button
									type="button"
									disabled={Boolean(busy)}
									onClick={() => void confirmBrief()}
								>
									确认生成
								</Button>
							</div>
						</div>
					) : confirmFollow ? (
						<div {...contentRegionProps} className="space-y-4 px-5 py-6">
							<p className="text-sm font-semibold">
								{confirmFollow.is_following === true
									? "取消关注这个仓库？"
									: "关注这个仓库？"}
							</p>
							<p className="text-xs leading-5 text-muted-foreground">
								{resultRepoName(confirmFollow) ?? confirmFollow.title}
							</p>
							{error ? (
								<p role="alert" className="text-xs leading-5 text-destructive">
									{error}
								</p>
							) : null}
							<div className="flex justify-end gap-2">
								<Button
									type="button"
									variant="outline"
									ref={confirmBackRef}
									onClick={() => {
										setConfirmFollow(null);
										window.setTimeout(() => inputRef.current?.focus(), 0);
									}}
								>
									返回
								</Button>
								<Button
									type="button"
									onClick={() => void confirmRepositoryFollow()}
									disabled={followBusyId !== null}
								>
									确认
								</Button>
							</div>
						</div>
					) : loading ? (
						<div
							{...contentRegionProps}
							role="status"
							aria-live="polite"
							className="flex items-center gap-3 px-5 py-8 text-sm text-muted-foreground"
						>
							<LoaderCircle className="size-4 animate-spin" /> 正在搜索本地缓存…
						</div>
					) : error ? (
						<div
							{...contentRegionProps}
							role="alert"
							aria-live="assertive"
							className="space-y-1 px-5 py-8"
						>
							<p className="text-sm font-medium text-destructive">
								搜索暂时不可用
							</p>
							<p className="text-xs leading-5 text-muted-foreground">{error}</p>
						</div>
					) : actionMode || !query.trim() ? (
						visibleActions.length > 0 ? (
							<div {...contentRegionProps} id="command-palette-results">
								{visibleActions.map((action, index) => (
									<ActionRow
										key={action.id}
										id={`command-palette-entry-${index}`}
										action={action}
										active={index === activeIndex}
										onSelect={() => setActiveIndex(index)}
										onOpen={() => void executeAction(action)}
										disabled={isActionDisabled(action)}
									/>
								))}
							</div>
						) : (
							<p
								{...contentRegionProps}
								className="px-5 py-8 text-sm text-muted-foreground"
							>
								没有匹配的动作。
							</p>
						)
					) : results.length > 0 ? (
						<>
							<div {...contentRegionProps} id="command-palette-results">
								{results.map((result, index) => (
									<SearchResultRow
										key={`${resultType(result)}:${result.id}`}
										id={`command-palette-entry-${index}`}
										result={result}
										active={index === activeIndex}
										onSelect={() => setActiveIndex(index)}
										onOpen={() =>
											void openTarget(buildSearchResultTarget(result))
										}
									/>
								))}
							</div>
							{activeRepository ? (
								<fieldset className="flex justify-end border-t border-border/60 px-4 py-2">
									<legend className="sr-only">仓库操作</legend>
									<Button
										type="button"
										variant="ghost"
										size="sm"
										className="gap-2"
										disabled={followBusyId === activeRepository.id}
										onClick={() => setConfirmFollow(activeRepository)}
									>
										{activeRepository.is_following === true ? (
											<BookmarkCheck className="size-4" />
										) : (
											<Bookmark className="size-4" />
										)}
										{activeRepository.is_following === true
											? "取消关注"
											: "关注仓库"}
									</Button>
								</fieldset>
							) : null}
						</>
					) : (
						<div {...contentRegionProps} className="px-5 py-8 text-center">
							<p className="text-sm font-medium text-foreground">
								没有找到本地内容
							</p>
							<p className="mt-1 text-xs leading-5 text-muted-foreground">
								试试引号短语、owner: 或 repo: 过滤器。
							</p>
						</div>
					)}
				</div>
				<div className="flex min-h-9 items-center justify-between gap-3 border-t border-border/70 px-4 py-2 text-[11px] text-muted-foreground">
					<span>{actionMode ? "动作模式" : "只搜索本地缓存"}</span>
					<span className="flex items-center gap-2 font-mono">
						{remaining !== null ? `剩余 ${remaining}/50` : null}
						{resetAt ? `· ${formatResetAt(resetAt) ?? "稍后重置"}` : null}
					</span>
				</div>
			</DialogContent>
		</Dialog>
	);
}
