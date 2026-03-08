import {
	UserManagement,
	type UserManagementStoryState,
} from "@/admin/UserManagement";
import type { MeResponse } from "@/api";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";

export function AdminPanel(props: {
	me: MeResponse;
	userManagementStoryState?: UserManagementStoryState;
}) {
	const { me, userManagementStoryState } = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="users" />}
			footer={<AppMetaFooter />}
		>
			<div className="space-y-4">
				<p className="text-muted-foreground text-sm">
					这是独立的管理员界面，当前包含用户管理与任务中心两个模块。
				</p>
				<UserManagement
					currentUserId={me.user.id}
					storyState={userManagementStoryState}
				/>
			</div>
		</AppShell>
	);
}
