import { FileText, type LucideIcon, Languages, Sparkles } from "lucide-react";

import { isReleaseFeedItem, type FeedItem, type FeedLane } from "@/feed/types";

export const PAGE_DEFAULT_LANE_STORAGE_KEY =
	"octo-rill.dashboard.releaseDefaultLane";

export const DEFAULT_PAGE_LANE: FeedLane = "smart";

export const FEED_LANE_OPTIONS: Array<{
	lane: FeedLane;
	label: string;
	icon: LucideIcon;
}> = [
	{ lane: "original", label: "原文", icon: FileText },
	{ lane: "translated", label: "翻译", icon: Languages },
	{ lane: "smart", label: "智能", icon: Sparkles },
];

export function isFeedLane(value: unknown): value is FeedLane {
	return value === "original" || value === "translated" || value === "smart";
}

export function resolvePreferredLaneForItem(
	item: FeedItem,
	preferredLane: FeedLane,
): FeedLane {
	if (!isReleaseFeedItem(item)) {
		return "original";
	}
	if (item.smart?.status === "insufficient") {
		return "smart";
	}
	if (
		preferredLane === "translated" &&
		item.translated?.status === "disabled"
	) {
		return "original";
	}
	if (preferredLane === "smart" && item.smart?.status === "disabled") {
		return "original";
	}
	return preferredLane;
}

export function resolveDisplayLaneForFeed(
	items: FeedItem[],
	preferredLane: FeedLane,
): FeedLane {
	if (preferredLane === "original" || items.length === 0) {
		return preferredLane;
	}
	return items.some(
		(item) =>
			resolvePreferredLaneForItem(item, preferredLane) === preferredLane,
	)
		? preferredLane
		: "original";
}
