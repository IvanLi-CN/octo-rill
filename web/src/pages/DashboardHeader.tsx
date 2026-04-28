import type * as React from "react";
import {
	ArrowUpRight,
	LogOut,
	RefreshCcw,
	Settings,
	ShieldCheck,
	Sparkles,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { BrandLogo } from "@/components/brand/BrandLogo";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { Button } from "@/components/ui/button";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";
import { clearAllWarmStartupCaches } from "@/auth/startupCache";
import { useAppShellChrome } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";

export type DashboardSyncProgress = {
	currentStep: number;
	totalSteps: number;
	stageLabel: string;
	detail: string;
};

export type DashboardHeaderProps = {
	login: string;
	name?: string | null;
	avatarUrl?: string | null;
	email?: string | null;
	isAdmin: boolean;
	aiDisabledHint?: boolean;
	busy?: boolean;
	syncingAll?: boolean;
	syncProgress?: DashboardSyncProgress | null;
	onSyncAll?: () => void;
	logoutHref?: string;
	mobileControlBand?: React.ReactNode;
};

function clampUnit(value: number) {
	return Math.max(0, Math.min(1, value));
}

function mix(from: number, to: number, progress: number) {
	return from + (to - from) * clampUnit(progress);
}

function resolveUserInitials(login: string, name?: string | null) {
	const source = (name?.trim() || login.trim()).replace(/^@+/, "");
	const parts = source.split(/\s+/).filter(Boolean);
	if (parts.length >= 2) {
		return `${parts[0][0] ?? ""}${parts[1][0] ?? ""}`.toUpperCase();
	}
	return source.slice(0, 2).toUpperCase();
}

function clearStartupCacheBeforeLogout() {
	clearAllWarmStartupCaches();
}

function DashboardSyncTooltipContent(props: {
	progress: DashboardSyncProgress | null | undefined;
}) {
	const { progress } = props;
	const currentStep = progress?.currentStep ?? 0;
	const totalSteps = progress?.totalSteps ?? 4;
	const stageLabel = progress?.stageLabel ?? "等待后台任务开始";
	const detail = progress?.detail ?? "正在连接任务事件流";
	const progressValue =
		totalSteps > 0
			? Math.max(0, Math.min(100, (currentStep / totalSteps) * 100))
			: 0;

	return (
		<div className="w-64 space-y-2 py-0.5">
			<div className="space-y-1">
				<p className="text-xs font-semibold">正在后台同步你的 GitHub 数据</p>
				<p className="text-[11px] leading-snug opacity-80">{stageLabel}</p>
			</div>
			<div className="space-y-1.5">
				<div className="flex items-center justify-between gap-3 text-[11px] font-medium">
					<span>阶段进度</span>
					<span className="font-mono">
						{currentStep}/{totalSteps}
					</span>
				</div>
				<div className="h-1.5 overflow-hidden rounded-full bg-background/20">
					<div
						className="h-full rounded-full bg-background"
						style={{ width: `${progressValue}%` }}
					/>
				</div>
			</div>
			<p className="text-[11px] leading-snug opacity-80">{detail}</p>
		</div>
	);
}

function DashboardUserAvatar(props: {
	login: string;
	name?: string | null;
	avatarUrl?: string | null;
	className: string;
}) {
	const { login, name, avatarUrl, className } = props;
	const initials = useMemo(
		() => resolveUserInitials(login, name),
		[login, name],
	);

	if (avatarUrl) {
		return (
			<img
				src={avatarUrl}
				alt=""
				loading="lazy"
				decoding="async"
				referrerPolicy="no-referrer"
				className={`${className} object-cover`}
			/>
		);
	}

	return (
		<span
			className={`${className} bg-[#495675] text-xs font-semibold tracking-[0.12em] text-white`}
		>
			{initials}
		</span>
	);
}

function DashboardUserInfoCard(props: {
	login: string;
	name?: string | null;
	avatarUrl?: string | null;
	email?: string | null;
	isAdmin: boolean;
	aiDisabledHint: boolean;
	logoutHref: string;
	showMobileAdminLink: boolean;
}) {
	const {
		login,
		name,
		avatarUrl,
		email,
		isAdmin,
		aiDisabledHint,
		logoutHref,
		showMobileAdminLink,
	} = props;
	const displayName = name?.trim() || login;
	const secondaryName =
		name?.trim() && name.trim() !== login ? `@${login}` : null;

	return (
		<div
			className="absolute top-full right-0 z-50 mt-2 w-[min(18rem,calc(100vw-2rem))] rounded-[1.6rem] border bg-card/98 p-4 shadow-lg ring-1 ring-black/5 backdrop-blur dark:ring-white/10 sm:w-64"
			data-dashboard-user-card
			role="dialog"
			aria-label="账号信息"
		>
			<div className="flex items-center gap-3">
				<DashboardUserAvatar
					login={login}
					name={name}
					avatarUrl={avatarUrl}
					className="flex size-12 shrink-0 items-center justify-center rounded-full border border-border/70"
				/>
				<div className="min-w-0 space-y-1">
					<div className="flex min-w-0 items-center gap-1.5">
						<p className="truncate text-sm font-semibold text-foreground">
							{displayName}
						</p>
						{isAdmin ? (
							<span
								className="inline-flex shrink-0 items-center justify-center rounded-full bg-[#495675]/10 p-1 text-[#495675] dark:bg-[#dbe7ff]/12 dark:text-[#dbe7ff]"
								aria-label="管理员"
								data-dashboard-user-admin-indicator
								role="img"
								title="管理员"
							>
								<ShieldCheck className="size-3.5" />
							</span>
						) : null}
					</div>
					{secondaryName || aiDisabledHint ? (
						<div className="flex min-w-0 flex-wrap items-center gap-x-1.5 gap-y-1 text-xs text-muted-foreground">
							{secondaryName ? (
								<p className="truncate">{secondaryName}</p>
							) : null}
							{aiDisabledHint ? (
								<span className="inline-flex items-center gap-1 whitespace-nowrap">
									{secondaryName ? <span aria-hidden="true">·</span> : null}
									<Sparkles className="size-3" />
									AI 未配置
								</span>
							) : null}
						</div>
					) : null}
				</div>
			</div>

			{email ? (
				<p className="mt-3 text-xs leading-relaxed text-muted-foreground">
					{email}
				</p>
			) : null}

			{showMobileAdminLink ? (
				<div className="mt-4 border-t border-border/70 pt-3 sm:hidden">
					<Button asChild variant="ghost" className="w-full justify-start px-2">
						<InternalLink
							href="/admin"
							to="/admin"
							data-dashboard-mobile-admin-entry="true"
						>
							<ArrowUpRight className="size-4" />
							管理员面板
						</InternalLink>
					</Button>
				</div>
			) : null}

			<div className="mt-4 border-t border-border/70 pt-3">
				<Button asChild variant="ghost" className="w-full justify-start px-2">
					<InternalLink
						href="/settings"
						to="/settings"
						data-dashboard-settings-entry="true"
					>
						<Settings className="size-4" />
						设置
					</InternalLink>
				</Button>
			</div>

			<div className="mt-2 border-t border-border/70 pt-3">
				<Button asChild variant="ghost" className="w-full justify-start px-2">
					<a
						aria-label="退出登录"
						href={logoutHref}
						onClick={clearStartupCacheBeforeLogout}
					>
						<LogOut className="size-4" />
						退出登录
					</a>
				</Button>
			</div>
		</div>
	);
}

function DashboardUserMenu(props: {
	login: string;
	name?: string | null;
	avatarUrl?: string | null;
	email?: string | null;
	isAdmin: boolean;
	aiDisabledHint: boolean;
	logoutHref: string;
	headerProgress: number;
	showMobileAdminLink: boolean;
}) {
	const {
		login,
		name,
		avatarUrl,
		email,
		isAdmin,
		aiDisabledHint,
		logoutHref,
		headerProgress,
		showMobileAdminLink,
	} = props;
	const cardId = useId();
	const wrapperRef = useRef<HTMLFieldSetElement | null>(null);
	const [hoverOpen, setHoverOpen] = useState(false);
	const [pinnedOpen, setPinnedOpen] = useState(false);
	const open = hoverOpen || pinnedOpen;

	useEffect(() => {
		if (!open) return;

		const handlePointerDown = (event: PointerEvent) => {
			const target = event.target;
			if (!(target instanceof Node)) return;
			if (!wrapperRef.current?.contains(target)) {
				setHoverOpen(false);
				setPinnedOpen(false);
			}
		};

		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setHoverOpen(false);
				setPinnedOpen(false);
			}
		};

		window.addEventListener("pointerdown", handlePointerDown);
		window.addEventListener("keydown", handleKeyDown);
		return () => {
			window.removeEventListener("pointerdown", handlePointerDown);
			window.removeEventListener("keydown", handleKeyDown);
		};
	}, [open]);

	return (
		<fieldset
			ref={wrapperRef}
			className="relative inline-flex min-w-0 items-center leading-none"
			data-app-shell-gesture-guard
			data-dashboard-user-menu
			aria-label="账号菜单"
			onMouseEnter={() => setHoverOpen(true)}
			onMouseLeave={() => setHoverOpen(false)}
		>
			<button
				type="button"
				className={cn(
					"inline-flex items-center justify-center overflow-hidden rounded-full border border-border/70 bg-card shadow-sm transition hover:border-foreground/20 hover:shadow",
				)}
				style={{
					width: `${mix(36, 32, headerProgress)}px`,
					height: `${mix(36, 32, headerProgress)}px`,
				}}
				aria-label="查看账号信息"
				aria-controls={cardId}
				aria-expanded={open}
				onClick={() => setPinnedOpen((current) => !current)}
				onFocus={() => setHoverOpen(true)}
				onBlur={(event) => {
					const nextTarget = event.relatedTarget;
					if (
						!(nextTarget instanceof Node) ||
						!event.currentTarget.parentElement?.contains(nextTarget)
					) {
						setHoverOpen(false);
					}
				}}
			>
				<DashboardUserAvatar
					login={login}
					name={name}
					avatarUrl={avatarUrl}
					className="flex size-full items-center justify-center"
				/>
			</button>

			{open ? (
				<div id={cardId}>
					<DashboardUserInfoCard
						login={login}
						name={name}
						avatarUrl={avatarUrl}
						email={email}
						isAdmin={isAdmin}
						aiDisabledHint={aiDisabledHint}
						logoutHref={logoutHref}
						showMobileAdminLink={showMobileAdminLink}
					/>
				</div>
			) : null}
		</fieldset>
	);
}

