import {
	ArrowLeft,
	CalendarClock,
	ExternalLink,
	KeyRound,
	LoaderCircle,
	Link2,
	Package,
	ShieldAlert,
	Unlink2,
} from "lucide-react";
import {
	type ReactNode,
	useCallback,
	useEffect,
	useMemo,
	useState,
} from "react";

import {
	type DailyBriefProfilePatchRequest,
	type LinuxDoConnectionResponse,
	type MeResponse,
	type MeProfileResponse,
	apiDeleteMeLinuxDo,
	apiGetMeLinuxDo,
	apiGetMeProfile,
	apiPatchMeProfile,
} from "@/api";
import {
	DailyBriefProfileForm,
	readHourAlignedBrowserTimeZone,
} from "@/briefs/DailyBriefProfileForm";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { InternalLink } from "@/lib/internalNavigation";
import { cn } from "@/lib/utils";
import { ThemeToggle } from "@/components/theme/ThemeToggle";
import {
	buildSettingsHref,
	buildSettingsSearch,
	type SettingsSection,
} from "@/settings/routeState";
import {
	PAT_CREATE_PATH,
	useReactionTokenEditor,
} from "@/settings/reactionTokenEditor";
import { GitHubPatInput } from "@/settings/GitHubPatInput";

const SECTION_META: Record<
	SettingsSection,
	{
		label: string;
		description: string;
	}
> = {
	linuxdo: {
		label: "LinuxDO 绑定",
		description:
			"绑定 LinuxDO Connect 账号，只保存本地快照，不存 LinuxDO PAT。",
	},
	"my-releases": {
		label: "我的发布",
		description:
			"把你自己 owner 的仓库发布也纳入 release 阅读面，但不会写进真实加星列表。",
	},
	"github-pat": {
		label: "GitHub PAT",
		description:
			"配置 release feedback 所需的 GitHub PAT，保留 800ms 防抖校验。",
	},
	"daily-brief": {
		label: "日报设置",
		description: "调整日报生成边界，继续沿用现有 /api/me/profile 契约。",
	},
};

const LINUXDO_STATUS_META: Record<
	string,
	{
		tone: "success" | "error";
		title: string;
		description: string;
	}
> = {
	connected: {
		tone: "success",
		title: "LinuxDO 已绑定",
		description: "绑定快照已同步到当前账号，现在可以在这里查看或解绑。",
	},
	already_bound: {
		tone: "error",
		title: "LinuxDO 账号已被占用",
		description:
			"这个 LinuxDO 账号已经绑定到其他 OctoRill 用户，请换账号后重试。",
	},
	not_configured: {
		tone: "error",
		title: "LinuxDO Connect 尚未配置",
		description: "服务端缺少 LinuxDO OAuth 环境变量，当前环境无法发起绑定。",
	},
	state_mismatch: {
		tone: "error",
		title: "绑定状态校验失败",
		description: "OAuth state 不匹配，请重新发起一次 LinuxDO Connect 绑定。",
	},
	exchange_failed: {
		tone: "error",
		title: "LinuxDO 授权交换失败",
		description: "服务端没能完成 code → token 交换，请稍后重试。",
	},
	fetch_user_failed: {
		tone: "error",
		title: "LinuxDO 用户信息获取失败",
		description: "授权成功后没能读取 LinuxDO 用户资料，请稍后再试。",
	},
	save_failed: {
		tone: "error",
		title: "LinuxDO 绑定保存失败",
		description: "本地绑定快照写入失败，请稍后重试。",
	},
};

function formatDateTime(value: string | null | undefined) {
	if (!value) return "—";
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? value : date.toLocaleString();
}

function statusToneClassName(tone: "success" | "error" | "idle" | "muted") {
	switch (tone) {
		case "success":
			return "border-emerald-500/30 bg-emerald-500/10 text-emerald-700 dark:text-emerald-300";
		case "error":
			return "border-destructive/30 bg-destructive/8 text-destructive";
		case "muted":
			return "border-border bg-muted/30 text-muted-foreground";
		default:
			return "border-border bg-background text-foreground";
	}
}

