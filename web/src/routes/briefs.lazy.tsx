import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/briefs");

export const Route = createLazyFileRoute("/briefs")({
	component: BriefsRouteComponent,
});

function BriefsRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({ pathname: "/briefs", search })}
		/>
	);
}
