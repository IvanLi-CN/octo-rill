import {
	createLazyFileRoute,
	getRouteApi,
	useRouter,
} from "@tanstack/react-router";
import { useEffect } from "react";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { parseDashboardRouteState } from "@/dashboard/routeState";
import { AppBoot } from "@/pages/AppBoot";
import { PublicReleaseLoadingPage } from "@/pages/PublicReleaseLoadingPage";
import { validatePublicReleaseSearch } from "@/publicRelease/routeState";
import { DashboardRouteShell } from "../../../../-dashboardRoute";

const routeApi = getRouteApi("/$owner/$repo/releases/tag/$tag");

export const Route = createLazyFileRoute("/$owner/$repo/releases/tag/$tag")({
	component: DashboardReleaseRouteComponent,
});

function DashboardReleaseRouteComponent() {
	const auth = useAuthBootstrap();
	const router = useRouter();
	const params = routeApi.useParams();
	const search = routeApi.useSearch() as {
		from?: unknown;
		brief?: unknown;
		scope?: unknown;
		items?: unknown;
		org?: unknown;
		lane?: unknown;
	};
	const from = typeof search.from === "string" ? search.from : null;
	const brief = typeof search.brief === "string" ? search.brief : null;
	const scope = typeof search.scope === "string" ? search.scope : null;
	const items = typeof search.items === "string" ? search.items : null;
	const org = typeof search.org === "string" ? search.org : null;
	const lane = typeof search.lane === "string" ? search.lane : null;
	const authenticated = auth.isAuthenticated && Boolean(auth.me);
	const shouldRedirectPublic = auth.status !== "pending" && !authenticated;

	useEffect(() => {
		if (!shouldRedirectPublic || typeof window === "undefined") return;
		const currentSearch = new URLSearchParams(window.location.search);
		const publicSearch = validatePublicReleaseSearch({
			highlight: currentSearch.getAll("highlight"),
			highlight_start: currentSearch.get("highlight_start") ?? undefined,
			highlight_end: currentSearch.get("highlight_end") ?? undefined,
			highlight_active: currentSearch.get("highlight_active") ?? undefined,
		});
		void router.navigate({
			to: "/public/$owner/$repo/releases/tag/$tag",
			params: {
				owner: params.owner,
				repo: params.repo,
				tag: params.tag,
			},
			search: publicSearch as never,
			replace: true,
		});
	}, [params.owner, params.repo, params.tag, router, shouldRedirectPublic]);

	if (auth.status === "pending") {
		return <AppBoot />;
	}
	if (authenticated) {
		return (
			<DashboardRouteShell
				routeState={parseDashboardRouteState({
					search: { from, brief, scope, items, org, lane },
					owner: params.owner,
					repo: params.repo,
					tag: params.tag,
				})}
			/>
		);
	}
	return <PublicReleaseLoadingPage owner={params.owner} repo={params.repo} />;
}
