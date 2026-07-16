import { createFileRoute } from "@tanstack/react-router";

import { validateDashboardSearch } from "@/dashboard/routeState";

export const Route = createFileRoute("/$owner/$repo/discussions/$number")({
	validateSearch: validateDashboardSearch,
	pendingMs: 0,
	pendingMinMs: 200,
});
