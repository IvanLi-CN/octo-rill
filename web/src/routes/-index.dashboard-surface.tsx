import type { DashboardWarmSnapshot } from "@/auth/startupCache";
import type { DashboardRouteState } from "@/dashboard/routeState";
import { Dashboard } from "@/pages/Dashboard";
import type { MeResponse } from "@/api";

export default function DashboardRouteSurface(props: {
	me: MeResponse;
	routeState: DashboardRouteState;
	warmStart: DashboardWarmSnapshot | null;
	onRouteStateChange: (
		nextRouteState: DashboardRouteState,
		options?: {
			replace?: boolean;
		},
	) => void;
}) {
	return <Dashboard {...props} />;
}
