import { ArrowUpRight, Languages, RefreshCcw } from "lucide-react";

import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardFooter,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import type { FeedItem, ReactionContent } from "@/feed/types";
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

function formatIsoShort(iso: string) {
	// "2026-02-21T08:00:00Z" -> "2026-02-21 08:00:00"
	// "2026-02-21T08:00:00.123Z" -> "2026-02-21 08:00:00"
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

export function FeedItemCard(props: {
	item: FeedItem;
	showOriginal: boolean;
	isTranslating: boolean;
	isReactionBusy: boolean;
	reactionError: string | null;
	onToggleOriginal: () => void;
	onTranslateNow: () => void;
	onToggleReaction: (content: ReactionContent) => void;
	onSyncReleases: () => void;
}) {
	const {
		item,
		showOriginal,
		isTranslating,
		isReactionBusy,
		reactionError,
		onToggleOriginal,
		onTranslateNow,
		onToggleReaction,
		onSyncReleases,
	} = props;

	const kindLabel = "RELEASE";
	const originalTitle = item.title ?? "(no title)";
	const translatedTitle =
		item.translated?.status === "ready" ? item.translated.title : null;
	const displayTitle = showOriginal
		? originalTitle
		: translatedTitle?.trim() || originalTitle;

	const translatedSummary =
		item.translated?.status === "ready" ? item.translated.summary : null;
	const originalExcerpt = item.excerpt?.trim() ? item.excerpt : null;

	const showAiFallbackInOriginal =
		showOriginal && !originalExcerpt && Boolean(translatedSummary?.trim());

	// Prefer the active language, but never render an empty original view if we do have an AI
	// summary available (some releases legitimately ship with an empty body).
	const bodyText = showAiFallbackInOriginal
		? translatedSummary
		: showOriginal
			? originalExcerpt
			: translatedSummary?.trim()
				? translatedSummary
				: originalExcerpt;

	const subtitleBits = [
		item.reason || item.subtitle,
		item.subject_type ? item.subject_type : null,
	].filter(Boolean);
	const subtitle = subtitleBits.join(" · ");

	const showTranslationControls = Boolean(item.translated);
	const aiDisabled = item.translated?.status === "disabled";
	const canTranslate =
		!aiDisabled && item.translated?.status === "missing" && !isTranslating;

	const reactions = item.reactions;

	return (
		<Card className="group bg-card/80 shadow-sm transition-shadow hover:shadow-md">
			<CardHeader className="pb-4">
				<div className="flex items-start justify-between gap-3">
					<div className="min-w-0">
						<div className="flex flex-wrap items-center gap-2">
							<span
								className={cn(
									"font-mono inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] tracking-wide",
									"bg-primary text-primary-foreground border-primary/20",
								)}
							>
								{kindLabel}
							</span>

							{item.unread ? (
								<span className="inline-flex items-center gap-1 text-[11px] font-medium">
									<span className="bg-primary size-1.5 rounded-full" />
									<span className="text-muted-foreground">未读</span>
								</span>
							) : null}

							{item.repo_full_name ? (
								<span className="font-mono text-muted-foreground truncate text-[11px]">
									{item.repo_full_name}
								</span>
							) : null}
						</div>

						<CardTitle className="mt-3 text-balance text-lg">
							{displayTitle}
						</CardTitle>
						<CardDescription className="mt-1 font-mono text-xs">
							{formatIsoShort(item.ts)}
							{subtitle ? ` · ${subtitle}` : ""}
						</CardDescription>
					</div>

					<div className="flex shrink-0 items-center gap-2">
						{showTranslationControls ? (
							aiDisabled ? (
								<span className="text-muted-foreground font-mono text-[11px]">
									AI 未配置
								</span>
							) : (
								<>
									<Button
										variant="ghost"
										size="sm"
										className="font-mono text-xs"
										onClick={onToggleOriginal}
										disabled={Boolean(isTranslating)}
										title={showOriginal ? "切回中文" : "切回原文"}
									>
										<Languages className="size-4" />
										{showOriginal ? "中文" : "原文"}
									</Button>
									<Button
										variant="ghost"
										size="sm"
										className="font-mono text-xs"
										onClick={onTranslateNow}
										disabled={!canTranslate}
										title="重新翻译"
									>
										<RefreshCcw className="size-4" />
										{isTranslating ? "翻译中…" : "翻译"}
									</Button>
								</>
							)
						) : null}

						<Button
							asChild
							variant="outline"
							size="sm"
							className="font-mono text-xs"
						>
							<a href={item.html_url ?? "#"} target="_blank" rel="noreferrer">
								<ArrowUpRight className="size-4" />
								GitHub
							</a>
						</Button>
					</div>
				</div>
			</CardHeader>

			{bodyText ? (
				<CardContent className="pt-0">
					{showAiFallbackInOriginal ? (
						<div className="text-muted-foreground mb-2 font-mono text-[11px]">
							原文 Release notes 为空/无法提取，以下为 AI 中文翻译
						</div>
					) : null}
					<Markdown content={bodyText} />
				</CardContent>
			) : (
				<CardContent className="pt-0">
					<p className="text-muted-foreground text-sm">
						暂无内容，点击 <span className="font-mono">GitHub</span> 查看详情。
					</p>
				</CardContent>
			)}

			{reactions ? (
				<CardFooter className="border-border/70 flex flex-wrap items-center gap-2 border-t px-6 py-4">
					{reactions.status === "ready"
						? REACTION_ITEMS.map((reaction) => {
								const active = reactions.viewer[reaction.content];
								const count = reactions.counts[reaction.content];
								return (
									<button
										key={reaction.content}
										type="button"
										onClick={() => onToggleReaction(reaction.content)}
										className={cn(
											"group inline-flex cursor-pointer items-center rounded-full border px-2 py-0.5 font-mono text-xs transition-all duration-200 ease-out focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary/50 focus-visible:ring-offset-1 hover:-translate-y-0.5 hover:shadow-sm active:translate-y-0 active:scale-95",
											count > 0 && "gap-1",
											active
												? "border-primary bg-primary/10 text-primary"
												: "text-muted-foreground hover:text-foreground border-border",
											isReactionBusy && "opacity-80",
										)}
										title={reaction.label}
									>
										<span className="transition-transform duration-200 group-hover:scale-110 group-active:scale-95">
											{reaction.emoji}
										</span>
										{count > 0 ? <span>{count}</span> : null}
									</button>
								);
							})
						: null}

					{reactions.status === "sync_required" ? (
						<>
							<span className="text-muted-foreground font-mono text-[11px]">
								反馈表情尚未就绪，请先同步 releases
							</span>
							<Button
								variant="outline"
								size="sm"
								className="font-mono text-xs"
								onClick={onSyncReleases}
							>
								Sync releases
							</Button>
						</>
					) : null}

					{reactionError ? (
						<span className="text-destructive w-full font-mono text-[11px]">
							{reactionError}
						</span>
					) : null}
				</CardFooter>
			) : null}
		</Card>
	);
}
