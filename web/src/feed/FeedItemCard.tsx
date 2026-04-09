import { ArrowUpRight, RefreshCcw } from "lucide-react";

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
import type { FeedItem, FeedLane, ReactionContent } from "@/feed/types";
import { formatIsoShortLocal } from "@/lib/datetime";
import { cn } from "@/lib/utils";

const REACTION_ITEMS: Array<{
	content: ReactionContent;
	emoji: string;
	label: string;
}> = [
	{ content: "plus1", emoji: "👍", label: "赞" },
	{ content: "laugh", emoji: "😄", label: "笑" },
	{ content: "heart", emoji: "❤️", label: "爱心" },
	{ content: "hooray", emoji: "🎉", label: "庆祝" },
	{ content: "rocket", emoji: "🚀", label: "火箭" },
	{ content: "eyes", emoji: "👀", label: "关注" },
];

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

function displayTitleForLane(item: FeedItem, lane: FeedLane) {
	const originalTitle = item.title ?? "(no title)";
	if (lane === "translated") {
		return item.translated?.title?.trim() || originalTitle;
	}
	if (lane === "smart") {
		return item.smart?.title?.trim() || originalTitle;
	}
	return originalTitle;
}

function OriginalLane(props: { item: FeedItem }) {
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

function TranslatedLane(props: { item: FeedItem; onTranslateNow: () => void }) {
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
	item: FeedItem;
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

export function FeedItemCard(props: {
	item: FeedItem;
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
		<CardFooter className="border-border/70 flex flex-wrap items-center gap-2 border-t px-6 py-4">
			{reactions.status === "ready"
				? REACTION_ITEMS.map((reaction) => {
						const active = reactions.viewer[reaction.content];
						const count = reactions.counts[reaction.content];
						return (
							<Button
								key={reaction.content}
								type="button"
								variant={active ? "secondary" : "outline"}
								size="sm"
								className={cn(
									"group h-7 rounded-full px-2 font-mono text-xs transition-all duration-200 ease-out hover:-translate-y-0.5 hover:shadow-sm active:translate-y-0 active:scale-95",
									count > 0 && "gap-1",
									active &&
										"border-primary bg-primary/10 text-primary hover:bg-primary/15",
									isReactionBusy && "opacity-80",
								)}
								onClick={() => onToggleReaction(reaction.content)}
								title={reaction.label}
								aria-pressed={active}
							>
								<span className="transition-transform duration-200 group-hover:scale-110 group-active:scale-95">
									{reaction.emoji}
								</span>
								{count > 0 ? <span>{count}</span> : null}
							</Button>
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
