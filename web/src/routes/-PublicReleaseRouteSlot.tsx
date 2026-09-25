import { lazy, Suspense } from "react";
import { Outlet, useRouterState } from "@tanstack/react-router";

import type { PublicReleaseListSearch } from "@/publicRelease/routeState";
import { parsePublicReleaseHighlight } from "@/publicRelease/routeState";
import { PublicReleaseLoadingPage } from "@/pages/PublicReleaseLoadingPage";

const LazyPublicReleasePage = lazy(async () => {
	const module = await import("@/pages/PublicReleasePage");
	return { default: module.PublicReleasePage };
});

const PUBLIC_RELEASE_LIST_ROUTE_ID = "/$owner/$repo/releases/";
const PUBLIC_RELEASE_TAG_ROUTE_ID = "/public/$owner/$repo/releases/tag/$tag";

type PublicReleaseRouteMatch = {
	routeId: string;
	params: Record<string, string>;
	search: PublicReleaseListSearch;
};

export function PublicReleaseRouteSlot() {
	const matches = useRouterState({ select: (state) => state.matches });
	const readerMatch = matches.find(
		(match) =>
			match.routeId === PUBLIC_RELEASE_LIST_ROUTE_ID ||
			match.routeId === PUBLIC_RELEASE_TAG_ROUTE_ID,
	) as PublicReleaseRouteMatch | undefined;

	if (!readerMatch) return <Outlet />;

	const { owner, repo, tag } = readerMatch.params;
	const isTagRoute = readerMatch.routeId === PUBLIC_RELEASE_TAG_ROUTE_ID;
	const readerProps = {
		owner,
		repo,
		tag: isTagRoute ? tag : null,
		highlight: parsePublicReleaseHighlight(readerMatch.search),
	};

	return (
		<Suspense fallback={<PublicReleaseLoadingPage owner={owner} repo={repo} />}>
			<LazyPublicReleasePage key={`${owner}/${repo}`} {...readerProps} />
		</Suspense>
	);
}
