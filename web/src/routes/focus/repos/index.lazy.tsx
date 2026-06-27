import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../-dashboardRoute";

const routeApi = getRouteApi("/focus/repos/");

export const Route = createLazyFileRoute("/focus/repos/")({
	component: FocusReposIndexRouteComponent,
});

function FocusReposIndexRouteComponent() {
	const search = routeApi.useSearch();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: "/focus/repos",
				search,
			})}
		/>
	);
}
