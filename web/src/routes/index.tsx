import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	readDashboardWarmSnapshot,
	readStartupPresentationSeed,
} from "@/auth/startupCache";
import { createFileRoute } from "@tanstack/react-router";
import { parseDashboardRouteState } from "@/dashboard/routeState";
import { AppBoot, DashboardStartupSkeleton } from "@/pages/AppBoot";

const startupSeed =
	typeof window === "undefined" ? null : readStartupPresentationSeed();
const startupRouteState =
	typeof window === "undefined"
		? null
		: parseDashboardRouteState({
				tab: new URLSearchParams(window.location.search).get("tab"),
				release: new URLSearchParams(window.location.search).get("release"),
			});
const startupWarmSnapshot =
	typeof window === "undefined" || !startupSeed?.me || !startupRouteState
		? null
		: readDashboardWarmSnapshot({
				userId: startupSeed.me.user.id,
				routeState: {
					tab: startupRouteState.tab,
					activeReleaseId: startupRouteState.activeReleaseId,
				},
			});
const shouldWarmDashboardSurface =
	typeof window !== "undefined" &&
	window.location.pathname === "/" &&
	startupWarmSnapshot !== null;
if (shouldWarmDashboardSurface) {
	void import("./-index.dashboard-surface");
}

export const Route = createFileRoute("/")({
	validateSearch: (search: Record<string, unknown>) => ({
		tab: typeof search.tab === "string" ? search.tab : undefined,
		release: typeof search.release === "string" ? search.release : undefined,
	}),
	pendingMs: 0,
	pendingMinMs: 200,
	pendingComponent: IndexRoutePendingComponent,
});

function IndexRoutePendingComponent() {
	const auth = useAuthBootstrap();

	if (auth.isAuthenticated && auth.me) {
		return <DashboardStartupSkeleton me={auth.me} />;
	}

	return <AppBoot />;
}
