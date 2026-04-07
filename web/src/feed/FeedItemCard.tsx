import { ArrowUpRight, RefreshCcw } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardFooter,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
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

function GeneratingPanel(props: { label: string }) {
	const { label } = props;
	return (
		<div
			data-feed-generating-panel={label}
			className="rounded-xl border border-dashed bg-muted/20 p-4"
		>
			<span className="sr-only">{label}</span>
			<div className="animate-pulse space-y-2">
				<div className="h-3 w-28 rounded bg-muted" />
				<div className="h-3 w-full rounded bg-muted" />
				<div className="h-3 w-11/12 rounded bg-muted" />
				<div className="h-3 w-8/12 rounded bg-muted" />
			</div>
		</div>
	);
}

function EmptyPanel(props: {
	title: string;
	description: string;
	actionLabel?: string;
	onAction?: () => void;
	disabled?: boolean;
}) {
	const { title, description, actionLabel, onAction, disabled = false } = props;
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
					disabled={disabled}
				>
					<RefreshCcw className="size-4" />
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

function TranslatedLane(props: {
	item: FeedItem;
	isTranslating: boolean;
	onTranslateNow: () => void;
}) {
	const { item, isTranslating, onTranslateNow } = props;
	if (isTranslating) {
		return <GeneratingPanel label="翻译生成中" />;
	}

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

	return (
		<EmptyPanel
			title="还没有翻译结果"
			description="切到这个 tab 后会按需生成中文译文。"
			actionLabel="生成翻译"
			onAction={onTranslateNow}
		/>
	);
}

function SmartLane(props: {
	item: FeedItem;
	isSmartGenerating: boolean;
	onSmartNow: () => void;
}) {
	const { item, isSmartGenerating, onSmartNow } = props;
	if (isSmartGenerating) {
		return <GeneratingPanel label="智能整理中" />;
	}

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
				description="可能是 compare diff 拉取失败，或者模型没有成功返回可用结果。"
				actionLabel="重试智能整理"
				onAction={onSmartNow}
			/>
		);
	}

	return (
		<EmptyPanel
			title="还没有智能版本变化摘要"
			description="这里会生成更适合快速了解版本变化的中文要点列表。"
			actionLabel="生成智能版"
			onAction={onSmartNow}
		/>
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

	const kindLabel = "RELEASE";
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
						<Badge className="border-primary/20 bg-primary font-mono text-[11px] tracking-wide text-primary-foreground">
							{kindLabel}
						</Badge>

						{item.unread ? (
							<span className="inline-flex items-center gap-1 text-[11px] font-medium">
								<span className="size-1.5 rounded-full bg-primary" />
								<span className="text-muted-foreground">未读</span>
							</span>
						) : null}

						{item.repo_full_name ? (
							<span className="truncate font-mono text-[11px] text-muted-foreground">
								{item.repo_full_name}
							</span>
						) : null}
					</div>

					<CardTitle className="mt-3 text-balance text-lg">
						{displayTitle}
					</CardTitle>
					<p className="mt-1 font-mono text-xs text-muted-foreground">
						{formatIsoShortLocal(item.ts)}
						{subtitle ? ` · ${subtitle}` : ""}
					</p>
				</div>

				<div className="flex flex-wrap items-center gap-2 sm:justify-end">
					{isVersionOnly ? null : (
						<TabsList className="h-auto shrink-0 rounded-full bg-muted/45 p-1">
							<TabsTrigger
								value="original"
								className="h-7 flex-none rounded-full px-3 font-mono text-xs shadow-none data-[state=active]:shadow-sm"
							>
								原文
							</TabsTrigger>
							<TabsTrigger
								value="translated"
								className="h-7 flex-none rounded-full px-3 font-mono text-xs shadow-none data-[state=active]:shadow-sm"
							>
								翻译
							</TabsTrigger>
							<TabsTrigger
								value="smart"
								className="h-7 flex-none rounded-full px-3 font-mono text-xs shadow-none data-[state=active]:shadow-sm"
							>
								智能
							</TabsTrigger>
						</TabsList>
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
							<TranslatedLane
								item={item}
								isTranslating={isTranslating}
								onTranslateNow={onTranslateNow}
							/>
						</TabsContent>
						<TabsContent value="smart" className="mt-0">
							<SmartLane
								item={item}
								isSmartGenerating={isSmartGenerating}
								onSmartNow={onSmartNow}
							/>
						</TabsContent>
					</CardContent>
				</Tabs>
			)}

			{isVersionOnly ? null : reactionsFooter}
		</Card>
	);
}
