import type { MeResponse } from "@/api";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { VersionUpdateNotice } from "@/layout/VersionUpdateNotice";
import { AdminHeader } from "@/layout/AdminHeader";
import { cn } from "@/lib/utils";

const APP_BOOT_KEYFRAMES = `
	@keyframes app-boot-float {
		0%, 100% {
			transform: translateY(0);
		}
		50% {
			transform: translateY(-6px);
		}
	}
`;

function PulseBlock(props: { className?: string; rounded?: string }) {
	const { className, rounded = "rounded-2xl" } = props;
	return (
		<div className={cn("bg-muted/70 animate-pulse", rounded, className)} />
	);
}

export function AppBoot() {
	return (
		<div
			className="bg-background relative min-h-screen overflow-hidden"
			data-app-boot
		>
			<style>{APP_BOOT_KEYFRAMES}</style>
			<div className="bg-primary/16 absolute left-1/2 top-1/2 size-[22rem] -translate-x-1/2 -translate-y-1/2 rounded-full blur-3xl sm:size-[28rem]" />
			<div className="bg-chart-2/14 absolute left-1/2 top-1/2 size-[14rem] -translate-x-1/2 -translate-y-1/2 rounded-full blur-2xl sm:size-[18rem]" />
			<div className="relative flex min-h-screen items-center justify-center px-6 py-16">
				<div className="flex flex-col items-center gap-8 text-center">
					<div
						className="relative flex items-center justify-center"
						style={{ animation: "app-boot-float 6s ease-in-out infinite" }}
					>
						<div className="bg-primary/10 absolute size-24 rounded-full blur-2xl animate-[pulse_5.6s_ease-in-out_infinite] sm:size-48 sm:blur-3xl" />
						<div className="bg-chart-2/10 absolute size-16 rounded-full blur-xl animate-[pulse_7.2s_ease-in-out_infinite] sm:size-32 sm:blur-2xl" />
						<BrandLogo
							variant="mark"
							className="relative z-10 size-14 drop-shadow-[0_12px_30px_rgba(15,23,42,0.10)] sm:hidden"
						/>
						<BrandLogo
							variant="wordmark"
							className="relative z-10 hidden h-12 drop-shadow-[0_14px_36px_rgba(15,23,42,0.08)] sm:block"
						/>
					</div>

					<div className="space-y-3">
						<p className="text-muted-foreground max-w-sm text-sm leading-6 sm:text-base">
							应用正在完成初始化，请稍候片刻。
						</p>
					</div>

					<div className="flex items-center gap-3">
						<PulseBlock className="h-2.5 w-20 rounded-full" />
						<PulseBlock className="h-2.5 w-12 rounded-full" />
						<PulseBlock className="h-2.5 w-16 rounded-full" />
					</div>
				</div>
			</div>
		</div>
	);
}

const DASHBOARD_BOOT_TAB_PILLS = [
	{ key: "all", width: "w-12" },
	{ key: "releases", width: "w-16" },
	{ key: "stars", width: "w-14" },
	{ key: "followers", width: "w-16" },
	{ key: "briefs", width: "w-12" },
	{ key: "inbox", width: "w-14" },
] as const;

