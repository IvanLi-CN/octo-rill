import { UserManagement } from "@/admin/UserManagement";
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

export function AdminPanel(props: { me: MeResponse }) {
	const { me } = props;

	return (
		<AppShell
			header={
				<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="min-w-0">
						<div className="flex flex-wrap items-center gap-2">
							<h1 className="text-xl font-semibold tracking-tight">
								管理员面板
							</h1>
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
			<div className="space-y-4">
				<p className="text-muted-foreground text-sm">
					这是独立的管理员界面，当前仅包含用户管理模块。
				</p>
				<UserManagement currentUserId={me.user.id} />
			</div>
		</AppShell>
	);
}
