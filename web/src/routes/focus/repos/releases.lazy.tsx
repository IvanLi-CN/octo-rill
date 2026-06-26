import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/repos/releases");

export const Route = createLazyFileRoute("/focus/repos/releases")({
	component: FocusReposReleasesRouteComponent,
});

function FocusReposReleasesRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/repos/releases",
				search,
			})}
		/>
	);
}
