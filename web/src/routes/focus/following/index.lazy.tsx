import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/following/");

export const Route = createLazyFileRoute("/focus/following/")({
	component: FocusFollowingIndexRouteComponent,
});

function FocusFollowingIndexRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/following",
				search,
			})}
		/>
	);
}
