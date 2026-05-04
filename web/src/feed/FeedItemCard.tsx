import {
	ArrowUpRight,
	FolderGit2,
	GitFork,
	Megaphone,
	RefreshCcw,
	Star,
	UserPlus,
	UserRound,
} from "lucide-react";
import {
	type CSSProperties,
	type ReactNode,
	useEffect,
	useLayoutEffect,
	useRef,
	useState,
} from "react";

import { ErrorBubble } from "@/components/feedback/ErrorBubble";
import { ErrorStatePanel } from "@/components/feedback/ErrorStatePanel";
import { Markdown } from "@/components/Markdown";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardFooter,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { FEED_LANE_OPTIONS } from "@/feed/laneOptions";
import {
	isReleaseFeedItem,
	isSocialFeedItem,
	type FeedActor,
	type FeedItem,
	type FeedLane,
	type FeedViewer,
	type ReactionContent,
	type ReleaseFeedItem,
	type SocialFeedItem,
} from "@/feed/types";
import { withBaseAssetPath } from "@/lib/asset-path";
import { formatIsoShortLocal } from "@/lib/datetime";
import {
	resolveErrorDetail,
	resolveErrorSummary,
} from "@/lib/errorPresentation";
import { resolveRepoVisualCandidates, type RepoVisual } from "@/lib/repoVisual";
import { cn } from "@/lib/utils";

const REACTION_ITEMS: Array<{
	content: ReactionContent;
	iconSrc: string;
	label: string;
}> = [
	{
		content: "plus1",
		iconSrc: withBaseAssetPath("reactions/plus1.svg"),
		label: "赞",
	},
	{
		content: "laugh",
		iconSrc: withBaseAssetPath("reactions/laugh.svg"),
		label: "笑",
	},
	{
		content: "heart",
		iconSrc: withBaseAssetPath("reactions/heart.svg"),
		label: "爱心",
	},
	{
		content: "hooray",
		iconSrc: withBaseAssetPath("reactions/hooray.svg"),
		label: "庆祝",
	},
	{
		content: "rocket",
		iconSrc: withBaseAssetPath("reactions/rocket.svg"),
		label: "火箭",
	},
	{
		content: "eyes",
		iconSrc: withBaseAssetPath("reactions/eyes.svg"),
		label: "关注",
	},
];

function reactionAriaLabel(label: string, count: number) {
	return count > 0 ? `${label} ${count}` : label;
}

function EmptyPanel(props: {
	title: string;
	description: string;
	actionLabel?: string;
	onAction?: () => void;
	disabled?: boolean;
	loading?: boolean;
}) {
	const {
		title,
		description,
		actionLabel,
		onAction,
		disabled = false,
		loading = false,
	} = props;
	return (
		<div className="rounded-xl border border-dashed bg-muted/20 p-4">
			<p className="text-sm font-medium">{title}</p>
			<p className="mt-1 text-sm text-muted-foreground">{description}</p>
			{actionLabel && onAction ? (
				<Button
					variant="outline"
					size="sm"
					className="mt-3 font-mono text-xs"
					onClick={onAction}
					disabled={disabled || loading}
					aria-busy={loading ? "true" : undefined}
					data-empty-panel-action-loading={loading ? "true" : "false"}
				>
					<RefreshCcw className={cn("size-4", loading && "animate-spin")} />
					{actionLabel}
				</Button>
			) : null}
		</div>
	);
}

function displayTitleForLane(item: ReleaseFeedItem, lane: FeedLane) {
	const originalTitle = item.title ?? "(no title)";
	if (lane === "translated") {
		return item.translated?.title?.trim() || originalTitle;
	}
	if (lane === "smart") {
		return item.smart?.title?.trim() || originalTitle;
	}
	return originalTitle;
}

function OriginalLane(props: { item: ReleaseFeedItem }) {
	const { item } = props;
	if (item.body?.trim()) {
		return (
			<>
				{item.body_truncated ? (
					<div className="mb-2 font-mono text-[11px] text-muted-foreground">
						列表正文已截断显示；完整 release notes 请前往 GitHub 查看。
					</div>
				) : null}
				<Markdown content={item.body} />
			</>
		);
	}
	return (
		<EmptyPanel
			title="原文暂无可读正文"
			description="这个 release 没有可直接展示的正文，主人可以切到翻译或润色看看。"
		/>
	);
}

function TranslatedLane(props: {
	item: ReleaseFeedItem;
	onTranslateNow: () => void;
}) {
	const { item, onTranslateNow } = props;

	if (item.translated?.status === "ready" && item.translated.summary?.trim()) {
		return <Markdown content={item.translated.summary} />;
	}

	if (item.translated?.status === "disabled") {
		return (
			<EmptyPanel
				title="翻译不可用"
				description="当前环境没有可用的 AI 翻译能力。"
			/>
		);
	}

	if (item.translated?.status === "error") {
		return (
			<ErrorStatePanel
				title="翻译失败"
				summary={resolveErrorSummary(
					item.translated,
					"这次翻译没有成功，可以重新触发一次。",
				)}
				detail={resolveErrorDetail(item.translated)}
				actionLabel="重试翻译"
				onAction={onTranslateNow}
				size="compact"
			/>
		);
	}

	return <OriginalLane item={item} />;
}