function DashboardBootHeader(_props: { me: MeResponse }) {
	return (
		<div className="flex flex-col gap-4 sm:gap-5" data-dashboard-boot-header>
			<div className="flex flex-row items-center justify-between gap-3 sm:flex-col sm:items-start sm:gap-3 lg:flex-row lg:items-start lg:justify-between">
				<div
					className="flex min-w-0 flex-1 items-center gap-2.5 sm:items-start sm:gap-3.5 lg:gap-4"
					data-dashboard-boot-brand-block
				>
					<BrandLogo
						variant="mark"
						alt=""
						className="size-8 shrink-0 sm:size-10 lg:size-12"
					/>
					<div className="min-w-0 space-y-0 pt-0.5 sm:space-y-1.5">
						<h1
							className="min-w-0 text-[1.75rem] leading-[0.95] font-semibold tracking-tight text-[#495675] sm:text-[1.75rem] dark:text-[#dbe7ff]"
							data-dashboard-boot-brand-heading
						>
							OctoRill
						</h1>
						<p
							className="text-muted-foreground hidden text-sm font-medium leading-snug sm:block"
							data-dashboard-boot-brand-subtitle
						>
							GitHub 动态 · 中文翻译 · 日报与 Inbox
						</p>
					</div>
				</div>
				<div
					className="flex shrink-0 items-center gap-1.5 self-auto sm:hidden"
					data-dashboard-boot-primary-actions-mobile
				>
					<PulseBlock className="size-8 rounded-full bg-card/85 border border-border/60" />
					<PulseBlock className="h-8 w-16 rounded-full bg-card/85 border border-border/60" />
					<PulseBlock className="size-8 rounded-full bg-card/85 border border-border/60" />
				</div>
				<div
					className="hidden self-start rounded-[1.35rem] border border-border/70 bg-card/82 p-1.5 shadow-sm sm:block"
					data-dashboard-boot-primary-actions
				>
					<div className="flex items-center gap-2">
						<PulseBlock className="size-10 rounded-full bg-muted/85" />
						<PulseBlock className="h-10 w-28 rounded-full bg-muted/85" />
						<PulseBlock className="size-10 rounded-full bg-muted/85" />
					</div>
				</div>
			</div>

			<div
				className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between"
				data-dashboard-boot-control-band
			>
				<div
					className="inline-flex max-w-full items-center gap-1 overflow-x-auto rounded-lg bg-muted/60 p-1 no-scrollbar"
					data-dashboard-boot-tab-strip
				>
					{DASHBOARD_BOOT_TAB_PILLS.map((pill) => (
						<PulseBlock
							key={pill.key}
							className={cn(
								"h-8 shrink-0 rounded-md border border-border/60 bg-card/85",
								pill.width,
							)}
						/>
					))}
				</div>

				<div
					className="hidden items-center gap-2 sm:flex"
					data-dashboard-boot-secondary-controls
				>
					<PulseBlock className="h-9 w-24 rounded-full border border-border/60 bg-card/85" />
					<PulseBlock className="h-9 w-20 rounded-full border border-border/60 bg-card/85" />
				</div>
			</div>
		</div>
	);
}

const DASHBOARD_FEED_SKELETON_KEYS = [
	"feed-1",
	"feed-2",
	"feed-3",
	"feed-4",
] as const;
const DASHBOARD_SIDEBAR_SKELETON_GROUPS = [
	["sidebar-1a", "sidebar-1b", "sidebar-1c", "sidebar-1d"],
	["sidebar-2a", "sidebar-2b", "sidebar-2c"],
	["sidebar-3a", "sidebar-3b", "sidebar-3c"],
] as const;
const ADMIN_PRIMARY_SKELETON_KEYS = [
	"primary-1",
	"primary-2",
	"primary-3",
	"primary-4",
	"primary-5",
] as const;
const ADMIN_ASIDE_SKELETON_KEYS = ["aside-1", "aside-2", "aside-3"] as const;
const ADMIN_DASHBOARD_HERO_SKELETON_KEYS = [
	"dashboard-hero-users",
	"dashboard-hero-active",
	"dashboard-hero-ongoing",
	"dashboard-hero-total",
] as const;
const ADMIN_DASHBOARD_LANE_SKELETON_KEYS = [
	"dashboard-lane-translate",
	"dashboard-lane-summary",
	"dashboard-lane-brief",
] as const;
const ADMIN_DASHBOARD_CHART_SKELETON_KEYS = [
	"dashboard-chart-today",
	"dashboard-chart-trend",
] as const;

