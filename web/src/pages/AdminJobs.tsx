import { JobManagement } from "@/admin/JobManagement";
import type { AdminJobsRouteState } from "@/admin/jobsRouteState";
import type { MeResponse } from "@/api";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

export function AdminJobs(props: {
	me: MeResponse;
	routeState?: AdminJobsRouteState;
	onNavigateRoute?: (
		nextRoute: AdminJobsRouteState,
		options?: {
			replace?: boolean;
		},
	) => void;
	syncSettingsDialogDefaultOpen?: boolean;
	syncSettingsHelpTooltipsOpen?: boolean;
}) {
	const {
		me,
		routeState,
		onNavigateRoute,
		syncSettingsDialogDefaultOpen,
		syncSettingsHelpTooltipsOpen,
	} = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="jobs" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<JobManagement
				currentUserId={me.user.id}
				routeState={routeState}
				onNavigateRoute={onNavigateRoute}
				syncSettingsDialogDefaultOpen={syncSettingsDialogDefaultOpen}
				syncSettingsHelpTooltipsOpen={syncSettingsHelpTooltipsOpen}
			/>
		</AppShell>
	);
}