function SmartLane(props: {
	item: ReleaseFeedItem;
	onSmartNow: () => void;
	isSmartGenerating: boolean;
}) {
	const { item, onSmartNow, isSmartGenerating } = props;

	if (item.smart?.status === "ready" && item.smart.summary?.trim()) {
		return <Markdown content={item.smart.summary} />;
	}

	if (item.smart?.status === "disabled") {
		return (
			<EmptyPanel
				title="润色不可用"
				description="当前环境没有可用的 AI 润色能力。"
			/>
		);
	}

	if (item.smart?.status === "error") {
		return (
			<ErrorStatePanel
				title="润色失败"
				summary={resolveErrorSummary(
					item.smart,
					"这次润色没有成功完成，可以立即再试一次。",
				)}
				detail={resolveErrorDetail(item.smart)}
				actionLabel="重试润色"
				onAction={onSmartNow}
				disabled={isSmartGenerating}
				loading={isSmartGenerating}
				size="compact"
			/>
		);
	}

	return <OriginalLane item={item} />;
}

function FeedCardLaneTabs(props: {
	activeLane: FeedLane;
	isTranslating: boolean;
	isSmartGenerating: boolean;
}) {
	const { activeLane, isTranslating, isSmartGenerating } = props;

	return (
		<TabsList className="h-7 shrink-0 gap-0.5 rounded-full border border-border/45 bg-muted/45 p-0.5 shadow-sm sm:h-8">
			{FEED_LANE_OPTIONS.map((option) => {
				const Icon = option.icon;
				const active = option.lane === activeLane;
				const isLoading =
					(option.lane === "translated" && isTranslating) ||
					(option.lane === "smart" && isSmartGenerating);
				const loadingClass = isLoading
					? active
						? "animate-pulse ring-2 ring-foreground/15 ring-offset-1 ring-offset-background"
						: "animate-pulse text-foreground/60 ring-1 ring-primary/20"
					: null;
				return (
					<Tooltip key={option.lane}>
						<TooltipTrigger asChild>
							<TabsTrigger
								value={option.lane}
								aria-label={option.label}
								data-feed-lane-trigger={option.lane}
								data-active={active ? "true" : "false"}
								data-feed-lane-loading={isLoading ? "true" : "false"}
								className={cn(
									"h-7 w-7 flex-none rounded-full border px-0 shadow-none transition-all",
									active
										? "border-foreground bg-foreground text-background shadow-sm hover:bg-foreground hover:text-background"
										: "border-transparent text-foreground/30 hover:text-foreground/60",
									loadingClass,
								)}
							>
								<Icon className="size-3.25" />
								<span className="sr-only">{option.label}</span>
							</TabsTrigger>
						</TooltipTrigger>
						<TooltipContent side="top" sideOffset={6}>
							{option.label}
						</TooltipContent>
					</Tooltip>
				);
			})}
		</TabsList>
	);
}

function SocialActorAvatar(props: { actor: FeedActor; className?: string }) {
	const { actor, className } = props;
	const [failed, setFailed] = useState(false);

	return (
		<div
			className={cn(
				"bg-muted text-muted-foreground flex size-11 shrink-0 items-center justify-center overflow-hidden rounded-full border",
				className,
			)}
		>
			{actor.avatar_url && !failed ? (
				<img
					src={actor.avatar_url}
					alt={`${actor.login} avatar`}
					className="size-full object-cover"
					onError={() => setFailed(true)}
				/>
			) : (
				<UserRound className="size-5" data-social-avatar-fallback="true" />
			)}
		</div>
	);
}

function SocialRepoAvatar(props: {
	repoVisual: RepoVisual | null | undefined;
	className?: string;
}) {
	const { repoVisual, className } = props;
	const [failed, setFailed] = useState(false);
	const candidates = resolveRepoVisualCandidates(repoVisual);
	const preferredCandidate =
		candidates.find((entry) => entry.kind === "owner_avatar") ?? null;
	const candidate = preferredCandidate ?? candidates[0] ?? null;

	return (
		<span
			className={cn(
				"bg-muted text-muted-foreground inline-flex size-4 shrink-0 items-center justify-center overflow-hidden border",
				"rounded-full",
				className,
			)}
		>
			{candidate && !failed ? (
				<img
					src={candidate.src}
					alt=""
					className="size-full object-cover"
					onError={() => setFailed(true)}
				/>
			) : (
				<FolderGit2 className="size-3" />
			)}
		</span>
	);
}

function middleEllipsis(value: string, head: number, tail: number) {
	if (value.length <= head + tail + 1) {
		return value;
	}
	return `${value.slice(0, head)}…${value.slice(-tail)}`;
}

function compactRepoFullName(repoFullName: string) {
	const slashIndex = repoFullName.indexOf("/");
	if (slashIndex > 0) {
		const owner = repoFullName.slice(0, slashIndex);
		const repo = repoFullName.slice(slashIndex + 1);
		const tail = 8;
		const desiredVisible = 36;
		const ownerVisible = owner.length + 1;
		const head = Math.max(8, desiredVisible - ownerVisible - tail);
		return `${owner}/${middleEllipsis(repo, head, tail)}`;
	}
	return middleEllipsis(repoFullName, 13, 10);
}