export function DashboardHeader({
	login,
	name = null,
	avatarUrl = null,
	email = null,
	isAdmin,
	aiDisabledHint = false,
	busy = false,
	syncingAll = false,
	syncProgress = null,
	onSyncAll,
	logoutHref = "/auth/logout",
	mobileControlBand = null,
}: DashboardHeaderProps) {
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
	const interactiveMotionClass = disableHeaderMotion
		? "transition-none"
		: "motion-safe:transition-[gap,transform] motion-safe:duration-200 motion-safe:ease-out";
	const actionScale = mix(1, 0.9, mobileHeaderProgress);
	const hideSubtitle = mobileChromeEnabled && isMobileViewport;
	const useSingleLineHeader = hideSubtitle;
	const shouldRenderMobileControlBand = Boolean(
		mobileControlBand && mobileChromeEnabled && isMobileViewport,
	);

	return (
		<div
			className={cn(
				"flex flex-col gap-3",
				!disableHeaderMotion &&
					"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
			)}
			style={
				hideSubtitle
					? {
							gap: `${mix(12, 8, mobileHeaderProgress)}px`,
						}
					: undefined
			}
			data-dashboard-header-compact={useMobileCompact ? "true" : "false"}
			data-dashboard-header-progress={mobileHeaderProgress.toFixed(3)}
			data-dashboard-header-interacting={headerInteracting ? "true" : "false"}
		>
			<div
				className={cn(
					"flex flex-col gap-3 sm:grid sm:grid-cols-[minmax(0,1fr)_auto] sm:items-start sm:gap-x-4 sm:gap-y-3 lg:flex lg:flex-row lg:items-start lg:justify-between",
					interactiveMotionClass,
					useSingleLineHeader && "flex-row items-center justify-between gap-3",
				)}
				style={
					useSingleLineHeader
						? {
								gap: `${mix(12, 8, mobileHeaderProgress)}px`,
							}
						: undefined
				}
				data-dashboard-header-main-row
			>
				<div
					className={cn(
						"flex min-w-0 flex-1 items-start gap-3",
						disableHeaderMotion
							? "transition-none"
							: "motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
						useSingleLineHeader && "items-center gap-2.5",
						hideSubtitle && "items-center",
					)}
					style={
						hideSubtitle
							? {
									gap: `${mix(10, 8, mobileHeaderProgress)}px`,
								}
							: undefined
					}
					data-dashboard-brand-block
				>
					<div
						className="shrink-0"
						style={
							hideSubtitle
								? {
										width: `${mix(34, 32, mobileHeaderProgress)}px`,
										height: `${mix(34, 32, mobileHeaderProgress)}px`,
									}
								: undefined
						}
					>
						<BrandLogo
							variant="mark"
							alt=""
							className="size-full"
							imgClassName={cn(
								"size-10 sm:size-11 lg:size-12",
								!disableHeaderMotion &&
									"motion-safe:transition-[width,height,transform] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
								hideSubtitle && "size-full",
							)}
						/>
					</div>

					<div
						className={cn(
							"min-w-0",
							!disableHeaderMotion &&
								"motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
							disableHeaderMotion && "transition-none",
							hideSubtitle && "space-y-0",
						)}
					>
						<h1
							className={cn(
								"min-w-0 text-2xl leading-[0.95] font-semibold tracking-tight text-[#495675] sm:text-[1.75rem] dark:text-[#dbe7ff]",
								disableHeaderMotion
									? "transition-none"
									: "motion-safe:transition-[font-size,line-height,letter-spacing,transform] motion-safe:duration-200 motion-safe:ease-out",
								hideSubtitle && "text-[1.75rem] sm:text-[1.75rem]",
							)}
							style={
								hideSubtitle
									? {
											fontSize: `${mix(28, 23.2, mobileHeaderProgress)}px`,
											lineHeight: mix(0.95, 0.92, mobileHeaderProgress),
										}
									: undefined
							}
							data-dashboard-brand-heading
						>
							OctoRill
						</h1>

						<p
							className={cn(
								"text-muted-foreground text-sm font-medium leading-snug",
								!disableHeaderMotion &&
									"motion-safe:transition-[opacity,transform] motion-safe:duration-150 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
								hideSubtitle && "hidden sm:block",
								useMobileCompact && "hidden",
							)}
							data-dashboard-brand-subtitle
						>
							GitHub 动态 · 中文翻译 · 日报与 Inbox
						</p>
					</div>
				</div>

				<div
					className={cn(
						"flex items-center gap-2 self-start lg:justify-end",
						disableHeaderMotion
							? "transition-none"
							: "motion-safe:transition-[gap] motion-safe:duration-200 motion-safe:ease-out",
						!useSingleLineHeader && "sm:justify-self-end sm:self-start",
						useSingleLineHeader &&
							"w-auto shrink-0 justify-end gap-1.5 self-auto",
					)}
					style={
						useSingleLineHeader
							? {
									gap: `${mix(6, 4, mobileHeaderProgress)}px`,
								}
							: undefined
					}
					data-dashboard-primary-actions
				>
					<div
						data-app-shell-gesture-guard
						style={
							hideSubtitle
								? {
										transform: `scale(${actionScale})`,
										transformOrigin: "center",
									}
								: undefined
						}
					>
						<ThemeToggle
							compact={useSingleLineHeader}
							className={cn(
								!disableHeaderMotion &&
									"motion-safe:transition-[width,height,padding,transform] motion-safe:duration-200 motion-safe:ease-out",
								disableHeaderMotion && "transition-none",
								useMobileCompact && "size-8",
							)}
						/>
					</div>
					<Tooltip>
						<TooltipTrigger asChild>
							<Button
								disabled={busy && !syncingAll}
								onClick={onSyncAll}
								size={hideSubtitle ? "sm" : "default"}
								data-app-shell-gesture-guard
								className={cn(
									!disableHeaderMotion &&
										"motion-safe:transition-[height,padding,border-radius,transform] motion-safe:duration-200 motion-safe:ease-out",
									disableHeaderMotion && "transition-none",
									hideSubtitle && "h-9 rounded-full px-3.5 text-sm",
									useMobileCompact && "h-8 px-3",
								)}
								style={
									hideSubtitle
										? {
												height: `${mix(36, 32, mobileHeaderProgress)}px`,
												paddingInline: `${mix(14, 12, mobileHeaderProgress)}px`,
											}
										: undefined
								}
							>
								<RefreshCcw
									className={syncingAll ? "size-4 animate-spin" : "size-4"}
								/>
								同步
							</Button>
						</TooltipTrigger>
						{syncingAll ? (
							<TooltipContent side="bottom" align="center" sideOffset={8}>
								<DashboardSyncTooltipContent progress={syncProgress} />
							</TooltipContent>
						) : null}
					</Tooltip>
					<DashboardUserMenu
						login={login}
						name={name}
						avatarUrl={avatarUrl}
						email={email}
						isAdmin={isAdmin}
						aiDisabledHint={aiDisabledHint}
						logoutHref={logoutHref}
						headerProgress={mobileHeaderProgress}
						showMobileAdminLink={isAdmin}
					/>
				</div>
			</div>

			{shouldRenderMobileControlBand ? (
				<div
					className={cn(
						"sm:hidden",
						!disableHeaderMotion &&
							"motion-safe:transition-[max-height,opacity,transform,margin] motion-safe:duration-200 motion-safe:ease-out",
						disableHeaderMotion && "transition-none",
						useMobileCompact
							? "pointer-events-none overflow-hidden max-h-0 -translate-y-1 opacity-0"
							: "overflow-visible max-h-32 translate-y-0 opacity-100",
					)}
					style={{
						maxHeight: `${mix(116, 0, mobileHeaderProgress)}px`,
						opacity: mix(1, 0, mobileHeaderProgress),
						transform: `translateY(${mix(0, -6, mobileHeaderProgress)}px)`,
					}}
					data-dashboard-mobile-top-shell={
						useMobileCompact ? undefined : "expanded"
					}
					data-dashboard-mobile-top-shell-section="workband"
					aria-hidden={useMobileCompact}
				>
					{mobileControlBand}
				</div>
			) : null}
		</div>
	);
}
