import { useEffect, useMemo, useRef } from "react";

import { FeedItemCard } from "@/feed/FeedItemCard";
import type { FeedItem, ReactionContent } from "@/feed/types";

function keyOf(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

export function FeedList(props: {
	items: FeedItem[];
	error: string | null;
	loadingInitial: boolean;
	loadingMore: boolean;
	hasMore: boolean;
	inFlightKeys: Set<string>;
	registerItemRef: (item: FeedItem) => (el: HTMLElement | null) => void;
	onLoadMore: () => void;
	showOriginalByKey: Record<string, boolean>;
	onToggleOriginal: (key: string) => void;
	onTranslateNow: (item: FeedItem) => void;
	reactionBusyKeys: Set<string>;
	onToggleReaction: (item: FeedItem, content: ReactionContent) => void;
	onSyncReleases: () => void;
}) {
	const {
		items,
		error,
		loadingInitial,
		loadingMore,
		hasMore,
		inFlightKeys,
		registerItemRef,
		onLoadMore,
		showOriginalByKey,
		onToggleOriginal,
		onTranslateNow,
		reactionBusyKeys,
		onToggleReaction,
		onSyncReleases,
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

			{items.map((item) => {
				const key = keyOf(item);
				const showOriginal = Boolean(showOriginalByKey[key]);
				const isTranslating = inFlightKeys.has(key);
				const isReactionBusy = reactionBusyKeys.has(key);
				return (
					<div key={key} ref={registerItemRef(item)}>
						<FeedItemCard
							item={item}
							showOriginal={showOriginal}
							isTranslating={isTranslating}
							isReactionBusy={isReactionBusy}
							onToggleOriginal={() => onToggleOriginal(key)}
							onTranslateNow={() => onTranslateNow(item)}
							onToggleReaction={(content) => onToggleReaction(item, content)}
							onSyncReleases={onSyncReleases}
						/>
					</div>
				);
			})}

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
