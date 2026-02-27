import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { Home, LogOut } from "lucide-react";

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
		<div className="flex flex-col gap-2">
			<div className="flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between">
				<div className="flex min-w-0 flex-col gap-2 lg:flex-row lg:items-center lg:gap-6">
					<div className="flex flex-wrap items-center gap-2">
						<h1 className="text-lg font-semibold tracking-tight">管理后台</h1>
						<span className="text-muted-foreground font-mono text-xs">
							OctoRill
						</span>
					</div>

					<nav aria-label="管理员导航">
						<div className="flex h-8 items-center gap-4 pr-1">
							{ADMIN_NAV_ITEMS.map((item) => {
								const isActive = activeNav === item.key;
								return (
									<a
										key={item.key}
										href={item.href}
										aria-current={isActive ? "page" : undefined}
										className={cn(
											"text-muted-foreground relative inline-flex h-8 items-center text-sm transition-colors hover:text-foreground",
											"after:absolute after:right-0 after:bottom-0 after:left-0 after:h-0.5 after:rounded-full after:bg-transparent",
											isActive
												? "text-foreground font-medium after:bg-foreground"
												: null,
										)}
									>
										{item.label}
									</a>
								);
							})}
						</div>
					</nav>
				</div>

				<div className="flex items-center gap-2 self-start lg:self-auto">
					<Button asChild variant="outline" size="sm" className="mr-2 h-8 px-2">
						<a href="/" aria-label="返回前台首页" title="返回前台首页">
							<Home className="size-4" />
							<span>返回前台</span>
						</a>
					</Button>
					<div className="flex items-center gap-1">
						<span className="text-muted-foreground text-sm">{user.login}</span>
						<Button asChild variant="ghost" size="icon" className="size-8">
							<a href="/auth/logout" aria-label="退出登录" title="退出登录">
								<LogOut className="size-4" />
							</a>
						</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
