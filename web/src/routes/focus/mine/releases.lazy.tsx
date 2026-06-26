import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/mine/releases");

export const Route = createLazyFileRoute("/focus/mine/releases")({
	component: FocusMineReleasesRouteComponent,
});

function FocusMineReleasesRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/mine/releases",
				search,
			})}
		/>
	);
}
