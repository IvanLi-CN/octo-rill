import { JobManagement } from "@/admin/JobManagement";
import { Button } from "@/components/ui/button";
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
			header={
				<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="min-w-0">
						<div className="flex flex-wrap items-center gap-2">
							<h1 className="text-xl font-semibold tracking-tight">任务中心</h1>
							<span className="text-muted-foreground font-mono text-xs">
								OctoRill
							</span>
						</div>
						<p className="text-muted-foreground mt-1 text-sm">
							Logged in as{" "}
							<span className="text-foreground font-medium">
								{me.user.login}
							</span>
							{" · Admin"}
						</p>
					</div>

					<div className="flex flex-wrap gap-2">
						<Button asChild variant="outline">
							<a href="/admin">用户管理</a>
						</Button>
						<Button asChild variant="secondary">
							<a href="/">返回仪表盘</a>
						</Button>
						<Button asChild variant="ghost">
							<a href="/auth/logout">Logout</a>
						</Button>
					</div>
				</div>
			}
			footer={<AppMetaFooter />}
		>
			<JobManagement currentUserId={me.user.id} />
		</AppShell>
	);
}
