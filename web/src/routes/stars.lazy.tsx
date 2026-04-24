import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/stars");

export const Route = createLazyFileRoute("/stars")({
	component: StarsRouteComponent,
});

function StarsRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({ pathname: "/stars", search })}
		/>
	);
}
