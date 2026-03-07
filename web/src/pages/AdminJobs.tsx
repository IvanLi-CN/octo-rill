import { JobManagement } from "@/admin/JobManagement";
import type { MeResponse } from "@/api";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";

export function AdminJobs(props: { me: MeResponse }) {
	const { me } = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="jobs" />}
			footer={<AppMetaFooter />}
		>
			<JobManagement currentUserId={me.user.id} />
		</AppShell>
	);
}
