import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "@/routes/-dashboardRoute";

const routeApi = getRouteApi("/$owner/$repo/releases/tag/$tag");

export const Route = createLazyFileRoute("/$owner/$repo/releases/tag/$tag")({
	component: DashboardReleaseRouteComponent,
});

function DashboardReleaseRouteComponent() {
	const params = routeApi.useParams();
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				owner: params.owner,
				repo: params.repo,
				tag: params.tag,
				from: search.from,
			})}
		/>
	);
}
