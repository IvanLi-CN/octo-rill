import { ArrowUpRight, Languages, RefreshCcw } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import type { FeedItem } from "@/feed/types";
import { cn } from "@/lib/utils";

function formatIsoShort(iso: string) {
	// "2026-02-21T08:00:00Z" -> "2026-02-21 08:00:00"
	// "2026-02-21T08:00:00.123Z" -> "2026-02-21 08:00:00"
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

function renderSummary(summary: string) {
	const lines = summary
		.split("\n")
		.map((l) => l.trim())
		.filter(Boolean);

	const looksLikeBullets = lines.some((l) => l.startsWith("- "));
	if (!looksLikeBullets) {
		return (
			<div className="text-muted-foreground text-sm whitespace-pre-wrap">
				{summary}
			</div>
		);
	}

	return (
		<ul className="text-muted-foreground list-disc space-y-1 pl-5 text-sm">
			{lines.map((l) => (
				<li key={l}>{l.replace(/^- /, "")}</li>
			))}
		</ul>
	);
}

export function FeedItemCard(props: {
	item: FeedItem;
	showOriginal: boolean;
	isTranslating: boolean;
	onToggleOriginal: () => void;
	onTranslateNow: () => void;
}) {
	const {
		item,
		showOriginal,
		isTranslating,
		onToggleOriginal,
		onTranslateNow,
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

	const subtitleBits = [
		item.reason || item.subtitle,
		item.subject_type ? item.subject_type : null,
	].filter(Boolean);
	const subtitle = subtitleBits.join(" · ");

	const showTranslationControls = Boolean(item.translated);
	const aiDisabled = item.translated?.status === "disabled";
	const canTranslate =
		!aiDisabled && item.translated?.status === "missing" && !isTranslating;

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

			{!showOriginal && translatedSummary ? (
				<CardContent className="pt-0">
					{renderSummary(translatedSummary)}
				</CardContent>
			) : null}
		</Card>
	);
}
