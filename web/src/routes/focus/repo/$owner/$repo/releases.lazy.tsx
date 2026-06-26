import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../../../-dashboardRoute";

const routeApi = getRouteApi("/focus/repo/$owner/$repo/releases");

export const Route = createLazyFileRoute("/focus/repo/$owner/$repo/releases")({
	component: FocusRepoReleasesRouteComponent,
});

function FocusRepoReleasesRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: `/focus/repo/${params.owner}/${params.repo}/releases`,
				search,
			})}
		/>
	);
}
