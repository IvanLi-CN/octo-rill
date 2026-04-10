import { LogOut, RefreshCcw, ShieldCheck, Sparkles } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { BrandLogo } from "@/components/brand/BrandLogo";
import { Button } from "@/components/ui/button";

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
}) {
	const { login, name, avatarUrl, email, isAdmin, aiDisabledHint, logoutHref } =
		props;
	const displayName = name?.trim() || login;
	const secondaryName =
		name?.trim() && name.trim() !== login ? `@${login}` : null;

	return (
		<div
			className="absolute top-full right-0 z-50 mt-2 w-64 rounded-3xl border bg-card/98 p-4 shadow-lg ring-1 ring-black/5 backdrop-blur"
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
}) {
	const { login, name, avatarUrl, email, isAdmin, aiDisabledHint, logoutHref } =
		props;
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
				className="inline-flex size-9 items-center justify-center overflow-hidden rounded-full border border-border/70 bg-card shadow-sm transition hover:border-foreground/20 hover:shadow"
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
	return (
		<div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
			<div
				className="flex min-w-0 flex-1 items-start gap-3"
				data-dashboard-brand-block
			>
				<BrandLogo
					variant="mark"
					alt=""
					className="shrink-0"
					imgClassName="size-10 sm:size-11 lg:size-12"
				/>

				<div className="min-w-0 space-y-1">
					<h1
						className="min-w-0 text-2xl leading-[0.95] font-semibold tracking-tight text-[#495675] sm:text-[1.75rem] dark:text-[#dbe7ff]"
						data-dashboard-brand-heading
					>
						OctoRill
					</h1>

					<p className="text-muted-foreground text-sm font-medium leading-snug">
						GitHub 信息流 · AI 中文翻译 · Inbox 工作台
					</p>
				</div>
			</div>

			<div
				className="flex flex-wrap items-center gap-2 self-start lg:justify-end"
				data-dashboard-primary-actions
			>
				<Button disabled={busy} onClick={onSyncAll}>
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
				/>
			</div>
		</div>
	);
}
