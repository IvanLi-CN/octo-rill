import { createLazyFileRoute, getRouteApi } from "@tanstack/react-router";

import { parseDashboardRouteState } from "@/dashboard/routeState";
import { DashboardRouteShell } from "../../../-dashboardRoute";

const routeApi = getRouteApi("/focus/org/$org/releases");

export const Route = createLazyFileRoute("/focus/org/$org/releases")({
	component: FocusOrgReleasesRouteComponent,
});

function FocusOrgReleasesRouteComponent() {
	const search = routeApi.useSearch();
	const params = routeApi.useParams();
	return (
		<DashboardRouteShell
			routeState={parseDashboardRouteState({
				pathname: `/focus/org/${params.org}/releases`,
				search,
			})}
		/>
	);
}
