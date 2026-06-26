import type { DashboardScope } from "@/dashboard/routeState";

export const DASHBOARD_MINE_ENTRY_LABEL = "我的仓库动态";

export function resolveDashboardScopeRepoNames(
	scope: DashboardScope,
	feedRepoNames: string[],
) {
	switch (scope.kind) {
		case "repo":
			return [`${scope.owner}/${scope.repo}`];
		case "repos":
			return scope.items;
		case "org":
		case "mine":
			return feedRepoNames;
	}
}

export function buildDashboardScopeSummary(
	scope: DashboardScope,
	itemCount: number,
) {
	const itemLabel = itemCount > 0 ? `${itemCount} 个仓库` : "0 个仓库";

	switch (scope.kind) {
		case "repo":
			return {
				title:
					scope.owner === scope.repo
						? scope.owner
						: `${scope.owner}/${scope.repo}`,
				kicker: "仓库",
				description: "查看这个仓库的发布与相关动态。",
				chip: "仓库",
				secondary: itemLabel,
			};
		case "repos":
			return {
				title: "自定义仓库集合",
				kicker: "集合",
				description: "查看这组仓库的发布与相关动态。",
				chip: "集合",
				secondary: itemLabel,
			};
		case "org":
			return {
				title: scope.org,
				kicker: "组织",
				description: "查看这个组织下仓库的发布与相关动态。",
				chip: "组织",
				secondary: itemLabel,
			};
		case "mine":
			return {
				title: DASHBOARD_MINE_ENTRY_LABEL,
				kicker: "我的",
				description: "查看你的仓库发布与相关动态。",
				chip: "我的",
				secondary: itemLabel,
			};
	}
}
