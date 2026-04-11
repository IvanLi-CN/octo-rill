import { useEffect, useMemo, useRef } from "react";

import { FeedItemCard } from "@/feed/FeedItemCard";
import type {
	FeedItem,
	FeedLane,
	FeedViewer,
	ReactionContent,
} from "@/feed/types";

function keyOf(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

export type FeedCardListProps = {
	items: FeedItem[];
	currentViewer?: FeedViewer | null;
	translationInFlightKeys: Set<string>;
	smartInFlightKeys: Set<string>;
	registerItemRef: (item: FeedItem) => (el: HTMLElement | null) => void;
	selectedLaneByKey: Record<string, FeedLane>;
	onSelectLane: (item: FeedItem, lane: FeedLane) => void;
	onTranslateNow: (item: FeedItem) => void;
	onSmartNow: (item: FeedItem) => void;
	reactionBusyKeys: Set<string>;
	reactionErrorByKey: Record<string, string>;
	onToggleReaction: (item: FeedItem, content: ReactionContent) => void;
};

export function FeedItems(props: FeedCardListProps) {
	const {
		items,
		currentViewer,
		translationInFlightKeys,
		smartInFlightKeys,
		registerItemRef,
		selectedLaneByKey,
		onSelectLane,
		onTranslateNow,
		onSmartNow,
		reactionBusyKeys,
		reactionErrorByKey,
		onToggleReaction,
	} = props;

	return items.map((item) => {
		const key = keyOf(item);
		const activeLane = selectedLaneByKey[key] ?? "original";
		const isTranslating = translationInFlightKeys.has(key);
		const isSmartGenerating = smartInFlightKeys.has(key);
		const isReactionBusy = reactionBusyKeys.has(key);
		const reactionError = reactionErrorByKey[key] ?? null;
		return (
			<div key={key} ref={registerItemRef(item)}>
				<FeedItemCard
					item={item}
					currentViewer={currentViewer}
					activeLane={activeLane}
					isTranslating={isTranslating}
					isSmartGenerating={isSmartGenerating}
					isReactionBusy={isReactionBusy}
					reactionError={reactionError}
					onSelectLane={(lane) => onSelectLane(item, lane)}
					onTranslateNow={() => onTranslateNow(item)}
					onSmartNow={() => onSmartNow(item)}
					onToggleReaction={(content) => onToggleReaction(item, content)}
				/>
			</div>
		);
	});
}

export function FeedList(
	props: FeedCardListProps & {
		error: string | null;
		loadingInitial: boolean;
		loadingMore: boolean;
		hasMore: boolean;
		onLoadMore: () => void;
	},
) {
	const {
		items,
		currentViewer,
		error,
		loadingInitial,
		loadingMore,
		hasMore,
		translationInFlightKeys,
		smartInFlightKeys,
		registerItemRef,
		onLoadMore,
		...feedCardProps
	} = props;

	const sentinelRef = useRef<HTMLDivElement | null>(null);

	useEffect(() => {
		if (!hasMore || loadingInitial || loadingMore) return;
		const el = sentinelRef.current;
		if (!el) return;

		const obs = new IntersectionObserver(
			(entries) => {
				if (entries.some((e) => e.isIntersecting)) onLoadMore();
			},
			{ rootMargin: "900px 0px", threshold: 0.01 },
		);

		obs.observe(el);
		return () => obs.disconnect();
	}, [hasMore, loadingInitial, loadingMore, onLoadMore]);

	const skeletons = useMemo(() => Array.from({ length: 6 }, (_, i) => i), []);

	return (
		<div className="space-y-4">
			{error ? <p className="text-destructive text-sm">{error}</p> : null}

			{loadingInitial && items.length === 0 ? (
				<div className="space-y-4">
					{skeletons.map((i) => (
						<div
							key={i}
							className="bg-card/70 animate-pulse rounded-xl border p-6 shadow-sm"
						>
							<div className="h-3 w-48 rounded bg-muted" />
							<div className="mt-4 h-5 w-3/4 rounded bg-muted" />
							<div className="mt-2 h-3 w-2/3 rounded bg-muted" />
						</div>
					))}
				</div>
			) : null}

			<FeedItems
				items={items}
				currentViewer={currentViewer}
				translationInFlightKeys={translationInFlightKeys}
				smartInFlightKeys={smartInFlightKeys}
				registerItemRef={registerItemRef}
				{...feedCardProps}
			/>

			<div ref={sentinelRef} />

			{loadingMore ? (
				<p className="text-muted-foreground font-mono text-xs">加载中…</p>
			) : null}

			{!hasMore && items.length > 0 ? (
				<p className="text-muted-foreground font-mono text-xs">
					已到尽头（共 {items.length} 条）
				</p>
			) : null}
		</div>
	);
}
