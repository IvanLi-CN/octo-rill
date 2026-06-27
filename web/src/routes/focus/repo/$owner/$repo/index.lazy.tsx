import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../../../-dashboardRoute";

const routeApi = getRouteApi("/focus/repo/$owner/$repo/");

export const Route = createLazyFileRoute("/focus/repo/$owner/$repo/")({
	component: FocusRepoIndexRouteComponent,
});

function FocusRepoIndexRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: `/focus/repo/${params.owner}/${params.repo}`,
				search,
			})}
		/>
	);
}