let inlineMeasureCanvas: HTMLCanvasElement | null = null;

function getInlineMeasureContext() {
	if (!inlineMeasureCanvas) {
		inlineMeasureCanvas = document.createElement("canvas");
	}
	return inlineMeasureCanvas.getContext("2d");
}

function measureInlineEntityWidth(group: HTMLElement): number {
	const avatarWidth =
		group.firstElementChild instanceof HTMLElement
			? group.firstElementChild.getBoundingClientRect().width
			: 0;
	const label = group.querySelector<HTMLElement>("[data-social-card-primary]");
	const gap =
		Number.parseFloat(window.getComputedStyle(group).columnGap || "0") || 0;
	let labelWidth = 0;
	if (label) {
		const computedStyle = window.getComputedStyle(label);
		const text =
			label.dataset.socialCardPrimaryMobile ??
			label.dataset.socialCardPrimaryFull ??
			label.textContent ??
			"";
		const context = getInlineMeasureContext();
		if (context) {
			context.font = computedStyle.font;
			const letterSpacing =
				Number.parseFloat(computedStyle.letterSpacing || "0") || 0;
			labelWidth =
				context.measureText(text).width +
				Math.max(0, text.length - 1) * letterSpacing;
		} else {
			labelWidth = Math.max(
				label.scrollWidth,
				label.getBoundingClientRect().width,
			);
		}
	}
	return Math.ceil(avatarWidth + gap + labelWidth);
}

function SocialEntityCard(props: {
	href?: string | null;
	avatar: ReactNode;
	primary: string;
	mono?: boolean;
	segment?: "actor" | "target";
	className?: string;
	primaryClassName?: string;
}) {
	const {
		href,
		avatar,
		primary,
		mono = false,
		segment,
		className,
		primaryClassName,
	} = props;
	return (
		<div
			data-social-card-segment={segment}
			className={cn(
				"relative z-10 flex min-w-0 items-center gap-1.5 rounded-xl border border-border/65 bg-background/78 px-2 py-2 shadow-sm",
				"sm:gap-3 sm:rounded-2xl sm:px-3 sm:py-3",
				className,
			)}
		>
			<div className="shrink-0">{avatar}</div>
			<div className="min-w-0 flex-1">
				<div className="flex min-w-0 items-center gap-0.5">
					<div
						data-social-card-primary="true"
						data-social-card-primary-full={primary}
						data-social-card-primary-mobile={primary}
						title={primary}
						className="min-w-0 flex-1"
					>
						<p
							className={cn(
								"truncate text-sm font-semibold text-foreground md:text-[15px]",
								mono && "sm:font-mono sm:font-medium",
								primaryClassName,
							)}
							data-social-card-primary-full-label="true"
						>
							{primary}
						</p>
					</div>
					{href ? (
						<a
							href={href}
							target="_blank"
							rel="noreferrer"
							aria-label={`打开 ${primary}`}
							className="inline-flex size-3.5 shrink-0 -translate-x-px translate-y-px items-center justify-center rounded-sm text-muted-foreground transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
						>
							<ArrowUpRight className="size-3" />
						</a>
					) : null}
				</div>
			</div>
		</div>
	);
}

function SocialActionChip(props: {
	icon: typeof Star;
	title: string;
	style?: CSSProperties;
}) {
	const { icon: Icon, title, style } = props;
	return (
		<div
			data-social-card-segment="action"
			role="img"
			aria-label={title}
			title={title}
			style={style}
			className="flex size-8 shrink-0 items-center justify-center rounded-full bg-muted/45 text-muted-foreground"
		>
			<Icon className="size-4" />
		</div>
	);
}

function SocialActionBridge(props: {
	icon: typeof Star;
	title: string;
	subtitle?: string;
	className?: string;
}) {
	const { icon: Icon, title, subtitle, className } = props;

	return (
		<div
			data-social-card-segment="action"
			className={cn(
				"flex min-w-0 flex-col items-center justify-center gap-1 text-center",
				className,
			)}
		>
			<div className="flex size-[30px] shrink-0 items-center justify-center rounded-full border border-border/65 bg-background shadow-sm sm:size-10 md:size-11">
				<Icon className="size-[15px] text-foreground/80 sm:size-[18px] md:size-5" />
			</div>
			<div className="space-y-0">
				<p className="text-[10px] leading-none font-semibold text-foreground sm:text-xs md:text-sm">
					{title}
				</p>
				{subtitle ? (
					<p className="text-xs text-muted-foreground">{subtitle}</p>
				) : null}
			</div>
		</div>
	);
}

function FreshContentCue(props: { className?: string }) {
	return (
		<span
			className={cn("inline-flex items-center align-middle", props.className)}
			title="刚刚同步"
		>
			<span
				className="dashboard-fresh-cue inline-flex size-2.5 rounded-full"
				data-dashboard-fresh-cue="true"
				aria-hidden="true"
			/>
			<span className="sr-only">刚刚同步</span>
		</span>
	);
}

