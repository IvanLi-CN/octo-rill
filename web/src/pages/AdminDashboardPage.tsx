import type { MeResponse } from "@/api";
import { AdminDashboard } from "@/admin/AdminDashboard";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

export function AdminDashboardPage(props: { me: MeResponse }) {
	const { me } = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="dashboard" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<AdminDashboard />
		</AppShell>
	);
}
