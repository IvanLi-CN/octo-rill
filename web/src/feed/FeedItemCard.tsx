import {
	ArrowUpRight,
	FolderGit2,
	RefreshCcw,
	Star,
	UserPlus,
	UserRound,
} from "lucide-react";
import { type ReactNode, useState } from "react";

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
			description="这个 release 没有可直接展示的正文，主人可以切到翻译或智能看看。"
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

	if (item.body_truncated) {
		return (
			<EmptyPanel
				title="正文过长，无法直接翻译"
				description="列表卡片只保留截断正文；这类超长 release 建议直接打开 GitHub 阅读完整内容。"
			/>
		);
	}

	if (item.translated?.status === "error") {
		return (
			<EmptyPanel
				title="翻译失败"
				description="这次翻译没有成功，可以重新触发一次。"
				actionLabel="重试翻译"
				onAction={onTranslateNow}
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
				title="智能整理不可用"
				description="当前环境没有可用的 AI 智能总结能力。"
			/>
		);
	}

	if (item.smart?.status === "error") {
		return (
			<EmptyPanel
				title="智能整理失败"
				description="这次智能整理没有成功完成，可以立即再试一次。"
				actionLabel="重试智能整理"
				onAction={onSmartNow}
				disabled={isSmartGenerating}
				loading={isSmartGenerating}
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
		<TabsList className="h-8 shrink-0 gap-0.5 rounded-full border border-border/45 bg-muted/45 p-0.5 shadow-sm">
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
								title={option.label}
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

function SocialEntityCard(props: {
	href?: string | null;
	avatar: ReactNode;
	primary: string;
	mono?: boolean;
}) {
	const { href, avatar, primary, mono = false } = props;
	return (
		<div className="relative z-10 flex min-w-0 items-center gap-3 rounded-2xl border border-border/65 bg-background/78 px-3 py-3 shadow-sm">
			<div className="shrink-0">{avatar}</div>
			<div className="min-w-0">
				<div className="flex min-w-0 items-center gap-0.5">
					<p
						className={cn(
							"truncate text-sm font-semibold text-foreground sm:text-[15px]",
							mono && "font-mono font-medium",
						)}
					>
						{primary}
					</p>
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

function SocialActionBridge(props: {
	icon: typeof Star;
	title: string;
	subtitle?: string;
}) {
	const { icon: Icon, title, subtitle } = props;

	return (
		<div className="flex flex-col items-center justify-center gap-2 text-center">
			<div className="flex size-11 shrink-0 items-center justify-center rounded-full border border-border/65 bg-background shadow-sm">
				<Icon className="size-5 text-foreground/80" />
			</div>
			<div className="space-y-0.5">
				<p className="text-sm font-semibold text-foreground">{title}</p>
				{subtitle ? (
					<p className="text-xs text-muted-foreground">{subtitle}</p>
				) : null}
			</div>
		</div>
	);
}

function SocialActivityCard(props: {
	item: SocialFeedItem;
	currentViewer?: FeedViewer | null;
}) {
	const { item, currentViewer } = props;
	const actor = item.actor;
	const isRepoStar = item.kind === "repo_star_received";
	const actorHref = actor.html_url ?? `https://github.com/${actor.login}`;
	const repoHref = item.repo_full_name
		? `https://github.com/${item.repo_full_name}`
		: null;
	const viewerHref =
		currentViewer?.html_url ??
		(currentViewer?.login ? `https://github.com/${currentViewer.login}` : null);
	const targetViewer: FeedActor = currentViewer ?? {
		login: "你",
		avatar_url: null,
		html_url: null,
	};

	return (
		<div className="space-y-0 px-1 py-1">
			<p className="mb-0.5 font-mono text-xs leading-none text-muted-foreground">
				{formatIsoShortLocal(item.ts)}
			</p>
			<div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_164px_minmax(0,1fr)] md:items-center">
				<SocialEntityCard
					href={actorHref}
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
					icon={isRepoStar ? Star : UserPlus}
					title={isRepoStar ? "标星" : "关注"}
				/>
				{isRepoStar ? (
					<SocialEntityCard
						href={repoHref}
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
						primary={item.repo_full_name ?? "你的仓库"}
						mono
					/>
				) : (
					<SocialEntityCard
						href={viewerHref}
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

function ReleaseFeedCard(props: {
	item: ReleaseFeedItem;
	activeLane: FeedLane;
	isTranslating: boolean;
	isSmartGenerating: boolean;
	isReactionBusy: boolean;
	reactionError: string | null;
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

	const header = (
		<CardHeader className={cn("pb-4", isVersionOnly && "pb-5")}>
			<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
				<div className="min-w-0">
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

					<CardTitle className="mt-2.5 text-balance text-lg">
						{displayTitle}
					</CardTitle>
					<p className="mt-1 font-mono text-xs text-muted-foreground">
						{formatIsoShortLocal(item.ts)}
						{subtitle ? ` · ${subtitle}` : ""}
					</p>
				</div>

				<div className="flex flex-wrap items-center gap-2 sm:justify-end">
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
						className="shrink-0 font-mono text-xs"
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
			className="border-border/70 flex flex-wrap items-center gap-3 border-t px-6 py-4"
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
										"group relative size-10 overflow-visible rounded-full border p-0 shadow-xs transition-[border-color,background-color,box-shadow,opacity] duration-200 ease-out hover:shadow-sm",
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
										className="size-[1.35rem] select-none transition-transform duration-200 group-hover:scale-105 group-active:scale-95"
										draggable={false}
										src={reaction.iconSrc}
									/>
									{count > 0 ? (
										<span
											aria-hidden="true"
											className={cn(
												"absolute -right-1 -top-1 inline-flex h-5 min-w-5 items-center justify-center rounded-full px-1 font-mono text-[11px] leading-none shadow-sm ring-2 ring-background",
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
				<span className="text-destructive w-full font-mono text-[11px]">
					{reactionError}
				</span>
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

					<CardContent className="pt-0">
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
	onSelectLane: (lane: FeedLane) => void;
	onTranslateNow: () => void;
	onSmartNow: () => void;
	onToggleReaction: (content: ReactionContent) => void;
}) {
	const { item, currentViewer } = props;

	if (isSocialFeedItem(item)) {
		return <SocialActivityCard item={item} currentViewer={currentViewer} />;
	}

	if (!isReleaseFeedItem(item)) {
		return null;
	}

	return <ReleaseFeedCard {...props} item={item} />;
}
