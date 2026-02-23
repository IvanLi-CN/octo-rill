export type ReleaseDetail = {
	release_id: string;
	full_name: string;
	tag_name: string;
	name: string | null;
	title: string;
	body: string;
	html_url: string;
	published_at: string | null;
	is_prerelease: number;
	is_draft: number;
};

export type ReleaseDetailTranslateResponse = {
	lang: string;
	status: "ready" | "disabled";
	title: string | null;
	body_markdown: string | null;
};
