import {
	UserManagement,
	type UserManagementStoryState,
} from "@/admin/UserManagement";
import type { AdminUsersWarmSnapshot } from "@/auth/startupCache";
import type { MeResponse } from "@/api";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

export function AdminPanel(props: {
	me: MeResponse;
	userManagementStoryState?: UserManagementStoryState;
	userManagementWarmStart?: AdminUsersWarmSnapshot | null;
}) {
	const {
		me,
		userManagementStoryState,
		userManagementWarmStart = null,
	} = props;

	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="users" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="space-y-3 sm:space-y-4">
				<p className="text-muted-foreground text-sm">
					这里聚焦用户资料与账号状态治理；若需查看整体运营指标，请切换到仪表盘。
				</p>
				<UserManagement
					currentUserId={me.user.id}
					storyState={userManagementStoryState}
					warmStart={userManagementWarmStart}
				/>
			</div>
		</AppShell>
	);
}
