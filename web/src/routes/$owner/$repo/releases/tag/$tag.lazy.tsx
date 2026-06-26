import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { parseDashboardRouteState } from "@/dashboard/routeState";
import { AppBoot } from "@/pages/AppBoot";
import { PublicReleasePage } from "@/pages/PublicReleasePage";
import { DashboardRouteShell } from "../../../../-dashboardRoute";

const routeApi = getRouteApi("/$owner/$repo/releases/tag/$tag");

export const Route = createLazyFileRoute("/$owner/$repo/releases/tag/$tag")({
	component: DashboardReleaseRouteComponent,
});

function DashboardReleaseRouteComponent() {
	const auth = useAuthBootstrap();
	const params = routeApi.useParams();
	const search = routeApi.useSearch() as {
		from?: unknown;
		scope?: unknown;
		items?: unknown;
		org?: unknown;
	};
	const from = typeof search.from === "string" ? search.from : null;
	const scope = typeof search.scope === "string" ? search.scope : null;
	const items = typeof search.items === "string" ? search.items : null;
	const org = typeof search.org === "string" ? search.org : null;
	if (auth.status === "pending") {
		return <AppBoot />;
	}
	if (auth.isAuthenticated && auth.me) {
		return (
			<DashboardRouteShell
				routeState={parseDashboardRouteState({
					search: { from, scope, items, org },
					owner: params.owner,
					repo: params.repo,
					tag: params.tag,
				})}
			/>
		);
	}
	return (
		<PublicReleasePage
			owner={params.owner}
			repo={params.repo}
			tag={params.tag}
		/>
	);
}
