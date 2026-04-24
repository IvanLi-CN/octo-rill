import { lazy, Suspense, useEffect, useMemo } from "react";
import { useRouter } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	readDashboardWarmSnapshot,
	readStartupPresentationSeed,
} from "@/auth/startupCache";
import {
	buildDashboardRouteUrl,
	buildDashboardWarmRouteState,
	isDashboardPathname,
	parseDashboardRouteStateFromLocation,
	type DashboardRouteState,
} from "@/dashboard/routeState";
import { AppBoot, DashboardStartupSkeleton } from "@/pages/AppBoot";

const loadDashboardRouteSurface = () => import("./-index.dashboard-surface");
const DashboardRouteSurface = lazy(loadDashboardRouteSurface);
const LandingRouteSurface = lazy(() => import("./-index.landing-surface"));

export function primeDashboardRouteSurfaceForStartup() {
	if (typeof window === "undefined") return;
	if (!isDashboardPathname(window.location.pathname)) return;
	const startupSeed = readStartupPresentationSeed();
	if (!startupSeed?.me) return;
	const startupRouteState = parseDashboardRouteStateFromLocation(
		window.location.pathname,
		window.location.search,
	);
	const warmSnapshot = readDashboardWarmSnapshot({
		userId: startupSeed.me.user.id,
		routeState: buildDashboardWarmRouteState(startupRouteState),
	});
	if (warmSnapshot) {
		void loadDashboardRouteSurface();
	}
}

export function DashboardRoutePendingComponent() {
	const auth = useAuthBootstrap();

	if (auth.isAuthenticated && auth.me) {
		return <DashboardStartupSkeleton me={auth.me} />;
	}

	return <AppBoot />;
}

export function DashboardRouteShell(props: {
	routeState: DashboardRouteState;
}) {
	const { routeState } = props;
	const auth = useAuthBootstrap();
	const router = useRouter();
	const canonicalUrl = useMemo(
		() => buildDashboardRouteUrl(routeState),
		[routeState],
	);
	const warmStart = useMemo(() => {
		if (!auth.me) return null;
		return readDashboardWarmSnapshot({
			userId: auth.me.user.id,
			routeState: buildDashboardWarmRouteState(routeState),
		});
	}, [auth.me, routeState]);

	useEffect(() => {
		if (typeof window === "undefined") return;
		if (routeState.activeReleaseId && !routeState.activeReleaseLocator) {
			return;
		}
		const currentUrl = `${window.location.pathname}${window.location.search}`;
		if (currentUrl === canonicalUrl) {
			return;
		}
		void router.navigate({
			to: canonicalUrl,
			replace: true,
		});
	}, [
		canonicalUrl,
		routeState.activeReleaseId,
		routeState.activeReleaseLocator,
		router,
	]);

	const fallback =
		auth.isAuthenticated && auth.me ? (
			<DashboardStartupSkeleton me={auth.me} />
		) : (
			<AppBoot />
		);

	if (auth.status === "pending") {
		return <AppBoot />;
	}

	if (!auth.isAuthenticated || !auth.me) {
		return (
			<Suspense fallback={fallback}>
				<LandingRouteSurface bootError={auth.bootError} />
			</Suspense>
		);
	}

	return (
		<Suspense fallback={fallback}>
			<DashboardRouteSurface
				me={auth.me}
				routeState={routeState}
				warmStart={auth.bootPresentation === "warm-cache" ? warmStart : null}
				onRouteStateChange={(nextRouteState, options) => {
					void router.navigate({
						to: buildDashboardRouteUrl(nextRouteState),
						replace: options?.replace,
					});
				}}
			/>
		</Suspense>
	);
}