function SocialActivityCard(props: {
	item: SocialFeedItem;
	currentViewer?: FeedViewer | null;
	isFresh?: boolean;
}) {
	const { item, currentViewer, isFresh = false } = props;
	const actor = item.actor;
	const isRepoStar = item.kind === "repo_star_received";
	const isFollower = item.kind === "follower_received";
	const isRepoTarget = !isFollower;
	const showTimestamp = !isFollower;
	const ActionIcon =
		item.kind === "announcement"
			? Megaphone
			: item.kind === "repo_forked"
				? GitFork
				: isRepoStar
					? Star
					: UserPlus;
	const actionTitle =
		item.kind === "announcement"
			? "公告"
			: item.kind === "repo_forked"
				? "Fork"
				: isRepoStar
					? "标星"
					: "关注";
	const actorHref = actor.html_url ?? `https://github.com/${actor.login}`;
	const repoHref = item.repo_full_name
		? `https://github.com/${item.repo_full_name}`
		: null;
	const actorCardHref = actorHref;
	const targetCardHref = isRepoStar
		? repoHref
		: isRepoTarget
			? (item.html_url ?? repoHref)
			: null;
	const targetViewer: FeedActor = currentViewer ?? {
		login: "",
		avatar_url: null,
		html_url: null,
	};
	const targetViewerHref =
		targetViewer.html_url ??
		(targetViewer.login ? `https://github.com/${targetViewer.login}` : null);
	const actorMobileLabel = actor.login;
	const targetMobileLabel = isRepoTarget
		? compactRepoFullName(item.title ?? item.repo_full_name ?? "仓库")
		: targetViewer.login;
	const mobileRowRef = useRef<HTMLDivElement | null>(null);
	const mobileActorGroupRef = useRef<HTMLSpanElement | null>(null);
	const mobileTargetGroupRef = useRef<HTMLSpanElement | null>(null);
	const mobileActionShellRef = useRef<HTMLDivElement | null>(null);
	const [mobileGridTemplate, setMobileGridTemplate] = useState(
		"minmax(0,1fr) auto minmax(0,1fr)",
	);
	const [mobileBalanceMode, setMobileBalanceMode] = useState<
		"centered" | "adaptive"
	>("centered");
	const mobileActorSegmentClass =
		mobileBalanceMode === "adaptive"
			? "shrink-0 justify-start"
			: "w-full justify-start";
	const mobileTargetSegmentClass =
		mobileBalanceMode === "adaptive"
			? "shrink-0 justify-end"
			: "w-full justify-end";

	useLayoutEffect(() => {
		const row = mobileRowRef.current;
		const actorGroup = mobileActorGroupRef.current;
		const targetGroup = mobileTargetGroupRef.current;
		const actionShell = mobileActionShellRef.current;
		if (!row || !actorGroup || !targetGroup || !actionShell) {
			return;
		}

		const computeTemplate = () => {
			const style = window.getComputedStyle(row);
			const gap = Number.parseFloat(style.columnGap || style.gap || "0") || 0;
			const actionWidth = actionShell.getBoundingClientRect().width;
			const availableWidth = row.clientWidth - actionWidth - gap * 2;
			if (availableWidth <= 0) {
				setMobileGridTemplate("minmax(0,1fr) auto minmax(0,1fr)");
				setMobileBalanceMode("centered");
				return;
			}

			const inlineWidthSafetyRoom = 6;
			const actorNaturalWidth =
				measureInlineEntityWidth(actorGroup) + inlineWidthSafetyRoom;
			const targetNaturalWidth =
				measureInlineEntityWidth(targetGroup) + inlineWidthSafetyRoom;
			const centeredWidth = availableWidth / 2;
			const centeredSafetyRoom = 8;
			const centeredFits =
				actorNaturalWidth < centeredWidth - centeredSafetyRoom &&
				targetNaturalWidth < centeredWidth - centeredSafetyRoom;
			const adaptiveFits =
				actorNaturalWidth + targetNaturalWidth <= availableWidth;

			let leftWidth = centeredWidth;
			let rightWidth = centeredWidth;
			let nextBalanceMode: "centered" | "adaptive" = "centered";

			if (!centeredFits && adaptiveFits) {
				nextBalanceMode = "adaptive";
				leftWidth = actorNaturalWidth;
				rightWidth = targetNaturalWidth;
			}

			setMobileGridTemplate(
				`minmax(0, ${leftWidth}px) auto minmax(0, ${rightWidth}px)`,
			);
			setMobileBalanceMode(nextBalanceMode);
		};

		computeTemplate();

		const resizeObserver = new ResizeObserver(() => {
			computeTemplate();
		});
		resizeObserver.observe(row);
		resizeObserver.observe(actorGroup);
		resizeObserver.observe(targetGroup);
		resizeObserver.observe(actionShell);
		window.addEventListener("resize", computeTemplate);
		return () => {
			resizeObserver.disconnect();
			window.removeEventListener("resize", computeTemplate);
		};
	}, []);

	return (
		<div
			className="space-y-0 px-1 py-1"
			data-feed-item-id={item.id}
			data-social-card-kind={item.kind}
			data-social-card-layout="inline-compact"
			data-social-card-time-visible={showTimestamp ? "true" : "false"}
		>
			{showTimestamp ? (
				<div className="mb-1 flex min-w-0 items-center gap-2 sm:mb-0.5">
					<p
						className="font-mono text-[11px] leading-none text-muted-foreground sm:text-xs"
						data-social-card-timestamp
					>
						{formatIsoShortLocal(item.ts)}
					</p>
					{isFresh ? <FreshContentCue className="shrink-0" /> : null}
					{item.kind === "announcement" || item.kind === "repo_forked" ? (
						<p className="min-w-0 truncate text-[11px] leading-none font-medium text-foreground/70 sm:text-xs">
							{item.title ?? actionTitle}
						</p>
					) : null}
				</div>
			) : null}
			<div
				ref={mobileRowRef}
				data-social-card-row="true"
				data-social-card-balance-mode={mobileBalanceMode}
				className={cn(
					"w-full min-w-0 items-center gap-2 rounded-[20px] border border-border/65 bg-background/84 px-2.5 py-1.5 shadow-sm backdrop-blur-[2px] sm:hidden",
					mobileBalanceMode === "adaptive" ? "flex justify-between" : "grid",
				)}
				style={
					mobileBalanceMode === "adaptive"
						? undefined
						: { gridTemplateColumns: mobileGridTemplate }
				}
			>
				{isRepoTarget ? (
					<>
						<a
							href={actorCardHref}
							target="_blank"
							rel="noreferrer"
							data-social-card-segment="actor"
							aria-label={`打开 ${actor.login}`}
							title={actor.login}
							className={cn(
								"flex min-w-0 max-w-full transition-opacity hover:opacity-100",
								mobileActorSegmentClass,
							)}
						>
							<span
								ref={mobileActorGroupRef}
								data-social-card-entity-group="actor"
								className="flex min-w-0 max-w-full items-center gap-2"
							>
								<SocialActorAvatar
									key={actor.avatar_url ?? actor.login}
									actor={actor}
									className="size-7"
								/>
								<p
									data-social-card-primary="true"
									data-social-card-primary-full={actor.login}
									data-social-card-primary-mobile={actorMobileLabel}
									title={actor.login}
									className="min-w-0 truncate text-[10px] leading-none font-medium tracking-tight text-foreground/78"
								>
									<span data-social-card-primary-mobile-label="true">
										{actorMobileLabel}
									</span>
								</p>
							</span>
						</a>
						<div
							ref={mobileActionShellRef}
							className="flex items-center justify-center"
						>
							<SocialActionChip icon={ActionIcon} title={actionTitle} />
						</div>
						<a
							href={targetCardHref ?? undefined}
							target="_blank"
							rel="noreferrer"
							data-social-card-segment="target"
							className={cn(
								"flex min-w-0 max-w-full transition-opacity hover:opacity-100",
								mobileTargetSegmentClass,
							)}
						>
							<span
								ref={mobileTargetGroupRef}
								data-social-card-entity-group="target"
								className="flex min-w-0 max-w-full items-center gap-2"
							>
								<SocialRepoAvatar
									key={[
										item.repo_visual?.owner_avatar_url ?? "",
										item.repo_visual?.open_graph_image_url ?? "",
										item.repo_visual?.uses_custom_open_graph_image ? "1" : "0",
										item.repo_full_name ?? "",
									].join("|")}
									repoVisual={item.repo_visual}
									className="size-7 border-border/60"
								/>
								<p
									data-social-card-primary="true"
									data-social-card-primary-full={
										item.title ?? item.repo_full_name ?? "仓库"
									}
									data-social-card-primary-mobile={targetMobileLabel}
									title={item.title ?? item.repo_full_name ?? "仓库"}
									className="min-w-0 truncate font-mono text-[10px] leading-none font-medium tracking-tight text-foreground"
								>
									<span data-social-card-primary-mobile-label="true">
										{targetMobileLabel}
									</span>
								</p>
							</span>
						</a>
					</>
				) : (
					<>
						<a
							href={actorCardHref}
							target="_blank"
							rel="noreferrer"
							data-social-card-segment="actor"
							className={cn(
								"flex min-w-0 max-w-full transition-opacity hover:opacity-100",
								mobileActorSegmentClass,
							)}
						>
							<span
								ref={mobileActorGroupRef}
								data-social-card-entity-group="actor"
								className="flex min-w-0 max-w-full items-center gap-2"
							>
								<SocialActorAvatar
									key={actor.avatar_url ?? actor.login}
									actor={actor}
									className="size-7"
								/>
								<p
									data-social-card-primary="true"
									data-social-card-primary-full={actor.login}
									data-social-card-primary-mobile={actorMobileLabel}
									title={actor.login}
									className="min-w-0 truncate text-[10px] leading-none font-semibold tracking-tight text-foreground"
								>
									<span data-social-card-primary-mobile-label="true">
										{actorMobileLabel}
									</span>
								</p>
							</span>
						</a>
						<div
							ref={mobileActionShellRef}
							className="flex items-center justify-center"
						>
							<SocialActionChip icon={UserPlus} title="关注" />
						</div>
						{targetViewerHref ? (
							<a
								href={targetViewerHref}
								target="_blank"
								rel="noreferrer"
								data-social-card-segment="target"
								className={cn(
									"flex min-w-0 max-w-full transition-opacity hover:opacity-100",
									mobileTargetSegmentClass,
								)}
							>
								<span
									ref={mobileTargetGroupRef}
									data-social-card-entity-group="target"
									className="flex min-w-0 max-w-full items-center gap-2"
								>
									<SocialActorAvatar
										key={targetViewer.avatar_url ?? targetViewer.login}
										actor={targetViewer}
										className="size-7"
									/>
									<p
										data-social-card-primary="true"
										data-social-card-primary-full={targetViewer.login}
										data-social-card-primary-mobile={targetViewer.login}
										title={targetViewer.login}
										className="min-w-0 truncate text-[10px] leading-none font-medium text-foreground"
									>
										<span data-social-card-primary-mobile-label="true">
											{targetViewer.login}
										</span>
									</p>
								</span>
							</a>
						) : (
							<div
								data-social-card-segment="target"
								className={cn(
									"flex min-w-0 max-w-full items-center gap-2",
									mobileTargetSegmentClass,
								)}
							>
								<span
									data-social-card-entity-group="target"
									className="flex min-w-0 max-w-full items-center gap-2"
								>
									<SocialActorAvatar
										key={targetViewer.avatar_url ?? "viewer"}
										actor={targetViewer}
										className="size-7"
									/>
								</span>
							</div>
						)}
					</>
				)}
			</div>
			<div
				data-social-card-row="true"
				className={cn(
					"hidden min-w-0 items-center gap-2 sm:grid sm:grid-cols-[minmax(0,1fr)_5.75rem_minmax(0,1fr)] sm:gap-3 md:grid-cols-[minmax(0,1fr)_164px_minmax(0,1fr)]",
				)}
			>
				<SocialEntityCard
					href={actorCardHref}
					segment="actor"
					avatar={
						<SocialActorAvatar
							key={actor.avatar_url ?? actor.login}
							actor={actor}
							className="size-11"
						/>
					}
					primary={actor.login}
				/>
				<SocialActionBridge
					icon={ActionIcon}
					title={actionTitle}
					className="px-0"
				/>
				{isRepoTarget ? (
					<SocialEntityCard
						href={targetCardHref}
						segment="target"
						avatar={
							<SocialRepoAvatar
								key={[
									item.repo_visual?.owner_avatar_url ?? "",
									item.repo_visual?.open_graph_image_url ?? "",
									item.repo_visual?.uses_custom_open_graph_image ? "1" : "0",
									item.repo_full_name ?? "",
								].join("|")}
								repoVisual={item.repo_visual}
								className="size-11 border-border/60"
							/>
						}
						primary={item.title ?? item.repo_full_name ?? "仓库"}
						mono
					/>
				) : (
					<SocialEntityCard
						href={null}
						segment="target"
						avatar={
							<SocialActorAvatar
								key={targetViewer.avatar_url ?? targetViewer.login}
								actor={targetViewer}
								className="size-11"
							/>
						}
						primary={targetViewer.login}
					/>
				)}
			</div>
		</div>
	);
}

