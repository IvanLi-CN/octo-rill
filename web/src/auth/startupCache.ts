import type { MeResponse } from "@/api";
import type { FeedItem } from "@/feed/types";
import type { NotificationItem } from "@/sidebar/InboxQuickList";
import type { BriefItem } from "@/sidebar/ReleaseDailyCard";

const AUTH_CACHE_KEY = "octo-rill.auth-bootstrap.v3";
const DASHBOARD_CACHE_KEY = "octo-rill.dashboard-warm.v1";
const ADMIN_USERS_CACHE_KEY = "octo-rill.admin-users-warm.v1";

export const STARTUP_WARM_TTL_MS = 60 * 60 * 1000;
const STARTUP_LAST_AUTH_TTL_MS = 30 * 24 * 60 * 60 * 1000;
const MAX_WARM_FEED_ITEMS = 8;
const MAX_WARM_NOTIFICATIONS = 8;
const MAX_WARM_BRIEFS = 8;
const MAX_WARM_ADMIN_USERS = 12;

export type StartupPresentation =
	| "cold-init"
	| "warm-cache"
	| "route-skeleton"
	| "live";
export type StartupRouteFamily =
	| "dashboard"
	| "admin-dashboard"
	| "admin-users"
	| "admin-jobs";
type FeedRequestType = "all" | "releases" | "stars" | "followers";

export type DashboardWarmRouteState = {
	tab: "all" | "releases" | "stars" | "followers" | "briefs" | "inbox";
	activeReleaseId: string | null;
};

type AuthCacheRecord = {
	savedAt: number;
	me: MeResponse;
};

type DashboardWarmCacheRecord = {
	savedAt: number;
	userId: string;
	routeState: DashboardWarmRouteState;
	feedRequestType: FeedRequestType;
	feedItems: FeedItem[];
	nextCursor: string | null;
	notifications: NotificationItem[];
	briefs: BriefItem[];
	selectedBriefId: string | null;
};

export type DashboardWarmSnapshot = DashboardWarmCacheRecord;

export type AdminUsersWarmItem = {
	id: string;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
	is_disabled: boolean;
	last_active_at: string | null;
	created_at: string;
	updated_at: string;
};

export type AdminUsersWarmSnapshot = {
	savedAt: number;
	userId: string;
	queryInput: string;
	query: string;
	role: "all" | "admin" | "user";
	status: "all" | "enabled" | "disabled";
	page: number;
	items: AdminUsersWarmItem[];
	total: number;
	guardSummary: {
		admin_total: number;
		active_admin_total: number;
	};
};

function canUseStorage() {
	return (
		typeof window !== "undefined" && typeof window.localStorage !== "undefined"
	);
}

function readStorageRecord<T>(key: string): T | null {
	if (!canUseStorage()) return null;
	try {
		const raw = window.localStorage.getItem(key);
		if (!raw) return null;
		return JSON.parse(raw) as T;
	} catch {
		return null;
	}
}

function writeStorageRecord<T>(key: string, value: T) {
	if (!canUseStorage()) return;
	try {
		window.localStorage.setItem(key, JSON.stringify(value));
	} catch {
		// Ignore cache write failures so boot never regresses into a fatal path.
	}
}

function removeStorageRecord(key: string) {
	if (!canUseStorage()) return;
	window.localStorage.removeItem(key);
}

function isFresh(savedAt: number, ttlMs: number, now = Date.now()) {
	return now - savedAt <= ttlMs;
}

export function deriveStartupRouteFamily(pathname: string): StartupRouteFamily {
	if (pathname.startsWith("/admin/jobs")) {
		return "admin-jobs";
	}
	if (pathname.startsWith("/admin/users")) {
		return "admin-users";
	}
	if (pathname.startsWith("/admin")) {
		return "admin-dashboard";
	}
	return "dashboard";
}

export function readStartupPresentationSeed(now = Date.now()): {
	me: MeResponse;
	presentation: Exclude<StartupPresentation, "live">;
} | null {
	const cached = readStorageRecord<AuthCacheRecord>(AUTH_CACHE_KEY);
	if (!cached?.me) return null;

	if (isFresh(cached.savedAt, STARTUP_WARM_TTL_MS, now)) {
		return {
			me: cached.me,
			presentation: "warm-cache",
		};
	}

	if (isFresh(cached.savedAt, STARTUP_LAST_AUTH_TTL_MS, now)) {
		return {
			me: cached.me,
			presentation: "route-skeleton",
		};
	}

	return null;
}

export function persistAuthenticatedStartup(
	me: MeResponse,
	savedAt = Date.now(),
) {
	writeStorageRecord<AuthCacheRecord>(AUTH_CACHE_KEY, {
		savedAt,
		me,
	});
}

export function clearStartupAuth() {
	removeStorageRecord(AUTH_CACHE_KEY);
}

export function clearAllWarmStartupCaches() {
	clearStartupAuth();
	removeStorageRecord(DASHBOARD_CACHE_KEY);
	removeStorageRecord(ADMIN_USERS_CACHE_KEY);
}

export function readDashboardWarmSnapshot(input: {
	userId: string;
	routeState: DashboardWarmRouteState;
	now?: number;
}) {
	const cached =
		readStorageRecord<DashboardWarmCacheRecord>(DASHBOARD_CACHE_KEY);
	if (!cached) return null;
	if (cached.userId !== input.userId) return null;
	if (!isFresh(cached.savedAt, STARTUP_WARM_TTL_MS, input.now)) return null;
	if (
		cached.routeState.tab !== input.routeState.tab ||
		cached.routeState.activeReleaseId !== input.routeState.activeReleaseId
	) {
		return null;
	}
	return cached;
}

export function persistDashboardWarmSnapshot(
	snapshot: Omit<DashboardWarmCacheRecord, "savedAt"> & { savedAt?: number },
) {
	writeStorageRecord<DashboardWarmCacheRecord>(DASHBOARD_CACHE_KEY, {
		savedAt: snapshot.savedAt ?? Date.now(),
		userId: snapshot.userId,
		routeState: snapshot.routeState,
		feedRequestType: snapshot.feedRequestType,
		feedItems: snapshot.feedItems.slice(0, MAX_WARM_FEED_ITEMS),
		nextCursor: snapshot.nextCursor,
		notifications: snapshot.notifications.slice(0, MAX_WARM_NOTIFICATIONS),
		briefs: snapshot.briefs.slice(0, MAX_WARM_BRIEFS),
		selectedBriefId: snapshot.selectedBriefId,
	});
}

export function readAdminUsersWarmSnapshot(input: {
	userId: string;
	now?: number;
}) {
	const cached = readStorageRecord<AdminUsersWarmSnapshot>(
		ADMIN_USERS_CACHE_KEY,
	);
	if (!cached) return null;
	if (cached.userId !== input.userId) return null;
	if (!isFresh(cached.savedAt, STARTUP_WARM_TTL_MS, input.now)) return null;
	return cached;
}

export function persistAdminUsersWarmSnapshot(
	snapshot: Omit<AdminUsersWarmSnapshot, "savedAt"> & { savedAt?: number },
) {
	writeStorageRecord<AdminUsersWarmSnapshot>(ADMIN_USERS_CACHE_KEY, {
		...snapshot,
		savedAt: snapshot.savedAt ?? Date.now(),
		items: snapshot.items.slice(0, MAX_WARM_ADMIN_USERS),
	});
}
