export type TranslatedStatus = "ready" | "missing" | "disabled";

export type TranslatedItem = {
	lang: string;
	status: TranslatedStatus;
	title: string | null;
	summary: string | null;
};

// Feed is releases-only (Inbox has its own API + UI tab).
export type FeedItemKind = "release";

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

export type FeedItem = {
	kind: FeedItemKind;
	ts: string;
	id: string;
	repo_full_name: string | null;
	title: string | null;
	excerpt: string | null;
	subtitle: string | null;
	reason: string | null;
	subject_type: string | null;
	html_url: string | null;
	unread: number | null;
	translated: TranslatedItem | null;
	reactions: ReleaseReactions | null;
};

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
	status: "ready" | "disabled" | "missing" | "error";
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