function AnnouncementContentCard(props: {
	item: SocialFeedItem;
	isFresh?: boolean;
}) {
	const { item, isFresh = false } = props;
	const title = item.title?.trim() || item.repo_full_name || "仓库公告";
	const subtitleBits = [
		item.subtitle || "仓库公告",
		item.actor?.login ? `by ${item.actor.login}` : null,
	].filter(Boolean);
	const subtitle = subtitleBits.join(" · ");

	return (
		<Card
			className="group bg-card/80 shadow-sm transition-shadow hover:shadow-md"
			data-announcement-card="true"
			data-feed-item-id={item.id}
		>
			<CardHeader className="px-4 pb-3 pt-4 sm:px-6 sm:pb-4 sm:pt-6">
				<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
					<div className="min-w-0 flex-1">
						<div className="flex items-start gap-2">
							<div className="min-w-0 flex-1">
								<div className="flex flex-wrap items-center gap-2">
									<RepoIdentity
										repoFullName={item.repo_full_name}
										repoVisual={item.repo_visual}
										className="min-w-0 min-h-8"
										labelClassName="font-mono text-base font-medium tracking-tight text-foreground/80"
										visualClassName="size-8"
									/>
									<span className="inline-flex items-center gap-1 rounded-full border border-border/70 bg-muted/35 px-2 py-1 text-[11px] font-medium text-muted-foreground">
										<Megaphone className="size-3.5" />
										公告
									</span>
								</div>

								<CardTitle className="mt-2 text-balance text-[1.35rem] leading-tight sm:mt-2.5 sm:text-lg">
									{title}
								</CardTitle>
								<p className="mt-1 font-mono text-[11px] text-muted-foreground sm:text-xs">
									{formatIsoShortLocal(item.ts)}
									{isFresh ? <FreshContentCue className="ml-2" /> : null}
									{subtitle ? ` · ${subtitle}` : ""}
								</p>
							</div>

							<a
								href={item.html_url ?? "#"}
								target="_blank"
								rel="noreferrer"
								aria-label="GitHub"
								title="GitHub"
								data-feed-mobile-github-link="true"
								className="text-muted-foreground hover:text-foreground focus-visible:ring-ring inline-flex size-8 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 sm:hidden"
							>
								<ArrowUpRight className="size-4" />
								<span className="sr-only">GitHub</span>
							</a>
						</div>
					</div>

					<div className="hidden flex-wrap items-center gap-2 sm:flex sm:justify-end">
						<Button
							asChild
							variant="outline"
							size="sm"
							className="h-8 shrink-0 px-3 font-mono text-xs"
						>
							<a href={item.html_url ?? "#"} target="_blank" rel="noreferrer">
								<ArrowUpRight className="size-4" />
								GitHub
							</a>
						</Button>
					</div>
				</div>
			</CardHeader>

			<CardContent className="px-4 pb-4 pt-0 sm:px-6 sm:pb-6">
				{item.body?.trim() ? (
					<>
						{item.body_truncated ? (
							<div className="mb-2 font-mono text-[11px] text-muted-foreground">
								列表正文已截断显示；完整公告内容请前往 GitHub 查看。
							</div>
						) : null}
						<Markdown content={item.body} />
					</>
				) : (
					<EmptyPanel
						title="公告暂无可读正文"
						description="这条公告没有可直接展示的正文，主人可以前往 GitHub 查看完整内容。"
					/>
				)}
			</CardContent>
		</Card>
	);
}

