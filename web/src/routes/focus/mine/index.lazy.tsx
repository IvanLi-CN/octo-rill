import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/mine/");

export const Route = createLazyFileRoute("/focus/mine/")({
	component: FocusMineIndexRouteComponent,
});

function FocusMineIndexRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/mine",
				search,
			})}
		/>
	);
}
