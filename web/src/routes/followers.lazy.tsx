import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/followers");

export const Route = createLazyFileRoute("/followers")({
	component: FollowersRouteComponent,
});

function FollowersRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({ pathname: "/followers", search })}
		/>
	);
}
