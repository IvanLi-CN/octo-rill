import { normalizeReleaseId } from "@/lib/releaseId";
import type { DashboardTab } from "@/pages/DashboardControlBand";

export type DashboardRouteState = {
	tab: DashboardTab;
	activeReleaseId: string | null;
};

export function parseDashboardRouteState(search: {
	tab?: string | null;
	release?: string | null;
}): DashboardRouteState {
	const releaseId = normalizeReleaseId(search.release);
	if (releaseId) {
		return { tab: "briefs", activeReleaseId: releaseId };
	}

	const rawTab = search.tab;
	const tab: DashboardTab =
		rawTab === "releases" ||
		rawTab === "stars" ||
		rawTab === "followers" ||
		rawTab === "briefs" ||
		rawTab === "inbox"
			? rawTab
			: "all";

	return { tab, activeReleaseId: null };
}

export function buildDashboardSearch(routeState: DashboardRouteState) {
	if (routeState.activeReleaseId) {
		return {
			tab: "briefs" as const,
			release: routeState.activeReleaseId,
		};
	}

	return {
		tab: routeState.tab === "all" ? undefined : routeState.tab,
		release: undefined,
	};
}
