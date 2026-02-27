import { JobManagement } from "@/admin/JobManagement";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";

type MeResponse = {
	user: {
		id: number;
		github_user_id: number;
		login: string;
		name: string | null;
		avatar_url: string | null;
		email: string | null;
		is_admin: boolean;
	};
};

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