function ReleaseFeedCard(props: {
	item: ReleaseFeedItem;
	activeLane: FeedLane;
	isTranslating: boolean;
	isSmartGenerating: boolean;
	isReactionBusy: boolean;
	reactionError: string | null;
	isFresh?: boolean;
	onSelectLane: (lane: FeedLane) => void;
	onTranslateNow: () => void;
	onSmartNow: () => void;
	onToggleReaction: (content: ReactionContent) => void;
}) {
	const {
		item,
		activeLane,
		isTranslating,
		isSmartGenerating,
		isReactionBusy,
		reactionError,
		isFresh = false,
		onSelectLane,
		onTranslateNow,
		onSmartNow,
		onToggleReaction,
	} = props;
	const subtitleBits = [
		item.reason || item.subtitle,
		item.subject_type ? item.subject_type : null,
	].filter(Boolean);
	const subtitle = subtitleBits.join(" · ");
	const reactions = item.reactions;
	const isVersionOnly = item.smart?.status === "insufficient";
	const displayTitle = displayTitleForLane(item, activeLane);
	const [reactionBubbleOpen, setReactionBubbleOpen] = useState(false);

	useEffect(() => {
		if (reactionError) {
			setReactionBubbleOpen(true);
		}
	}, [reactionError]);

	const header = (
		<CardHeader
			className={cn(
				"px-4 pb-3 pt-4 sm:px-6 sm:pb-4 sm:pt-6",
				isVersionOnly && "pb-4 sm:pb-5",
			)}
		>
			<div className="flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between sm:gap-4">
				<div className="min-w-0 flex-1">
					<div className="flex items-start gap-2">
						<div className="min-w-0 flex-1">
							<div className="flex flex-wrap items-center gap-2">
								{item.unread ? (
									<span className="inline-flex items-center gap-1 text-[11px] font-medium">
										<span className="size-1.5 rounded-full bg-primary" />
										<span className="text-muted-foreground">未读</span>
									</span>
								) : null}

								<RepoIdentity
									repoFullName={item.repo_full_name}
									repoVisual={item.repo_visual}
									className="min-w-0 min-h-8"
									labelClassName="font-mono text-base font-medium tracking-tight text-foreground/80"
									visualClassName="size-8"
								/>
							</div>

							<CardTitle className="mt-2 text-balance text-[1.35rem] leading-tight sm:mt-2.5 sm:text-lg">
								{displayTitle}
							</CardTitle>
							<p className="mt-1 font-mono text-[11px] text-muted-foreground sm:text-xs">
								{formatIsoShortLocal(item.ts)}
								{isFresh ? <FreshContentCue className="ml-2" /> : null}
								{subtitle ? ` · ${subtitle}` : ""}
							</p>
						</div>

						<a
							href={item.html_url ?? "#"}
							target="_blank"
							rel="noreferrer"
							aria-label="GitHub"
							title="GitHub"
							data-feed-mobile-github-link="true"
							className="text-muted-foreground hover:text-foreground focus-visible:ring-ring inline-flex size-8 shrink-0 items-center justify-center rounded-full transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-offset-2 sm:hidden"
						>
							<ArrowUpRight className="size-4" />
							<span className="sr-only">GitHub</span>
						</a>
					</div>
				</div>

				<div className="hidden flex-wrap items-center gap-2 sm:flex sm:justify-end">
					{isVersionOnly ? null : (
						<FeedCardLaneTabs
							activeLane={activeLane}
							isTranslating={isTranslating}
							isSmartGenerating={isSmartGenerating}
						/>
					)}

					<Button
						asChild
						variant="outline"
						size="sm"
						className="h-8 shrink-0 px-3 font-mono text-xs"
					>
						<a href={item.html_url ?? "#"} target="_blank" rel="noreferrer">
							<ArrowUpRight className="size-4" />
							GitHub
						</a>
					</Button>
				</div>
			</div>
		</CardHeader>
	);

	const reactionsFooter = reactions ? (
		<CardFooter
			className="border-border/70 flex flex-wrap items-center gap-2 border-t px-4 py-3 sm:gap-3 sm:px-6 sm:py-4"
			data-reaction-footer="true"
		>
			{reactions.status === "ready"
				? REACTION_ITEMS.map((reaction) => {
						const active = reactions.viewer[reaction.content];
						const count = reactions.counts[reaction.content];
						const label = reactionAriaLabel(reaction.label, count);
						return (
							<div
								key={reaction.content}
								className="relative inline-flex"
								data-reaction-chip={reaction.content}
							>
								<Button
									type="button"
									variant="outline"
									size="icon"
									className={cn(
										"group relative size-9 overflow-visible rounded-full border p-0 shadow-xs transition-[border-color,background-color,box-shadow,opacity] duration-200 ease-out hover:shadow-sm sm:size-10",
										active
											? "border-primary/45 bg-primary/10 hover:bg-primary/14"
											: "border-border/70 bg-background hover:border-foreground/15 hover:bg-accent/70",
										isReactionBusy && "opacity-80",
									)}
									onClick={() => onToggleReaction(reaction.content)}
									title={reaction.label}
									aria-label={label}
									aria-pressed={active}
									data-reaction-trigger={reaction.content}
									data-reaction-shape="round"
									data-reaction-count={count}
								>
									<img
										alt=""
										aria-hidden="true"
										className="size-[1.125rem] select-none transition-transform duration-200 group-hover:scale-105 group-active:scale-95"
										data-reaction-icon={reaction.content}
										draggable={false}
										src={reaction.iconSrc}
									/>
									{count > 0 ? (
										<span
											aria-hidden="true"
											className={cn(
												"absolute -right-0.5 -top-0.5 inline-flex h-[1.125rem] min-w-[1.125rem] items-center justify-center rounded-full px-1 font-mono text-[10px] leading-none shadow-sm ring-1 ring-background",
												active
													? "bg-primary text-primary-foreground"
													: "border border-border/70 bg-background text-foreground/80",
											)}
											data-reaction-count-badge={reaction.content}
											data-reaction-count-position="outside"
										>
											{count}
										</span>
									) : null}
								</Button>
							</div>
						);
					})
				: null}

			{reactions.status === "sync_required" ? (
				<span className="font-mono text-[11px] text-muted-foreground">
					反馈表情尚未就绪，请先使用顶部的{" "}
					<span className="font-mono">同步</span> 更新 releases。
				</span>
			) : null}

			{reactionError ? (
				<div className="ml-auto">
					<ErrorBubble
						open={reactionBubbleOpen}
						onOpenChange={setReactionBubbleOpen}
						title="反馈提交失败"
						summary={reactionError}
						align="end"
					>
						<Button
							type="button"
							variant="ghost"
							size="sm"
							className="text-destructive h-8 rounded-full px-3 font-mono text-[11px] hover:bg-destructive/8 hover:text-destructive"
						>
							反馈失败
						</Button>
					</ErrorBubble>
				</div>
			) : null}
		</CardFooter>
	) : null;

	return (
		<Card
			className={cn(
				"group bg-card/80 shadow-sm transition-shadow hover:shadow-md",
				isVersionOnly && "border-dashed",
			)}
		>
			{isVersionOnly ? (
				header
			) : (
				<Tabs
					value={activeLane}
					onValueChange={(value) => onSelectLane(value as FeedLane)}
					className="gap-0"
				>
					{header}

					<CardContent className="px-4 pb-4 pt-0 sm:px-6 sm:pb-6">
						<TabsContent value="original" className="mt-0">
							<OriginalLane item={item} />
						</TabsContent>
						<TabsContent value="translated" className="mt-0">
							<TranslatedLane item={item} onTranslateNow={onTranslateNow} />
						</TabsContent>
						<TabsContent value="smart" className="mt-0">
							<SmartLane
								item={item}
								onSmartNow={onSmartNow}
								isSmartGenerating={isSmartGenerating}
							/>
						</TabsContent>
					</CardContent>
				</Tabs>
			)}

			{reactionsFooter}
		</Card>
	);
}

