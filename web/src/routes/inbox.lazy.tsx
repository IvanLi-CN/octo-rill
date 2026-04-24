import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/inbox");

export const Route = createLazyFileRoute("/inbox")({
	component: InboxRouteComponent,
});

function InboxRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({ pathname: "/inbox", search })}
		/>
	);
}
