import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/following/releases");

export const Route = createLazyFileRoute("/focus/following/releases")({
	component: FocusFollowingReleasesRouteComponent,
});

function FocusFollowingReleasesRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/following/releases",
				search,
			})}
		/>
	);
}
