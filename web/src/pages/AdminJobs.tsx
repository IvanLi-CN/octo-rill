import { type AdminJobsRouteState, JobManagement } from "@/admin/JobManagement";
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
}) {
	const { me, routeState, onNavigateRoute } = props;

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
			/>
		</AppShell>
	);
}