export function FeedItemCard(props: {
	item: FeedItem;
	currentViewer?: FeedViewer | null;
	activeLane: FeedLane;
	isTranslating: boolean;
	isSmartGenerating: boolean;
	isReactionBusy: boolean;
	reactionError: string | null;
	isFresh?: boolean;
	onSelectLane: (lane: FeedLane) => void;
	onTranslateNow: () => void;
	onSmartNow: () => void;
	onToggleReaction: (content: ReactionContent) => void;
}) {
	const { item, currentViewer, isFresh = false } = props;
	let card: ReactNode = null;

	if (item.kind === "announcement") {
		card = <AnnouncementContentCard item={item} isFresh={isFresh} />;
	} else if (isSocialFeedItem(item)) {
		card = (
			<SocialActivityCard
				item={item}
				currentViewer={currentViewer}
				isFresh={isFresh}
			/>
		);
	} else if (isReleaseFeedItem(item)) {
		card = <ReleaseFeedCard {...props} item={item} />;
	} else {
		return null;
	}

	return (
		<div
			className={cn(
				"relative rounded-[24px] transition-[background-color,border-color,box-shadow] duration-200 ease-out",
				isFresh && "dashboard-fresh-surface",
			)}
			data-feed-item-fresh={isFresh ? "true" : "false"}
		>
			{card}
		</div>
	);
}
