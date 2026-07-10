export type PublicReleaseSelector = string;

export const REPEATED_SEARCH_VALUE_SEPARATOR = "\u001f";

export type PublicReleaseHighlightSelection =
	| {
			mode: "discrete";
			selectors: PublicReleaseSelector[];
			active?: PublicReleaseSelector;
	  }
	| {
			mode: "range";
			start: PublicReleaseSelector;
			end: PublicReleaseSelector;
			active?: PublicReleaseSelector;
	  }
	| {
			mode: "invalid";
			selectors: PublicReleaseSelector[];
			start?: PublicReleaseSelector;
			end?: PublicReleaseSelector;
			active?: PublicReleaseSelector;
			reason: "mixed_modes" | "incomplete_range" | "too_many_targets";
	  }
	| null;

export type PublicReleaseListSearch = {
	highlight?: string | string[];
	highlight_start?: string;
	highlight_end?: string;
	highlight_active?: string;
};

function stringValues(value: unknown): string[] {
	if (typeof value === "string") {
		return value.split(REPEATED_SEARCH_VALUE_SEPARATOR);
	}
	if (Array.isArray(value)) {
		return value.filter((entry): entry is string => typeof entry === "string");
	}
	return [];
}

export function validatePublicReleaseSearch(
	search: Record<string, unknown>,
): PublicReleaseListSearch {
	const highlights = stringValues(search.highlight);
	return {
		highlight:
			highlights.length > 1
				? highlights
				: highlights.length === 1
					? highlights[0]
					: undefined,
		highlight_start:
			typeof search.highlight_start === "string"
				? search.highlight_start
				: undefined,
		highlight_end:
			typeof search.highlight_end === "string"
				? search.highlight_end
				: undefined,
		highlight_active:
			typeof search.highlight_active === "string"
				? search.highlight_active
				: undefined,
	};
}

export function parsePublicReleaseHighlight(
	search: PublicReleaseListSearch,
): PublicReleaseHighlightSelection {
	const selectors = stringValues(search.highlight);
	const hasStart = search.highlight_start !== undefined;
	const hasEnd = search.highlight_end !== undefined;
	const active = search.highlight_active;

	if (selectors.length > 0 && (hasStart || hasEnd)) {
		return {
			mode: "invalid",
			selectors,
			start: search.highlight_start,
			end: search.highlight_end,
			active,
			reason: "mixed_modes",
		};
	}
	if (selectors.length > 20) {
		return {
			mode: "invalid",
			selectors,
			active,
			reason: "too_many_targets",
		};
	}
	if (selectors.length > 0) {
		return { mode: "discrete", selectors, active };
	}
	if (hasStart !== hasEnd) {
		return {
			mode: "invalid",
			selectors: [],
			start: search.highlight_start,
			end: search.highlight_end,
			active,
			reason: "incomplete_range",
		};
	}
	if (hasStart && hasEnd) {
		return {
			mode: "range",
			start: search.highlight_start ?? "",
			end: search.highlight_end ?? "",
			active,
		};
	}
	return null;
}

export function publicReleaseHighlightSearch(
	highlight: PublicReleaseHighlightSelection,
): PublicReleaseListSearch {
	if (!highlight) return {};
	if (highlight.mode === "discrete") {
		return {
			highlight: highlight.selectors,
			highlight_active: highlight.active,
		};
	}
	if (highlight.mode === "range") {
		return {
			highlight_start: highlight.start,
			highlight_end: highlight.end,
			highlight_active: highlight.active,
		};
	}
	return {
		highlight: highlight.selectors,
		highlight_start: highlight.start,
		highlight_end: highlight.end,
		highlight_active: highlight.active,
	};
}

export function appendPublicReleaseHighlightParams(
	params: URLSearchParams,
	highlight: PublicReleaseHighlightSelection,
) {
	const search = publicReleaseHighlightSearch(highlight);
	for (const value of stringValues(search.highlight)) {
		params.append("highlight", value);
	}
	if (search.highlight_start) {
		params.set("highlight_start", search.highlight_start);
	}
	if (search.highlight_end) params.set("highlight_end", search.highlight_end);
	if (search.highlight_active) {
		params.set("highlight_active", search.highlight_active);
	}
	return params;
}
