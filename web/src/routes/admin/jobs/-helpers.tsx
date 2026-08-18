import { useEffect, useMemo } from "react";
import { useRouter } from "@tanstack/react-router";

import { useAuthBootstrap } from "@/auth/AuthBootstrap";
import {
	type AdminJobsPrimaryTab,
	type AdminJobsSearchInput,
	buildAdminJobsRouteState,
	buildAdminJobsRouteUrl,
	llmCallRouteFiltersToSearch,
	parseLlmCallRouteFilters,
	type AdminJobsRouteState,
	ADMIN_JOBS_BASE_PATH,
	ADMIN_JOBS_LLM_PATH,
	ADMIN_JOBS_SCHEDULED_PATH,
	ADMIN_JOBS_SUBSCRIPTIONS_PATH,
	ADMIN_JOBS_TRANSLATIONS_PATH,
} from "@/admin/jobsRouteState";
import { AdminJobs } from "@/pages/AdminJobs";
import { AdminJobsStartupSkeleton, AppBoot } from "@/pages/AppBoot";

import { useRequiredAdmin } from "../../-adminGuard";

function buildAdminJobsCanonicalSearch(
	routeState: AdminJobsRouteState,
): AdminJobsSearchInput {
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

	if (routeState.primaryTab === "llm") {
		return {
			from: undefined,
			view: undefined,
			...llmCallRouteFiltersToSearch(
				routeState.llmCallFilters ?? parseLlmCallRouteFilters({}),
			),
		};
	}

	return {
		from: undefined,
		view: undefined,
	};
}

function sameSearch(left: AdminJobsSearchInput, right: AdminJobsSearchInput) {
	return (
		(left.from ?? undefined) === (right.from ?? undefined) &&
		(left.view ?? undefined) === (right.view ?? undefined) &&
		(left.llm_status ?? undefined) === (right.llm_status ?? undefined) &&
		(left.llm_model ?? undefined) === (right.llm_model ?? undefined) &&
		(left.llm_source ?? undefined) === (right.llm_source ?? undefined) &&
		(left.llm_requested_by ?? undefined) ===
			(right.llm_requested_by ?? undefined) &&
		(left.llm_started_from ?? undefined) ===
			(right.llm_started_from ?? undefined) &&
		(left.llm_started_to ?? undefined) ===
			(right.llm_started_to ?? undefined) &&
		(left.llm_finished_from ?? undefined) ===
			(right.llm_finished_from ?? undefined) &&
		(left.llm_finished_before ?? undefined) ===
			(right.llm_finished_before ?? undefined) &&
		(left.llm_time_field ?? undefined) ===
			(right.llm_time_field ?? undefined) &&
		(left.llm_time_from ?? undefined) === (right.llm_time_from ?? undefined) &&
		(left.llm_time_to ?? undefined) === (right.llm_time_to ?? undefined)
	);
}

export function AdminJobsRoutePage(props: {
	primaryTab: AdminJobsPrimaryTab;
	search: AdminJobsSearchInput;
	taskId?: string;
	llmCallId?: string;
	subscriptionDetailTaskId?: string;
}) {
	const { primaryTab, search, taskId, llmCallId, subscriptionDetailTaskId } =
		props;
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
				subscriptionDetailTaskId,
			}),
		[llmCallId, primaryTab, search, subscriptionDetailTaskId, taskId],
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
	subscriptions: ADMIN_JOBS_SUBSCRIPTIONS_PATH,
	llm: ADMIN_JOBS_LLM_PATH,
	translations: ADMIN_JOBS_TRANSLATIONS_PATH,
} as const;
