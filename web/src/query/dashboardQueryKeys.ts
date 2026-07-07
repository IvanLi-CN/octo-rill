import type { ReactionTokenStatusResponse } from "@/api";
import {
	buildDashboardScopeSignature,
	type DashboardScope,
} from "@/dashboard/routeState";
import type { FeedItem } from "@/feed/types";
import type { NotificationItem } from "@/sidebar/InboxQuickList";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";

export const DASHBOARD_QUERY_ROOT = "dashboard" as const;
export const DASHBOARD_QUERY_MAX_AGE_MS = 60 * 60 * 1000;
export const DASHBOARD_QUERY_STALE_MS = 60 * 1000;

export type DashboardFeedQueryData = {
	type: "all" | "releases" | "stars" | "followers";
	scopeSignature: string | null;
	items: FeedItem[];
	nextCursor: string | null;
};

export type DashboardBriefsQueryData = {
	items: BriefItem[];
	selectedBriefId: string | null;
};

export type DashboardNotificationsQueryData = {
	items: NotificationItem[];
};

export type DashboardReactionTokenQueryData = ReactionTokenStatusResponse;

export function dashboardFeedQueryKey(input: {
	userId: string;
	type: DashboardFeedQueryData["type"];
	scope: DashboardScope | null;
	cursor?: string | null;
}) {
	return [
		DASHBOARD_QUERY_ROOT,
		"feed",
		{
			userId: input.userId,
			type: input.type,
			scope: buildDashboardScopeSignature(input.scope),
			cursor: input.cursor ?? null,
		},
	] as const;
}

export function dashboardBriefsQueryKey(userId: string) {
	return [DASHBOARD_QUERY_ROOT, "briefs", { userId }] as const;
}

export function dashboardNotificationsQueryKey(userId: string) {
	return [DASHBOARD_QUERY_ROOT, "notifications", { userId }] as const;
}

export function dashboardReactionTokenQueryKey(userId: string) {
	return [DASHBOARD_QUERY_ROOT, "reaction-token-status", { userId }] as const;
}

export function shouldPersistDashboardQuery(queryKey: readonly unknown[]) {
	return queryKey[0] === DASHBOARD_QUERY_ROOT;
}
