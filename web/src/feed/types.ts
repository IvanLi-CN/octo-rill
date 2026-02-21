export type TranslatedStatus = "ready" | "missing" | "disabled";

export type TranslatedItem = {
	lang: string;
	status: TranslatedStatus;
	title: string | null;
	summary: string | null;
};

export type FeedItemKind = "release" | "notification";

export type FeedItem = {
	kind: FeedItemKind;
	ts: string;
	id: string;
	repo_full_name: string | null;
	title: string | null;
	subtitle: string | null;
	reason: string | null;
	subject_type: string | null;
	html_url: string | null;
	unread: number | null;
	translated: TranslatedItem | null;
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
