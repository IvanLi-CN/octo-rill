import {
	ArrowUpRight,
	LogOut,
	RefreshCcw,
	ShieldCheck,
	Sparkles,
} from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { BrandLogo } from "@/components/brand/BrandLogo";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import { Button } from "@/components/ui/button";
import { useAppShellChrome } from "@/layout/AppShell";
import { cn } from "@/lib/utils";

export type DashboardHeaderProps = {
	login: string;
	name?: string | null;
	avatarUrl?: string | null;
	email?: string | null;
	isAdmin: boolean;
	aiDisabledHint?: boolean;
	busy?: boolean;
	syncingAll?: boolean;
	onSyncAll?: () => void;
	logoutHref?: string;
};

function resolveUserInitials(login: string, name?: string | null) {
	const source = (name?.trim() || login.trim()).replace(/^@+/, "");
	const parts = source.split(/\s+/).filter(Boolean);
	if (parts.length >= 2) {
		return `${parts[0][0] ?? ""}${parts[1][0] ?? ""}`.toUpperCase();
	}
	return source.slice(0, 2).toUpperCase();
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
						<a href="/admin" data-dashboard-mobile-admin-entry="true">
							<ArrowUpRight className="size-4" />
							管理员面板
						</a>
					</Button>
				</div>
			) : null}

			<div className="mt-4 border-t border-border/70 pt-3">
				<Button asChild variant="ghost" className="w-full justify-start px-2">
					<a aria-label="退出登录" href={logoutHref}>
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
	compactHeader: boolean;
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
		compactHeader,
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
			className="relative m-0 min-w-0 border-0 p-0"
			aria-label="账号菜单"
			onMouseEnter={() => setHoverOpen(true)}
			onMouseLeave={() => setHoverOpen(false)}
		>
			<button
				type="button"
				className={cn(
					"inline-flex items-center justify-center overflow-hidden rounded-full border border-border/70 bg-card shadow-sm transition hover:border-foreground/20 hover:shadow",
					compactHeader ? "size-8" : "size-9",
				)}
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
	onSyncAll,
	logoutHref = "/auth/logout",
}: DashboardHeaderProps) {
	const { compactHeader, isMobileViewport, mobileChromeEnabled } =
		useAppShellChrome();
	const useMobileCompact =
		mobileChromeEnabled && isMobileViewport && compactHeader;
	const hideSubtitle = mobileChromeEnabled && isMobileViewport;

	return (
		<div
			className={cn(
				"flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between",
				useMobileCompact && "flex-row items-center justify-between gap-2",
			)}
			data-dashboard-header-compact={useMobileCompact ? "true" : "false"}
		>
			<div
				className={cn(
					"flex min-w-0 flex-1 items-start gap-3",
					useMobileCompact && "items-center gap-2",
				)}
				data-dashboard-brand-block
			>
				<BrandLogo
					variant="mark"
					alt=""
					className="shrink-0"
					imgClassName={cn(
						"size-10 sm:size-11 lg:size-12",
						hideSubtitle && "size-9 sm:size-11 lg:size-12",
						useMobileCompact && "size-8 sm:size-10 lg:size-12",
					)}
				/>

				<div
					className={cn(
						"min-w-0",
						useMobileCompact ? "space-y-0" : "space-y-1",
					)}
				>
					<h1
						className={cn(
							"min-w-0 text-2xl leading-[0.95] font-semibold tracking-tight text-[#495675] sm:text-[1.75rem] dark:text-[#dbe7ff]",
							hideSubtitle && "text-[1.9rem] sm:text-[1.75rem]",
							useMobileCompact && "text-[1.45rem] sm:text-[1.65rem]",
						)}
						data-dashboard-brand-heading
					>
						OctoRill
					</h1>

<<<<<<< HEAD
					<p
						className={cn(
							"text-muted-foreground text-sm font-medium leading-snug",
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
					hideSubtitle && "w-full justify-between",
					useMobileCompact && "w-auto justify-end gap-1.5 self-auto",
				)}
				data-dashboard-primary-actions
			>
				<ThemeToggle className={cn(useMobileCompact && "p-0.5")} />
				<Button
					disabled={busy}
					onClick={onSyncAll}
					size={hideSubtitle ? "sm" : "default"}
					className={cn(hideSubtitle && "h-8 px-3")}
				>
					<RefreshCcw
						className={syncingAll ? "size-4 animate-spin" : "size-4"}
					/>
					同步
				</Button>
				<DashboardUserMenu
					login={login}
					name={name}
					avatarUrl={avatarUrl}
					email={email}
					isAdmin={isAdmin}
					aiDisabledHint={aiDisabledHint}
					logoutHref={logoutHref}
					compactHeader={useMobileCompact}
					showMobileAdminLink={isAdmin}
				/>
			</div>
		</div>
	);
}
