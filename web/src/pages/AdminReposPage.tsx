import type { MeResponse } from "@/api";
import { AdminRepoGovernance } from "@/admin/AdminRepoGovernance";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

export function AdminReposPage({ me }: { me: MeResponse }) {
	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="repos" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="space-y-3 sm:space-y-4">
				<p className="text-muted-foreground text-sm">
					这里只看系统预算、闭环进度和仓库老化，不把交互刷新带来的表面新鲜度混进来。
				</p>
				<AdminRepoGovernance />
			</div>
		</AppShell>
	);
}
