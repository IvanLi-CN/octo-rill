import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type AdminHeaderUser = {
	login: string;
};

type AdminNavItem = {
	key: "users" | "jobs";
	label: string;
	href: string;
};

type AdminHeaderProps = {
	user: AdminHeaderUser;
	activeNav: AdminNavItem["key"];
};

const ADMIN_NAV_ITEMS: AdminNavItem[] = [
	{ key: "users", label: "用户管理", href: "/admin" },
	{ key: "jobs", label: "任务中心", href: "/admin/jobs" },
];

export function AdminHeader({ user, activeNav }: AdminHeaderProps) {
	return (
		<div className="flex flex-col gap-4">
			<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
				<div className="min-w-0">
					<div className="flex flex-wrap items-center gap-2">
						<h1 className="text-xl font-semibold tracking-tight">管理后台</h1>
						<span className="text-muted-foreground font-mono text-xs">
							OctoRill
						</span>
					</div>
					<p className="text-muted-foreground mt-1 text-sm">
						Logged in as{" "}
						<span className="text-foreground font-medium">{user.login}</span>
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

			<nav aria-label="管理员导航">
				<div className="bg-card/60 inline-flex items-center gap-1 rounded-lg border p-1">
					{ADMIN_NAV_ITEMS.map((item) => {
						const isActive = activeNav === item.key;
						return (
							<Button
								key={item.key}
								asChild
								size="sm"
								variant={isActive ? "default" : "ghost"}
								className={cn(isActive ? "shadow-xs" : null)}
							>
								<a
									href={item.href}
									aria-current={isActive ? "page" : undefined}
								>
									{item.label}
								</a>
							</Button>
						);
					})}
				</div>
			</nav>
		</div>
	);
}
