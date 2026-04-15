import { useEffect, useMemo } from "react";
import { createFileRoute, useRouter } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import { readDashboardWarmSnapshot } from "@/auth/startupCache";
import {
	buildDashboardSearch,
	Dashboard,
	parseDashboardRouteState,
} from "@/pages/Dashboard";
import { Landing } from "@/pages/Landing";

export const Route = createFileRoute("/")({
	component: IndexRouteComponent,
	validateSearch: (search: Record<string, unknown>) => ({
		tab: typeof search.tab === "string" ? search.tab : undefined,
		release: typeof search.release === "string" ? search.release : undefined,
	}),
});

function IndexRouteComponent() {
	const auth = useAuthBootstrap();
	const router = useRouter();
	const search = Route.useSearch();
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
	const warmStart = useMemo(() => {
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

	if (!auth.isAuthenticated || !auth.me) {
		return <Landing bootError={auth.bootError} />;
	}

	return (
		<Dashboard
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
	);
}
