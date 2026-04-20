import { lazy, Suspense, useEffect, useMemo } from "react";
import {
	createLazyFileRoute,
	getRouteApi,
	useRouter,
} from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	type DashboardWarmSnapshot,
	readDashboardWarmSnapshot,
} from "@/auth/startupCache";
import {
	buildDashboardSearch,
	parseDashboardRouteState,
} from "@/dashboard/routeState";
import { AppBoot, DashboardStartupSkeleton } from "@/pages/AppBoot";

const routeApi = getRouteApi("/");
const loadDashboardRouteSurface = () => import("./-index.dashboard-surface");

const DashboardRouteSurface = lazy(loadDashboardRouteSurface);
const LandingRouteSurface = lazy(() => import("./-index.landing-surface"));

export const Route = createLazyFileRoute("/")({
	component: IndexRouteComponent,
});

function IndexRouteComponent() {
	const auth = useAuthBootstrap();
	const router = useRouter();
	const search = routeApi.useSearch();
	const routeState = useMemo(
		() =>
			parseDashboardRouteState({
				tab: search.tab,
				release: search.release,
			}),
		[search.release, search.tab],
	);
	const canonicalSearch = useMemo(
		() => buildDashboardSearch(routeState),
		[routeState],
	);
	const warmStart = useMemo<DashboardWarmSnapshot | null>(() => {
		if (!auth.me) return null;
		return readDashboardWarmSnapshot({
			userId: auth.me.user.id,
			routeState: {
				tab: routeState.tab,
				activeReleaseId: routeState.activeReleaseId,
			},
		});
	}, [auth.me, routeState.activeReleaseId, routeState.tab]);

	useEffect(() => {
		if (
			(search.tab ?? undefined) === canonicalSearch.tab &&
			(search.release ?? undefined) === canonicalSearch.release
		) {
			return;
		}
		void router.navigate({
			to: "/",
			search: canonicalSearch as never,
			replace: true,
		});
	}, [canonicalSearch, router, search.release, search.tab]);

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
						to: "/",
						search: buildDashboardSearch(nextRouteState) as never,
						replace: options?.replace,
					});
				}}
			/>
		</Suspense>
	);
}