function DashboardSkeletonGrid() {
	return (
		<div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_320px]">
			<section className="space-y-4">
				<div className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6">
					<div className="mb-4 flex items-center justify-between gap-3">
						<div className="space-y-2">
							<PulseBlock className="h-4 w-28 rounded-full" />
							<PulseBlock className="h-8 w-52 rounded-2xl" />
						</div>
						<PulseBlock className="h-10 w-28 rounded-2xl" />
					</div>
					<div className="space-y-3">
						{DASHBOARD_FEED_SKELETON_KEYS.map((key) => (
							<div
								key={key}
								className="rounded-2xl border border-border/60 bg-background/70 p-4"
							>
								<div className="flex items-start justify-between gap-3">
									<div className="space-y-2">
										<PulseBlock className="h-3.5 w-28 rounded-full" />
										<PulseBlock className="h-5 w-56 rounded-xl" />
									</div>
									<PulseBlock className="h-8 w-16 rounded-full" />
								</div>
								<div className="mt-4 space-y-2">
									<PulseBlock className="h-3 w-full rounded-full" />
									<PulseBlock className="h-3 w-11/12 rounded-full" />
									<PulseBlock className="h-3 w-8/12 rounded-full" />
								</div>
							</div>
						))}
					</div>
				</div>
			</section>

			<aside className="space-y-4">
				{DASHBOARD_SIDEBAR_SKELETON_GROUPS.map((keys, _index) => (
					<div
						key={keys[0]}
						className="rounded-[24px] border border-border/70 bg-card/75 p-4 shadow-sm sm:p-5"
					>
						<div className="space-y-2">
							<PulseBlock className="h-4 w-24 rounded-full" />
							<PulseBlock className="h-3 w-16 rounded-full" />
						</div>
						<div className="mt-4 space-y-3">
							{keys.map((key) => (
								<PulseBlock key={key} className="h-12 w-full rounded-2xl" />
							))}
						</div>
					</div>
				))}
			</aside>
		</div>
	);
}

export function DashboardStartupSkeleton(props: { me: MeResponse }) {
	const { me } = props;

	return (
		<AppShell
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="mx-auto max-w-6xl space-y-4">
				<DashboardBootHeader me={me} />
				<DashboardSkeletonGrid />
			</div>
		</AppShell>
	);
}

function AdminSkeletonGrid(props: { variant: "dashboard" | "users" | "jobs" }) {
	const { variant } = props;

	if (variant === "dashboard") {
		return (
			<div className="space-y-4">
				<div className="grid gap-4 xl:grid-cols-[1.55fr_1fr]">
					<div className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6">
						<div className="space-y-2">
							<PulseBlock className="h-4 w-28 rounded-full" />
							<PulseBlock className="h-7 w-72 rounded-2xl" />
						</div>
						<div className="mt-5 grid gap-3 md:grid-cols-2 xl:grid-cols-4">
							{ADMIN_DASHBOARD_HERO_SKELETON_KEYS.map((key) => (
								<div
									key={key}
									className="rounded-[24px] border border-border/70 bg-card/72 p-4"
								>
									<PulseBlock className="h-3 w-20 rounded-full" />
									<PulseBlock className="mt-4 h-8 w-20 rounded-2xl" />
									<PulseBlock className="mt-3 h-3 w-28 rounded-full" />
								</div>
							))}
						</div>
					</div>
					<div className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6">
						<PulseBlock className="h-4 w-24 rounded-full" />
						<div className="mt-5 grid gap-3 sm:grid-cols-3 xl:grid-cols-1">
							{ADMIN_DASHBOARD_LANE_SKELETON_KEYS.map((key) => (
								<div
									key={key}
									className="rounded-[24px] border border-border/70 bg-card/72 p-4"
								>
									<PulseBlock className="h-3 w-16 rounded-full" />
									<PulseBlock className="mt-4 h-7 w-14 rounded-2xl" />
									<PulseBlock className="mt-3 h-3 w-24 rounded-full" />
								</div>
							))}
						</div>
					</div>
				</div>

				<div className="grid gap-4 xl:grid-cols-2">
					{ADMIN_DASHBOARD_CHART_SKELETON_KEYS.map((key) => (
						<div
							key={key}
							className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6"
						>
							<PulseBlock className="h-4 w-24 rounded-full" />
							<PulseBlock className="mt-5 h-64 w-full rounded-[24px]" />
						</div>
					))}
				</div>
			</div>
		);
	}

	return (
		<div className="space-y-4">
			<div className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6">
				<div className="space-y-2">
					<PulseBlock className="h-4 w-28 rounded-full" />
					<PulseBlock className="h-7 w-56 rounded-2xl" />
				</div>
				<div className="mt-5 grid gap-3 md:grid-cols-4">
					<PulseBlock className="h-10 w-full rounded-2xl" />
					<PulseBlock className="h-10 w-full rounded-2xl" />
					<PulseBlock className="h-10 w-full rounded-2xl" />
					<PulseBlock className="h-10 w-full rounded-2xl" />
				</div>
			</div>

			<div className="grid gap-4 xl:grid-cols-[minmax(0,1fr)_300px]">
				<div className="space-y-3">
					{(variant === "users"
						? ADMIN_PRIMARY_SKELETON_KEYS
						: ADMIN_PRIMARY_SKELETON_KEYS.slice(0, 4)
					).map((key) => (
						<div
							key={key}
							className="rounded-[24px] border border-border/70 bg-card/72 p-4"
						>
							<div className="flex items-start justify-between gap-3">
								<div className="space-y-2">
									<PulseBlock className="h-4 w-28 rounded-full" />
									<PulseBlock className="h-3 w-44 rounded-full" />
									<PulseBlock className="h-3 w-24 rounded-full" />
								</div>
								<div className="flex gap-2">
									<PulseBlock className="h-9 w-16 rounded-xl" />
									<PulseBlock className="h-9 w-20 rounded-xl" />
								</div>
							</div>
						</div>
					))}
				</div>
				<div className="space-y-3">
					{ADMIN_ASIDE_SKELETON_KEYS.map((key) => (
						<div
							key={key}
							className="rounded-[24px] border border-border/70 bg-card/72 p-4"
						>
							<PulseBlock className="h-4 w-24 rounded-full" />
							<div className="mt-4 space-y-2">
								<PulseBlock className="h-3 w-full rounded-full" />
								<PulseBlock className="h-3 w-10/12 rounded-full" />
								<PulseBlock className="h-12 w-full rounded-2xl" />
							</div>
						</div>
					))}
				</div>
			</div>
		</div>
	);
}

