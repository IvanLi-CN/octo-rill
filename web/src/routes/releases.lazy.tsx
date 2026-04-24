import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/releases");

export const Route = createLazyFileRoute("/releases")({
	component: ReleasesRouteComponent,
});

function ReleasesRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({ pathname: "/releases", search })}
		/>
	);
}
