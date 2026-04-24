import { createFileRoute } from "@tanstack/react-router";

import { validateDashboardSearch } from "@/dashboard/routeState";
import {
	DashboardRoutePendingComponent,
	primeDashboardRouteSurfaceForStartup,
} from "./-dashboardRoute";

primeDashboardRouteSurfaceForStartup();

export const Route = createFileRoute("/stars")({
	validateSearch: validateDashboardSearch,
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: DashboardRoutePendingComponent,
});
