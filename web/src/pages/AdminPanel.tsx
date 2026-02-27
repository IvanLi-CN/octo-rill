import { UserManagement } from "@/admin/UserManagement";
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

export function AdminPanel(props: { me: MeResponse }) {
	const { me } = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="users" />}
			footer={<AppMetaFooter />}
		>
			<div className="space-y-4">
				<p className="text-muted-foreground text-sm">
					这是独立的管理员界面，当前包含用户管理与任务中心两个模块。
				</p>
				<UserManagement currentUserId={me.user.id} />
			</div>
		</AppShell>
	);
}
