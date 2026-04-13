import { BrandLogo } from "@/components/brand/BrandLogo";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { Button } from "@/components/ui/button";
import { useAppShellChrome } from "@/layout/AppShell";
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
	const { compactHeader, isMobileViewport, mobileChromeEnabled } =
		useAppShellChrome();
	const useMobileCompact =
		mobileChromeEnabled && isMobileViewport && compactHeader;

	return (
		<div
			className={cn(
				"flex flex-col gap-2.5 motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
				useMobileCompact && "gap-2",
			)}
			data-admin-header-compact={useMobileCompact ? "true" : "false"}
		>
			<div
				className={cn(
					"flex flex-col gap-2 motion-safe:transition-[gap,transform] motion-safe:duration-200 motion-safe:ease-out lg:flex-row lg:items-center lg:justify-between",
					useMobileCompact && "gap-1.5",
				)}
			>
				<div
					className={cn(
						"flex min-w-0 flex-col gap-2 motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out lg:flex-row lg:items-center lg:gap-6",
						useMobileCompact && "gap-1.5",
					)}
				>
					<div className="flex min-w-0 flex-wrap items-center gap-2">
						<h1
							className={cn(
								"text-lg font-semibold tracking-tight motion-safe:transition-[font-size,transform] motion-safe:duration-200 motion-safe:ease-out",
								useMobileCompact && "text-base",
							)}
						>
							管理后台
						</h1>
						<BrandLogo
							variant="wordmark"
							className={cn(
								"h-5 motion-safe:transition-[height,transform] motion-safe:duration-200 motion-safe:ease-out",
								useMobileCompact && "h-[18px]",
							)}
						/>
					</div>

					<nav
						aria-label="管理员导航"
						className="-mx-1 overflow-x-auto px-1 no-scrollbar"
					>
						<div
							className={cn(
								"flex h-8 min-w-max items-center gap-4 pr-1 whitespace-nowrap motion-safe:transition-[gap,height] motion-safe:duration-200 motion-safe:ease-out",
								useMobileCompact && "h-7 gap-3",
							)}
						>
							{ADMIN_NAV_ITEMS.map((item) => {
								const isActive = activeNav === item.key;
								return (
									<a
										key={item.key}
										href={item.href}
										aria-current={isActive ? "page" : undefined}
										className={cn(
											"text-muted-foreground relative inline-flex h-8 items-center text-sm transition-colors hover:text-foreground",
											useMobileCompact && "h-7 text-[13px]",
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

				<div
					className={cn(
						"flex items-center gap-2 self-start motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out lg:self-auto",
						useMobileCompact && "gap-1.5",
					)}
				>
					<ThemeToggle
						className={cn(
							"motion-safe:transition-[padding,transform] motion-safe:duration-200 motion-safe:ease-out",
							useMobileCompact && "p-0.5",
						)}
					/>
					<Button
						asChild
						variant="outline"
						size="sm"
						className={cn(
							"mr-2 h-8 px-2 motion-safe:transition-[height,padding,margin,transform] motion-safe:duration-200 motion-safe:ease-out",
							useMobileCompact && "mr-1 h-7 px-2",
						)}
					>
						<a href="/" aria-label="返回前台首页" title="返回前台首页">
							<Home className="size-4" />
							<span>返回前台</span>
						</a>
					</Button>
					<div
						className={cn(
							"flex items-center gap-1",
							useMobileCompact && "gap-0.5",
						)}
					>
						<span
							className={cn(
								"text-muted-foreground text-sm motion-safe:transition-[font-size,transform] motion-safe:duration-200 motion-safe:ease-out",
								useMobileCompact && "text-xs",
							)}
						>
							{user.login}
						</span>
						<Button
							asChild
							variant="ghost"
							size="icon"
							className={cn(
								"size-8 motion-safe:transition-[width,height,transform] motion-safe:duration-200 motion-safe:ease-out",
								useMobileCompact && "size-7",
							)}
						>
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
