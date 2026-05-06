import { PublicReleaseRepoManagement } from "@/admin/PublicReleaseRepoManagement";
import type { MeResponse } from "@/api";
import { AdminHeader } from "@/layout/AdminHeader";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";

export function AdminPublicReleasesPage({ me }: { me: MeResponse }) {
	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="public-releases" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="space-y-3 sm:space-y-4">
				<p className="text-muted-foreground text-sm">
					这里展示公开 Release 端点自动登记的仓库与资源占用线索。
				</p>
				<PublicReleaseRepoManagement />
			</div>
		</AppShell>
	);
}