function DetailItem(props: {
	label: string;
	value: ReactNode;
	hint?: ReactNode;
	mono?: boolean;
}) {
	const { label, value, hint, mono = false } = props;
	return (
		<div className="space-y-1.5 rounded-xl border border-border/70 bg-background/80 p-3">
			<p className="text-muted-foreground text-[11px] font-medium tracking-[0.16em] uppercase">
				{label}
			</p>
			<div
				className={cn(
					"text-sm leading-6 text-foreground",
					mono && "font-mono text-[13px]",
				)}
			>
				{value}
			</div>
			{hint ? (
				<div className="text-muted-foreground text-xs leading-5">{hint}</div>
			) : null}
		</div>
	);
}

export function SettingsPage(props: {
	me: MeResponse;
	section: SettingsSection;
	linuxdoStatus?: string | null;
	onSectionChange: (section: SettingsSection) => void;
	onProfileSaved?: () => Promise<void> | void;
}) {
	const {
		me,
		section,
		linuxdoStatus = null,
		onSectionChange,
		onProfileSaved,
	} = props;
	const [linuxdoLoading, setLinuxdoLoading] = useState(true);
	const [linuxdoBusy, setLinuxdoBusy] = useState(false);
	const [linuxdoError, setLinuxdoError] = useState<string | null>(null);
	const [linuxdoAvailable, setLinuxdoAvailable] = useState(false);
	const [linuxdoConnection, setLinuxdoConnection] =
		useState<LinuxDoConnectionResponse | null>(null);
	const {
		reactionTokenLoading,
		reactionTokenConfigured,
		reactionTokenMasked,
		patInput,
		setPatInput,
		patCheckState,
		patCheckMessage,
		patCheckedAt,
		patSaving,
		canSavePat,
		savePat,
	} = useReactionTokenEditor();

	const [briefProfileLoading, setBriefProfileLoading] = useState(true);
	const [briefProfileSaving, setBriefProfileSaving] = useState(false);
	const [ownReleaseSaving, setOwnReleaseSaving] = useState(false);
	const [_briefProfile, setBriefProfile] = useState<MeProfileResponse | null>(
		null,
	);
	const [briefProfileError, setBriefProfileError] = useState<string | null>(
		null,
	);
	const [briefProfileDraft, setBriefProfileDraft] =
		useState<DailyBriefProfilePatchRequest>({
			daily_brief_local_time: me.dashboard.daily_boundary_local,
			daily_brief_time_zone:
				me.dashboard.daily_boundary_time_zone ??
				readHourAlignedBrowserTimeZone() ??
				"Asia/Shanghai",
		});
	const [includeOwnReleases, setIncludeOwnReleases] = useState(false);

	const activeStatusMeta = linuxdoStatus
		? (LINUXDO_STATUS_META[linuxdoStatus] ?? null)
		: null;

	const loadLinuxDo = useCallback(async () => {
		setLinuxdoLoading(true);
		setLinuxdoError(null);
		try {
			const res = await apiGetMeLinuxDo();
			setLinuxdoAvailable(res.available);
			setLinuxdoConnection(res.connection);
		} catch (err) {
			setLinuxdoError(err instanceof Error ? err.message : String(err));
		} finally {
			setLinuxdoLoading(false);
		}
	}, []);

	const loadBriefProfile = useCallback(async () => {
		setBriefProfileLoading(true);
		setBriefProfileError(null);
		try {
			const profile = await apiGetMeProfile();
			setBriefProfile(profile);
			setBriefProfileDraft({
				daily_brief_local_time: profile.daily_brief_local_time,
				daily_brief_time_zone: profile.daily_brief_time_zone,
			});
			setIncludeOwnReleases(profile.include_own_releases);
		} catch (err) {
			setBriefProfileError(err instanceof Error ? err.message : String(err));
		} finally {
			setBriefProfileLoading(false);
		}
	}, []);

	useEffect(() => {
		void Promise.all([loadLinuxDo(), loadBriefProfile()]);
	}, [loadBriefProfile, loadLinuxDo]);

	const onConnectLinuxDo = useCallback(() => {
		window.location.assign("/auth/linuxdo/login");
	}, []);

	const onDisconnectLinuxDo = useCallback(() => {
		setLinuxdoBusy(true);
		setLinuxdoError(null);
		void apiDeleteMeLinuxDo()
			.then((res) => {
				setLinuxdoAvailable(res.available);
				setLinuxdoConnection(res.connection);
			})
			.catch((err) => {
				setLinuxdoError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setLinuxdoBusy(false);
			});
	}, []);

	const onSavePat = useCallback(() => {
		void savePat();
	}, [savePat]);

	const onSaveBriefProfile = useCallback(() => {
		setBriefProfileSaving(true);
		setBriefProfileError(null);
		void apiPatchMeProfile({
			...briefProfileDraft,
			include_own_releases: includeOwnReleases,
		})
			.then(async (profile) => {
				setBriefProfile(profile);
				setBriefProfileDraft({
					daily_brief_local_time: profile.daily_brief_local_time,
					daily_brief_time_zone: profile.daily_brief_time_zone,
				});
				setIncludeOwnReleases(profile.include_own_releases);
				await onProfileSaved?.();
			})
			.catch((err) => {
				setBriefProfileError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setBriefProfileSaving(false);
			});
	}, [briefProfileDraft, includeOwnReleases, onProfileSaved]);

	const onSaveOwnReleases = useCallback(() => {
		setOwnReleaseSaving(true);
		setBriefProfileError(null);
		void apiPatchMeProfile({
			...briefProfileDraft,
			include_own_releases: includeOwnReleases,
		})
			.then(async (profile) => {
				setBriefProfile(profile);
				setBriefProfileDraft({
					daily_brief_local_time: profile.daily_brief_local_time,
					daily_brief_time_zone: profile.daily_brief_time_zone,
				});
				setIncludeOwnReleases(profile.include_own_releases);
				await onProfileSaved?.();
			})
			.catch((err) => {
				setBriefProfileError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				setOwnReleaseSaving(false);
			});
	}, [briefProfileDraft, includeOwnReleases, onProfileSaved]);

	const patTone = useMemo(() => {
		if (patCheckState === "valid") return "success";
		if (patCheckState === "invalid" || patCheckState === "error")
			return "error";
		if (patCheckState === "checking") return "muted";
		return "idle";
	}, [patCheckState]);

	const briefSummary = useMemo(() => {
		if (briefProfileLoading) return "读取中";
		return `${briefProfileDraft.daily_brief_local_time} · ${briefProfileDraft.daily_brief_time_zone}`;
	}, [
		briefProfileDraft.daily_brief_local_time,
		briefProfileDraft.daily_brief_time_zone,
		briefProfileLoading,
	]);
	const ownReleaseSummary = includeOwnReleases ? "已开启" : "已关闭";
	const profileBusy =
		briefProfileLoading || briefProfileSaving || ownReleaseSaving;

	const linuxdoStatusBadge = linuxdoLoading
		? { label: "读取中", variant: "outline" as const }
		: linuxdoConnection
			? { label: "已绑定", variant: "secondary" as const }
			: linuxdoAvailable
				? { label: "未绑定", variant: "outline" as const }
				: { label: "未启用", variant: "outline" as const };

	const patStatusBadge =
		patCheckState === "valid"
			? { label: "PAT 可用", variant: "secondary" as const }
			: patCheckState === "invalid"
				? { label: "PAT 无效", variant: "destructive" as const }
				: patCheckState === "error"
					? { label: "校验异常", variant: "destructive" as const }
					: patCheckState === "checking"
						? { label: "校验中", variant: "outline" as const }
						: reactionTokenConfigured
							? { label: "已配置", variant: "secondary" as const }
							: { label: "未配置", variant: "outline" as const };

	const sectionNavItems = [
		{
			id: "daily-brief" as const,
			icon: <CalendarClock className="size-4" />,
		},
		{
			id: "my-releases" as const,
			icon: <Package className="size-4" />,
		},
		{
			id: "github-pat" as const,
			icon: <KeyRound className="size-4" />,
		},
		{
			id: "linuxdo" as const,
			icon: <Link2 className="size-4" />,
		},
	];

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
						<div className="min-w-0">
							<p className="text-muted-foreground text-xs font-medium tracking-[0.18em] uppercase">
								Settings
							</p>
							<h1 className="truncate text-xl font-semibold tracking-tight text-foreground">
								账号与偏好
							</h1>
						</div>
					</div>
					<div className="flex items-center gap-2">
						<ThemeToggle />
						<Button asChild variant="outline" size="sm">
							<InternalLink href="/" to="/">
								<ArrowLeft className="size-4" />
								返回工作台
							</InternalLink>
						</Button>
					</div>
				</div>
			}
			footer={<AppMetaFooter />}
			mobileChrome
		>
			<div className="mx-auto max-w-3xl space-y-4">
				{activeStatusMeta?.tone === "error" ? (
					<section
						className={cn(
							"rounded-xl border px-3 py-2.5 text-sm shadow-sm",
							statusToneClassName("error"),
						)}
					>
						<div className="flex items-start gap-2.5">
							<ShieldAlert className="mt-0.5 size-4 shrink-0" />
							<div className="space-y-0.5">
								<p className="font-medium">{activeStatusMeta.title}</p>
								<p className="text-xs leading-5">
									{activeStatusMeta.description}
								</p>
							</div>
						</div>
					</section>
				) : null}

				<nav className="flex flex-wrap gap-2 rounded-2xl border border-border/70 bg-card/95 p-2 shadow-sm">
					{sectionNavItems.map((item) => (
						<Button
							key={item.id}
							asChild
							variant={section === item.id ? "default" : "ghost"}
							size="sm"
							className="h-9 rounded-xl px-3"
						>
							<InternalLink
								href={buildSettingsHref(item.id)}
								to="/settings"
								search={buildSettingsSearch(item.id)}
								onClick={() => onSectionChange(item.id)}
							>
								{item.icon}
								{SECTION_META[item.id].label}
							</InternalLink>
						</Button>
					))}
				</nav>

				<div className="min-w-0">
					{section === "linuxdo" ? (
						<section id="settings-linuxdo" data-settings-section="linuxdo">
							<Card className="border-border/70 shadow-sm">
								<CardHeader className="border-b border-border/60 p-5">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META.linuxdo.label}
											</CardTitle>
											<Badge variant={linuxdoStatusBadge.variant}>
												{linuxdoStatusBadge.label}
											</Badge>
										</div>
										{linuxdoConnection ? (
											<Button
												variant="outline"
												size="sm"
												disabled={linuxdoBusy}
												onClick={onDisconnectLinuxDo}
											>
												{linuxdoBusy ? (
													<LoaderCircle className="size-4 animate-spin" />
												) : (
													<Unlink2 className="size-4" />
												)}
												解绑 LinuxDO
											</Button>
										) : (
											<Button
												size="sm"
												disabled={!linuxdoAvailable}
												onClick={onConnectLinuxDo}
											>
												<ExternalLink className="size-4" />
												Connect LinuxDO
											</Button>
										)}
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5">
									{linuxdoError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{linuxdoError}
										</div>
									) : null}

									{linuxdoLoading ? (
										<div className="text-muted-foreground flex items-center gap-2 text-sm">
											<LoaderCircle className="size-4 animate-spin" />
											正在读取 LinuxDO 绑定状态…
										</div>
									) : linuxdoConnection ? (
										<>
											<div className="flex min-w-0 items-center gap-3 rounded-2xl border border-border/70 bg-muted/20 p-4">
												{linuxdoConnection.avatar_url ? (
													<img
														src={linuxdoConnection.avatar_url}
														alt={`${linuxdoConnection.username} avatar`}
														className="size-12 rounded-full border border-border/70 object-cover"
														referrerPolicy="no-referrer"
													/>
												) : (
													<div className="bg-muted flex size-12 items-center justify-center rounded-full border border-border/70 text-xs font-semibold">
														LD
													</div>
												)}
												<div className="min-w-0">
													<p className="truncate text-sm font-semibold text-foreground">
														{linuxdoConnection.name ??
															linuxdoConnection.username}
													</p>
													<p className="text-muted-foreground truncate text-xs">
														@{linuxdoConnection.username}
													</p>
												</div>
											</div>

											<div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
												<DetailItem
													label="Trust level"
													value={linuxdoConnection.trust_level}
												/>
												<DetailItem
													label="账号状态"
													value={linuxdoConnection.active ? "正常" : "非活跃"}
												/>
												<DetailItem
													label="发言状态"
													value={linuxdoConnection.silenced ? "受限" : "正常"}
												/>
												<DetailItem
													label="绑定时间"
													value={formatDateTime(linuxdoConnection.linked_at)}
												/>
												<DetailItem
													label="更新时间"
													value={formatDateTime(linuxdoConnection.updated_at)}
												/>
											</div>
										</>
									) : (
										<div className="rounded-2xl border border-border/70 bg-muted/20 px-4 py-3 text-sm text-foreground">
											未绑定 LinuxDO。
										</div>
									)}
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "my-releases" ? (
						<section
							id="settings-my-releases"
							data-settings-section="my-releases"
						>
							<Card className="border-border/70 shadow-sm">
								<CardHeader className="border-b border-border/60 p-5">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["my-releases"].label}
											</CardTitle>
											<Badge
												variant={includeOwnReleases ? "secondary" : "outline"}
											>
												{ownReleaseSummary}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={profileBusy}
											onClick={onSaveOwnReleases}
										>
											{ownReleaseSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<Package className="size-4" />
											)}
											保存“我的发布”
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5">
									<div className="flex items-start justify-between gap-4 rounded-2xl border border-border/70 bg-muted/20 px-4 py-3">
										<div className="space-y-1.5">
											<p className="text-sm font-medium text-foreground">
												把自己的仓库发布也纳入发布流
											</p>
											<p className="text-muted-foreground text-sm leading-6">
												开启后，当前 GitHub 账号本人 owner 的仓库 release
												会像已加星仓库一样进入 “全部 /
												发布”、详情、翻译、智能总结与日报链路；真实“加星”列表和社交动态不会被污染。
											</p>
										</div>
										<Switch
											checked={includeOwnReleases}
											onCheckedChange={setIncludeOwnReleases}
											aria-label="我的发布"
											disabled={profileBusy}
										/>
									</div>

									{briefProfileError ? (
										<div
											className={cn(
												"rounded-xl border px-3 py-2.5 text-sm",
												statusToneClassName("error"),
											)}
										>
											{briefProfileError}
										</div>
									) : null}

									<div className="grid gap-3 sm:grid-cols-2">
										<DetailItem
											label="当前状态"
											value={
												includeOwnReleases
													? "已纳入我的发布"
													: "仅显示已加星仓库"
											}
										/>
										<DetailItem
											label="影响范围"
											value="发布列表 / Release 详情 / 翻译与智能总结 / 日报"
											hint="不影响真实加星列表，也不会新增社交事件。"
										/>
									</div>
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "github-pat" ? (
						<section
							id="settings-github-pat"
							data-settings-section="github-pat"
						>
							<Card className="border-border/70 shadow-sm">
								<CardHeader className="border-b border-border/60 p-5">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["github-pat"].label}
											</CardTitle>
											<Badge variant={patStatusBadge.variant}>
												{patStatusBadge.label}
											</Badge>
										</div>
										<Button
											size="sm"
											disabled={patSaving || !canSavePat}
											onClick={onSavePat}
										>
											{patSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<KeyRound className="size-4" />
											)}
											保存 GitHub PAT
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5">
									<div className="space-y-2">
										<Label htmlFor="settings-reaction-pat">GitHub PAT</Label>
										<GitHubPatInput
											id="settings-reaction-pat"
											value={patInput}
											onChange={(event) => setPatInput(event.target.value)}
											placeholder="粘贴新的 classic PAT"
											autoCapitalize="none"
											autoCorrect="off"
											spellCheck={false}
											inputClassName="h-10 font-mono text-sm"
										/>
									</div>

									<div
										className={cn(
											"rounded-xl border px-3 py-2.5 text-sm",
											statusToneClassName(patTone),
										)}
									>
										<p className="font-medium">
											{patCheckState === "checking"
												? "正在校验 GitHub PAT"
												: patCheckState === "valid"
													? "GitHub PAT 可用"
													: patCheckState === "invalid"
														? "GitHub PAT 无效"
														: patCheckState === "error"
															? "GitHub PAT 校验失败"
															: reactionTokenConfigured
																? "已保存 GitHub PAT"
																: "尚未填写新的 GitHub PAT"}
										</p>
										<p className="mt-1 text-xs leading-5">
											{patCheckMessage ??
												"输入后会在 800ms 后自动校验；只有 valid 时允许保存。"}
										</p>
									</div>

									<div className="grid gap-3 sm:grid-cols-2">
										<DetailItem
											label="当前保存"
											value={
												reactionTokenLoading
													? "读取中…"
													: (reactionTokenMasked ?? "未配置")
											}
										/>
										<DetailItem
											label="最近检查"
											value={patCheckedAt ? formatDateTime(patCheckedAt) : "—"}
										/>
									</div>

									<details className="rounded-xl border border-border/70 px-3 py-2.5 text-sm">
										<summary className="cursor-pointer font-medium">
											创建 classic PAT
										</summary>
										<p className="text-muted-foreground mt-2 font-mono text-xs leading-5">
											{PAT_CREATE_PATH}
										</p>
									</details>
								</CardContent>
							</Card>
						</section>
					) : null}

					{section === "daily-brief" ? (
						<section
							id="settings-daily-brief"
							data-settings-section="daily-brief"
						>
							<Card className="border-border/70 shadow-sm">
								<CardHeader className="border-b border-border/60 p-5">
									<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
										<div className="flex flex-wrap items-center gap-2">
											<CardTitle className="text-lg">
												{SECTION_META["daily-brief"].label}
											</CardTitle>
											<Badge variant="outline">{briefSummary}</Badge>
										</div>
										<Button
											size="sm"
											disabled={profileBusy}
											onClick={onSaveBriefProfile}
										>
											{briefProfileSaving ? (
												<LoaderCircle className="size-4 animate-spin" />
											) : (
												<CalendarClock className="size-4" />
											)}
											保存日报设置
										</Button>
									</div>
								</CardHeader>
								<CardContent className="space-y-4 p-5">
									<DailyBriefProfileForm
										localTime={briefProfileDraft.daily_brief_local_time}
										timeZone={briefProfileDraft.daily_brief_time_zone}
										disabled={profileBusy}
										error={briefProfileError}
										compact
										helperText={null}
										onLocalTimeChange={(value) =>
											setBriefProfileDraft((current) => ({
												...current,
												daily_brief_local_time: value,
											}))
										}
										onTimeZoneChange={(value) =>
											setBriefProfileDraft((current) => ({
												...current,
												daily_brief_time_zone: value,
											}))
										}
										onUseBrowserTimeZone={(timeZone) =>
											setBriefProfileDraft((current) => ({
												...current,
												daily_brief_time_zone: timeZone,
											}))
										}
									/>
								</CardContent>
							</Card>
						</section>
					) : null}
				</div>
			</div>
		</AppShell>
	);
}