export function AdminDashboardStartupSkeleton(props: { me: MeResponse }) {
	const { me } = props;
	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="dashboard" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<AdminSkeletonGrid variant="dashboard" />
		</AppShell>
	);
}

export function AdminUsersStartupSkeleton(props: { me: MeResponse }) {
	const { me } = props;
	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="users" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<AdminSkeletonGrid variant="users" />
		</AppShell>
	);
}

export function AdminJobsStartupSkeleton(props: { me: MeResponse }) {
	const { me } = props;
	return (
		<AppShell
			header={<AdminHeader user={me.user} activeNav="jobs" />}
			notice={<VersionUpdateNotice />}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<AdminSkeletonGrid variant="jobs" />
		</AppShell>
	);
}

export function SettingsStartupSkeleton(_props: { me: MeResponse }) {
	return (
		<AppShell
			header={
				<div className="flex flex-wrap items-center justify-between gap-3">
					<div className="flex min-w-0 items-center gap-3">
						<div className="size-9 shrink-0 rounded-2xl border border-border/60 bg-card p-1.5 shadow-sm">
							<BrandLogo
								variant="mark"
								alt=""
								className="size-full"
								imgClassName="size-full"
							/>
						</div>
						<div className="min-w-0 space-y-2">
							<PulseBlock className="h-3 w-16 rounded-full" />
							<PulseBlock className="h-7 w-32 rounded-2xl" />
						</div>
					</div>
					<div className="flex items-center gap-2">
						<PulseBlock className="size-9 rounded-2xl border border-border/60 bg-card/85" />
						<PulseBlock className="h-9 w-28 rounded-xl border border-border/60 bg-card/85" />
					</div>
				</div>
			}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="mx-auto max-w-3xl space-y-4">
				<div className="rounded-2xl border border-border/70 bg-card/95 p-2 shadow-sm">
					<div className="flex flex-wrap gap-2">
						<PulseBlock className="h-9 w-28 rounded-xl" />
						<PulseBlock className="h-9 w-28 rounded-xl" />
						<PulseBlock className="h-9 w-28 rounded-xl" />
						<PulseBlock className="h-9 w-28 rounded-xl" />
					</div>
				</div>
				<div className="rounded-[28px] border border-border/70 bg-card/78 p-5 shadow-sm sm:p-6">
					<div className="space-y-2">
						<PulseBlock className="h-4 w-28 rounded-full" />
						<PulseBlock className="h-7 w-52 rounded-2xl" />
					</div>
					<div className="mt-5 grid gap-3 sm:grid-cols-2">
						<PulseBlock className="h-20 w-full rounded-2xl" />
						<PulseBlock className="h-20 w-full rounded-2xl" />
						<PulseBlock className="h-20 w-full rounded-2xl" />
						<PulseBlock className="h-20 w-full rounded-2xl" />
					</div>
				</div>
			</div>
		</AppShell>
	);
}
