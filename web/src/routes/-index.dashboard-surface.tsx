import type { DashboardWarmSnapshot } from "@/auth/startupCache";
import type { DashboardRouteState } from "@/dashboard/routeState";
import { Dashboard } from "@/pages/Dashboard";
import type { MeResponse } from "@/api";
import type { NetworkErrorKind } from "@/lib/errorPresentation";

export default function DashboardRouteSurface(props: {
	me: MeResponse;
	routeState: DashboardRouteState;
	warmStart: DashboardWarmSnapshot | null;
	bootError: string | null;
	bootErrorKind: NetworkErrorKind | null;
	bootErrorDetail: string | null;
	onRetryBoot: () => unknown | Promise<unknown>;
	onRouteStateChange: (
		nextRouteState: DashboardRouteState,
		options?: {
			replace?: boolean;
		},
	) => void;
}) {
	return <Dashboard {...props} />;
}
