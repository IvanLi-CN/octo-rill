import type { RepoVisual } from "@/lib/repoVisual";

export type TranslatedStatus = "ready" | "missing" | "disabled" | "error";
export type SmartStatus =
	| "ready"
	| "missing"
	| "disabled"
	| "error"
	| "insufficient";

export type TranslatedItem = {
	lang: string;
	status: TranslatedStatus;
	title: string | null;
	summary: string | null;
	error_code?: string | null;
	error_summary?: string | null;
	error_detail?: string | null;
	auto_translate?: boolean;
};

export type SmartItem = {
	lang: string;
	status: SmartStatus;
	title: string | null;
	summary: string | null;
	error_code?: string | null;
	error_summary?: string | null;
	error_detail?: string | null;
	auto_translate?: boolean;
};

export type FeedLane = "original" | "translated" | "smart";
export type FeedItemKind =
	| "release"
	| "repo_star_received"
	| "follower_received";

export type FeedActor = {
	login: string;
	avatar_url?: string | null;
	html_url?: string | null;
};

export type FeedViewer = FeedActor;

export type ReactionContent =
	| "plus1"
	| "laugh"
	| "heart"
	| "hooray"
	| "rocket"
	| "eyes";

export type ReactionCounts = {
	plus1: number;
	laugh: number;
	heart: number;
	hooray: number;
	rocket: number;
	eyes: number;
};

export type ReactionViewerState = {
	plus1: boolean;
	laugh: boolean;
	heart: boolean;
	hooray: boolean;
	rocket: boolean;
	eyes: boolean;
};

export type ReleaseReactions = {
	counts: ReactionCounts;
	viewer: ReactionViewerState;
	status: "ready" | "sync_required";
};

type FeedItemBase = {
	kind: FeedItemKind;
	ts: string;
	id: string;
	repo_full_name: string | null;
	repo_visual: RepoVisual | null;
	title: string | null;
	body: string | null;
	body_truncated: boolean;
	subtitle: string | null;
	reason: string | null;
	subject_type: string | null;
	html_url: string | null;
	unread: number | null;
};

export type ReleaseFeedItem = FeedItemBase & {
	kind: "release";
	actor?: null | undefined;
	translated: TranslatedItem | null;
	smart: SmartItem | null;
	reactions: ReleaseReactions | null;
};

export type SocialFeedItem = FeedItemBase & {
	kind: "repo_star_received" | "follower_received";
	actor: FeedActor;
	translated: null;
	smart: null;
	reactions: null;
};

export type FeedItem = ReleaseFeedItem | SocialFeedItem;

export function isReleaseFeedItem(item: FeedItem): item is ReleaseFeedItem {
	return item.kind === "release";
}

export function isSocialFeedItem(item: FeedItem): item is SocialFeedItem {
	return (
		item.kind === "repo_star_received" || item.kind === "follower_received"
	);
}

export type FeedResponse = {
	items: FeedItem[];
	next_cursor: string | null;
};

export type TranslateResponse = {
	lang: string;
	status: "ready" | "disabled";
	title: string | null;
	summary: string | null;
};

export type TranslateBatchItem = {
	id: string;
	lang: string;
	status: "ready" | "disabled" | "missing" | "error" | "processing";
	title: string | null;
	summary: string | null;
	error: string | null;
};

export type TranslateBatchResponse = {
	items: TranslateBatchItem[];
};

export type TranslateBatchStreamEvent = {
	event: "item" | "done" | "error";
	item?: TranslateBatchItem | null;
	error?: string | null;
};

export type ToggleReleaseReactionResponse = {
	release_id: string;
	reactions: ReleaseReactions;
};
