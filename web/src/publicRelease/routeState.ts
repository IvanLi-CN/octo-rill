export type PublicReleaseHighlightSelection =
	| { mode: "ids"; ids: string[] }
	| { mode: "range"; start: string; end: string }
	| { mode: "invalid"; ids?: string[]; start?: string; end?: string }
	| null;

export type PublicReleaseListSearch = {
	highlight_ids?: string;
	highlight_start?: string;
	highlight_end?: string;
};

export function validatePublicReleaseSearch(
	search: Record<string, unknown>,
): PublicReleaseListSearch {
	return {
		highlight_ids:
			typeof search.highlight_ids === "string"
				? search.highlight_ids
				: undefined,
		highlight_start:
			typeof search.highlight_start === "string"
				? search.highlight_start
				: undefined,
		highlight_end:
			typeof search.highlight_end === "string"
				? search.highlight_end
				: undefined,
	};
}

export function parsePublicReleaseHighlight(
	search: PublicReleaseListSearch,
): PublicReleaseHighlightSelection {
	const hasIds = search.highlight_ids !== undefined;
	const hasStart = search.highlight_start !== undefined;
	const hasEnd = search.highlight_end !== undefined;
	const ids = hasIds ? (search.highlight_ids ?? "").split(",") : undefined;

	if (hasIds && (hasStart || hasEnd)) {
		return {
			mode: "invalid",
			ids,
			start: search.highlight_start,
			end: search.highlight_end,
		};
	}
	if (hasIds) return { mode: "ids", ids: ids ?? [] };
	if (hasStart || hasEnd) {
		return {
			mode: "range",
			start: search.highlight_start ?? "",
			end: search.highlight_end ?? "",
		};
	}
	return null;
}
