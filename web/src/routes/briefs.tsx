import { createFileRoute } from "@tanstack/react-router";

import { validateDashboardSearch } from "@/dashboard/routeState";
import {
	DashboardRoutePendingComponent,
	primeDashboardRouteSurfaceForStartup,
} from "./-dashboardRoute";

primeDashboardRouteSurfaceForStartup();

export const Route = createFileRoute("/briefs")({
	validateSearch: validateDashboardSearch,
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: DashboardRoutePendingComponent,
});
