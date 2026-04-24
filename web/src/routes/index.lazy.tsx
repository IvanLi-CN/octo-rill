import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "./-dashboardRoute";

const routeApi = getRouteApi("/");

export const Route = createLazyFileRoute("/")({
	component: IndexRouteComponent,
});

function IndexRouteComponent() {
	const search = routeApi.useSearch();
	const routeState = parseDashboardRouteState({
		pathname: "/",
		search,
	});
	return <DashboardRouteShell routeState={routeState} />;
}
