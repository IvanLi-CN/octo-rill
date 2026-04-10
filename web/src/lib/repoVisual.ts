export type RepoVisual = {
	owner_avatar_url: string | null;
	open_graph_image_url: string | null;
	uses_custom_open_graph_image: boolean;
};

export type RepoVisualCandidate = {
	kind: "owner_avatar";
	src: string;
};

function normalizeVisualUrl(raw: string | null | undefined) {
	const trimmed = raw?.trim();
	return trimmed ? trimmed : null;
}

export function resolveRepoVisualCandidates(
	repoVisual: RepoVisual | null | undefined,
): RepoVisualCandidate[] {
	if (!repoVisual) return [];

	const candidates: RepoVisualCandidate[] = [];
	const seen = new Set<string>();
	const push = (kind: RepoVisualCandidate["kind"], src: string | null) => {
		if (!src || seen.has(src)) return;
		seen.add(src);
		candidates.push({ kind, src });
	};

	const ownerAvatarUrl = normalizeVisualUrl(repoVisual.owner_avatar_url);

	push("owner_avatar", ownerAvatarUrl);

	return candidates;
}
