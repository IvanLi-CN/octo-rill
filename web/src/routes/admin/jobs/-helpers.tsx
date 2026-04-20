import { useEffect, useMemo } from "react";
import { useRouter } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	type AdminJobsPrimaryTab,
	type AdminJobsSearchInput,
	buildAdminJobsRouteState,
	buildAdminJobsRouteUrl,
	type AdminJobsRouteState,
	ADMIN_JOBS_BASE_PATH,
	ADMIN_JOBS_LLM_PATH,
	ADMIN_JOBS_SCHEDULED_PATH,
	ADMIN_JOBS_TRANSLATIONS_PATH,
} from "@/admin/jobsRouteState";
import { AdminJobs } from "@/pages/AdminJobs";
import { AdminJobsStartupSkeleton, AppBoot } from "@/pages/AppBoot";

import { useRequiredAdmin } from "../../-adminGuard";

function buildAdminJobsCanonicalSearch(routeState: AdminJobsRouteState) {
	if (routeState.taskDrawerRoute) {
		if (routeState.drawerFromTab === "translations") {
			return {
				from: "translations",
				view: routeState.translationView,
			};
		}
		if (routeState.drawerFromTab) {
			return {
				from: routeState.drawerFromTab,
				view: undefined,
			};
		}
		return {
			from: undefined,
			view: undefined,
		};
	}

	if (routeState.primaryTab === "translations") {
		return {
			from: undefined,
			view: routeState.translationView,
		};
	}

	return {
		from: undefined,
		view: undefined,
	};
}

function sameSearch(
	left: AdminJobsSearchInput,
	right: {
		from?: string;
		view?: string;
	},
) {
	return (
		(left.from ?? undefined) === (right.from ?? undefined) &&
		(left.view ?? undefined) === (right.view ?? undefined)
	);
}

export function AdminJobsRoutePage(props: {
	primaryTab: AdminJobsPrimaryTab;
	search: AdminJobsSearchInput;
	taskId?: string;
	llmCallId?: string;
}) {
	const { primaryTab, search, taskId, llmCallId } = props;
	const auth = useAuthBootstrap();
	const me = useRequiredAdmin();
	const router = useRouter();
	const routeState = useMemo(
		() =>
			buildAdminJobsRouteState({
				primaryTab,
				search,
				taskId,
				llmCallId,
			}),
		[llmCallId, primaryTab, search, taskId],
	);
	const canonicalSearch = useMemo(
		() => buildAdminJobsCanonicalSearch(routeState),
		[routeState],
	);

	useEffect(() => {
		if (sameSearch(search, canonicalSearch)) return;
		const canonicalUrl = buildAdminJobsRouteUrl({
			...routeState,
			translationView:
				canonicalSearch.view === "history"
					? "history"
					: routeState.translationView,
			drawerFromTab:
				canonicalSearch.from === undefined
					? routeState.drawerFromTab
					: canonicalSearch.from === "realtime" ||
							canonicalSearch.from === "scheduled" ||
							canonicalSearch.from === "llm" ||
							canonicalSearch.from === "translations"
						? canonicalSearch.from
						: routeState.drawerFromTab,
		});
		void router.navigate({
			href: canonicalUrl,
			replace: true,
		});
	}, [canonicalSearch, routeState, router, search]);

	if (auth.isBootstrapping && auth.bootPresentation !== "live" && !me) {
		return <AppBoot />;
	}

	if (!me) {
		return null;
	}

	if (auth.isBootstrapping && auth.bootPresentation !== "live") {
		return <AdminJobsStartupSkeleton me={me} />;
	}

	return (
		<AdminJobs
			me={me}
			routeState={routeState}
			onNavigateRoute={(nextRoute, options) => {
				const routeUrl = buildAdminJobsRouteUrl(nextRoute);
				void router.navigate({
					href: routeUrl,
					replace: options?.replace,
				});
			}}
		/>
	);
}

export const ADMIN_JOBS_ROUTE_PATHS = {
	realtime: ADMIN_JOBS_BASE_PATH,
	scheduled: ADMIN_JOBS_SCHEDULED_PATH,
	llm: ADMIN_JOBS_LLM_PATH,
	translations: ADMIN_JOBS_TRANSLATIONS_PATH,
} as const;
