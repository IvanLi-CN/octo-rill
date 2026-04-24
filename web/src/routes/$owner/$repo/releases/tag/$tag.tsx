import { createFileRoute } from "@tanstack/react-router";

import { validateDashboardSearch } from "@/dashboard/routeState";
import {
	DashboardRoutePendingComponent,
	primeDashboardRouteSurfaceForStartup,
} from "@/routes/-dashboardRoute";

primeDashboardRouteSurfaceForStartup();

export const Route = createFileRoute("/$owner/$repo/releases/tag/$tag")({
	validateSearch: validateDashboardSearch,
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: DashboardRoutePendingComponent,
});
