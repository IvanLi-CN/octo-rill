import { clearAllWarmStartupCaches } from "@/auth/startupCache";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { Button } from "@/components/ui/button";
import { useAppShellChrome } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";
import { Home, LogOut } from "lucide-react";

function clampUnit(value: number) {
	return Math.max(0, Math.min(1, value));
}

function mix(from: number, to: number, progress: number) {
	return from + (to - from) * clampUnit(progress);
}

type AdminHeaderUser = {
	login: string;
};

type AdminNavItem = {
	key: "dashboard" | "users" | "jobs";
	label: string;
	href: string;
};

type AdminHeaderProps = {
	user: AdminHeaderUser;
	activeNav: AdminNavItem["key"];
};

const ADMIN_NAV_ITEMS: AdminNavItem[] = [
	{ key: "dashboard", label: "仪表盘", href: "/admin" },
	{ key: "users", label: "用户管理", href: "/admin/users" },
	{ key: "jobs", label: "任务中心", href: "/admin/jobs" },
];

function clearStartupCacheBeforeLogout() {
	clearAllWarmStartupCaches();
}

export function AdminHeader({ user, activeNav }: AdminHeaderProps) {
	const {
		compactHeader,
		headerInteracting,
		headerTransitionSuppressed,
		headerProgress,
		isMobileViewport,
		mobileChromeEnabled,
	} = useAppShellChrome();
	const useMobileCompact =
		mobileChromeEnabled && isMobileViewport && compactHeader;
	const mobileHeaderProgress =
		mobileChromeEnabled && isMobileViewport
			? clampUnit(headerProgress)
			: useMobileCompact
				? 1
				: 0;
	const disableHeaderMotion = headerInteracting || headerTransitionSuppressed;

	return (
		<div
			className={cn(
				"flex flex-col gap-2.5",
				!disableHeaderMotion &&
					"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
			)}
			style={
				isMobileViewport
					? {
							gap: `${mix(10, 8, mobileHeaderProgress)}px`,
						}
					: undefined
			}
			data-admin-header-compact={useMobileCompact ? "true" : "false"}
		>
			<h1 className="sr-only">管理后台</h1>
			<div
				className={cn(
					"flex flex-col gap-2 lg:flex-row lg:items-center lg:justify-between",
					!disableHeaderMotion &&
						"motion-safe:transition-[gap,transform] motion-safe:duration-200 motion-safe:ease-out",
				)}
				style={
					isMobileViewport
						? {
								gap: `${mix(8, 6, mobileHeaderProgress)}px`,
							}
						: undefined
				}
			>
				<div
					className={cn(
						"flex min-w-0 flex-col gap-2 lg:flex-row lg:items-center lg:gap-4",
						!disableHeaderMotion &&
							"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
					)}
					style={
						isMobileViewport
							? {
									gap: `${mix(8, 6, mobileHeaderProgress)}px`,
								}
							: undefined
					}
				>
					<div
						className={cn(
							"flex min-w-0 items-center gap-3 lg:gap-4",
							!disableHeaderMotion &&
								"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
							disableHeaderMotion && "transition-none",
						)}
						style={
							isMobileViewport
								? {
										gap: `${mix(12, 8, mobileHeaderProgress)}px`,
									}
								: undefined
						}
					>
						<InternalLink
							href="/admin"
							to="/admin"
							aria-label="OctoRill 管理后台"
							className="shrink-0"
						>
							<div
								style={
									isMobileViewport
										? {
												height: `${mix(20, 18, mobileHeaderProgress)}px`,
											}
										: undefined
								}
							>
								<BrandLogo
									variant="wordmark"
									className={cn(
										"h-5",
										!disableHeaderMotion &&
											"motion-safe:transition-[height,transform] motion-safe:duration-200 motion-safe:ease-out",
										disableHeaderMotion && "transition-none",
										isMobileViewport && "h-full",
										useMobileCompact && "h-[18px]",
									)}
									alt="OctoRill"
								/>
							</div>
						</InternalLink>
						<div
							className={cn(
								"hidden h-5 w-px shrink-0 rounded-full bg-border/80 lg:block",
								useMobileCompact && "hidden",
							)}
						/>
					</div>

					<nav
						aria-label="管理员导航"
						className="-mx-1 overflow-x-auto px-1 no-scrollbar"
					>
						<div
							className={cn(
								"flex h-8 min-w-max items-center gap-4 pr-1 whitespace-nowrap",
								!disableHeaderMotion &&
									"motion-safe:transition-[gap,height] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
							)}
							style={
								isMobileViewport
									? {
											height: `${mix(32, 28, mobileHeaderProgress)}px`,
											gap: `${mix(16, 12, mobileHeaderProgress)}px`,
										}
									: undefined
							}
						>
							{ADMIN_NAV_ITEMS.map((item) => {
								const isActive = activeNav === item.key;
								return (
									<InternalLink
										key={item.key}
										href={item.href}
										to={item.href}
										aria-current={isActive ? "page" : undefined}
										className={cn(
											"text-muted-foreground relative inline-flex h-8 items-center text-sm transition-colors hover:text-foreground",
											"after:absolute after:right-0 after:bottom-0 after:left-0 after:h-0.5 after:rounded-full after:bg-transparent",
											isActive
												? "text-foreground font-medium after:bg-foreground"
												: null,
										)}
										style={
											isMobileViewport
												? {
														height: `${mix(32, 28, mobileHeaderProgress)}px`,
														fontSize: `${mix(14, 13, mobileHeaderProgress)}px`,
													}
												: undefined
										}
									>
										{item.label}
									</InternalLink>
								);
							})}
						</div>
					</nav>
				</div>

				<div
					className={cn(
						"flex items-center gap-2 self-start lg:self-auto",
						!disableHeaderMotion &&
							"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
						disableHeaderMotion && "transition-none",
					)}
					style={
						isMobileViewport
							? {
									gap: `${mix(8, 6, mobileHeaderProgress)}px`,
								}
							: undefined
					}
				>
					<div
						style={
							isMobileViewport
								? {
										transform: `scale(${mix(1, 0.92, mobileHeaderProgress)})`,
										transformOrigin: "center",
									}
								: undefined
						}
					>
						<ThemeToggle
							className={cn(
								!disableHeaderMotion &&
									"motion-safe:transition-[padding,transform] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
								useMobileCompact && "p-0.5",
							)}
						/>
					</div>
					<Button
						asChild
						variant="outline"
						size="sm"
						className={cn(
							"mr-2 h-8 px-2",
							!disableHeaderMotion &&
								"motion-safe:transition-[height,padding,margin,transform] motion-safe:duration-200 motion-safe:ease-out",
							disableHeaderMotion && "transition-none",
							useMobileCompact && "mr-1 h-7 px-2",
						)}
						style={
							isMobileViewport
								? {
										height: `${mix(32, 28, mobileHeaderProgress)}px`,
									}
								: undefined
						}
					>
						<InternalLink
							href="/"
							to="/"
							aria-label="返回前台首页"
							title="返回前台首页"
						>
							<Home className="size-4" />
							<span>返回前台</span>
						</InternalLink>
					</Button>
					<div
						className={cn(
							"flex items-center gap-1",
							useMobileCompact && "gap-0.5",
						)}
						style={
							isMobileViewport
								? {
										gap: `${mix(4, 2, mobileHeaderProgress)}px`,
									}
								: undefined
						}
					>
						<span
							className={cn(
								"text-muted-foreground text-sm",
								!disableHeaderMotion &&
									"motion-safe:transition-[font-size,transform] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
							)}
							style={
								isMobileViewport
									? {
											fontSize: `${mix(14, 12, mobileHeaderProgress)}px`,
										}
									: undefined
							}
						>
							{user.login}
						</span>
						<Button
							asChild
							variant="ghost"
							size="icon"
							className={cn(
								"size-8",
								!disableHeaderMotion &&
									"motion-safe:transition-[width,height,transform] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
								useMobileCompact && "size-7",
							)}
							style={
								isMobileViewport
									? {
											width: `${mix(32, 28, mobileHeaderProgress)}px`,
											height: `${mix(32, 28, mobileHeaderProgress)}px`,
										}
									: undefined
							}
						>
							<a
								href="/auth/logout"
								aria-label="退出登录"
								title="退出登录"
								onClick={clearStartupCacheBeforeLogout}
							>
								<LogOut className="size-4" />
							</a>
						</Button>
					</div>
				</div>
			</div>
		</div>
	);
}
