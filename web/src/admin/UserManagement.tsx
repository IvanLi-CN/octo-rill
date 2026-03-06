import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type AdminUserProfileResponse,
	ApiError,
	apiGet,
	apiGetAdminUserProfile,
	apiPatchJson,
} from "@/api";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";

type AdminRole = "all" | "admin" | "user";
type AdminStatus = "all" | "enabled" | "disabled";

export type AdminUserItem = {
	id: number;
	github_user_id: number;
	login: string;
	name: string | null;
	avatar_url: string | null;
	email: string | null;
	is_admin: boolean;
	is_disabled: boolean;
	last_active_at: string | null;
	created_at: string;
	updated_at: string;
};

type AdminUsersListResponse = {
	items: AdminUserItem[];
	page: number;
	page_size: number;
	total: number;
	guard?: {
		admin_total: number;
		active_admin_total: number;
	};
};

export type UserManagementStoryState = {
	queryInput?: string;
	query?: string;
	role?: AdminRole;
	status?: AdminStatus;
	profileUserId?: number;
	pendingAdminConfirmUserId?: number;
};

type UserManagementProps = {
	currentUserId: number;
	storyState?: UserManagementStoryState;
};

type PendingAdminConfirm = {
	user: AdminUserItem;
	nextIsAdmin: boolean;
};

const HM_FORMATTER = new Intl.DateTimeFormat(undefined, {
	hour: "2-digit",
	minute: "2-digit",
	hour12: false,
});

const PAGE_SIZE = 20;
const DEFAULT_GUARD = { admin_total: 0, active_admin_total: 0 };
const ROLE_OPTIONS: Array<{ value: AdminRole; label: string }> = [
	{ value: "all", label: "角色：全部" },
	{ value: "admin", label: "角色：管理员" },
	{ value: "user", label: "角色：普通用户" },
];
const STATUS_OPTIONS: Array<{ value: AdminStatus; label: string }> = [
	{ value: "all", label: "状态：全部" },
	{ value: "enabled", label: "状态：启用" },
	{ value: "disabled", label: "状态：禁用" },
];

function formatLocalHm(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return "-";
	return HM_FORMATTER.format(parsed);
}

function formatUtcClockToLocalHm(utcClock: string | null | undefined) {
	if (!utcClock) return "-";
	const [hourRaw, minuteRaw] = utcClock.split(":");
	const hour = Number(hourRaw);
	const minute = Number(minuteRaw ?? "0");
	if (!Number.isInteger(hour) || !Number.isInteger(minute)) return "-";
	const parsed = new Date(Date.UTC(1970, 0, 1, hour, minute, 0));
	if (Number.isNaN(parsed.getTime())) return "-";
	return HM_FORMATTER.format(parsed);
}

function toAdminUserErrorMessage(err: unknown) {
	if (err instanceof ApiError) {
		switch (err.code) {
			case "forbidden_admin_only":
				return "你当前没有管理员权限。请刷新页面确认账号状态。";
			case "account_disabled":
				return "账号已被禁用，请联系其他管理员处理。";
			case "last_admin_guard":
				return "至少保留一名启用管理员，当前操作已被拦截。";
			case "cannot_disable_self":
				return "不能禁用自己。";
			case "not_found":
				return "目标用户不存在，列表将自动刷新。";
			default:
				break;
		}
	}
	return err instanceof Error ? err.message : String(err);
}

function userRoleBadgeClass(isAdmin: boolean) {
	return isAdmin
		? "border-emerald-300 bg-emerald-100 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100"
		: "bg-muted/70 text-foreground";
}

function userStatusBadgeClass(isDisabled: boolean) {
	return isDisabled
		? "border-red-300 bg-red-100 text-red-900 dark:border-red-500/60 dark:bg-red-500/20 dark:text-red-100"
		: "border-sky-300 bg-sky-100 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100";
}

export function UserManagement({
	currentUserId,
	storyState,
}: UserManagementProps) {
	const [queryInput, setQueryInput] = useState(storyState?.queryInput ?? "");
	const [query, setQuery] = useState(storyState?.query ?? "");
	const [role, setRole] = useState<AdminRole>(storyState?.role ?? "all");
	const [status, setStatus] = useState<AdminStatus>(
		storyState?.status ?? "all",
	);
	const [page, setPage] = useState(1);

	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [items, setItems] = useState<AdminUserItem[]>([]);
	const [total, setTotal] = useState(0);
	const [guardSummary, setGuardSummary] = useState<{
		admin_total: number;
		active_admin_total: number;
	}>(DEFAULT_GUARD);
	const [actionBusyUserId, setActionBusyUserId] = useState<number | null>(null);
	const [pendingAdminConfirm, setPendingAdminConfirm] =
		useState<PendingAdminConfirm | null>(null);
	const [profileUser, setProfileUser] = useState<AdminUserItem | null>(null);
	const [profileLoading, setProfileLoading] = useState(false);
	const [profileError, setProfileError] = useState<string | null>(null);
	const [profile, setProfile] = useState<AdminUserProfileResponse | null>(null);

	const storyProfileInitializedRef = useRef(false);
	const storyConfirmInitializedRef = useRef(false);

	const totalPages = useMemo(
		() => Math.max(1, Math.ceil(total / PAGE_SIZE)),
		[total],
	);

	const loadUsers = useCallback(
		async (options?: { clearError?: boolean }) => {
			const clearError = options?.clearError ?? true;
			setLoading(true);
			if (clearError) {
				setError(null);
			}
			try {
				const params = new URLSearchParams();
				if (query.trim()) params.set("query", query.trim());
				params.set("role", role);
				params.set("status", status);
				params.set("page", String(page));
				params.set("page_size", String(PAGE_SIZE));

				const res = await apiGet<AdminUsersListResponse>(
					`/api/admin/users?${params.toString()}`,
				);
				setItems(res.items);
				setTotal(res.total);
				setGuardSummary(
					res.guard ?? {
						admin_total: res.items.filter((item) => item.is_admin).length,
						active_admin_total: res.items.filter(
							(item) => item.is_admin && !item.is_disabled,
						).length,
					},
				);
			} catch (err) {
				if (err instanceof ApiError && err.code === "forbidden_admin_only") {
					// Permission can change within the same session (e.g. self demotion),
					// so force a full reload to refresh /api/me and exit admin view.
					window.location.replace("/");
					return;
				}
				setError(toAdminUserErrorMessage(err));
			} finally {
				setLoading(false);
			}
		},
		[page, query, role, status],
	);

	useEffect(() => {
		void loadUsers();
	}, [loadUsers]);

	const onApplyFilters = useCallback(() => {
		setPage(1);
		setQuery(queryInput.trim());
	}, [queryInput]);

	const patchUser = useCallback(
		async (
			userId: number,
			payload: { is_admin?: boolean; is_disabled?: boolean },
		) => {
			setActionBusyUserId(userId);
			setError(null);
			try {
				await apiPatchJson<AdminUserItem>(
					`/api/admin/users/${userId}`,
					payload,
				);
				await loadUsers();
			} catch (err) {
				setError(toAdminUserErrorMessage(err));
				await loadUsers({ clearError: false });
			} finally {
				setActionBusyUserId(null);
			}
		},
		[loadUsers],
	);

	const onToggleAdmin = useCallback((user: AdminUserItem) => {
		setPendingAdminConfirm({
			user,
			nextIsAdmin: !user.is_admin,
		});
	}, []);

	const onConfirmToggleAdmin = useCallback(async () => {
		if (!pendingAdminConfirm) return;
		const { user, nextIsAdmin } = pendingAdminConfirm;
		setPendingAdminConfirm(null);

		await patchUser(user.id, { is_admin: nextIsAdmin });
	}, [patchUser, pendingAdminConfirm]);

	const onToggleDisabled = useCallback(
		async (user: AdminUserItem) => {
			await patchUser(user.id, { is_disabled: !user.is_disabled });
		},
		[patchUser],
	);

	const onOpenProfile = useCallback(async (user: AdminUserItem) => {
		setProfileUser(user);
		setProfileLoading(true);
		setProfileError(null);
		setProfile(null);
		try {
			const detail = await apiGetAdminUserProfile(user.id);
			setProfile(detail);
		} catch (err) {
			setProfileError(toAdminUserErrorMessage(err));
		} finally {
			setProfileLoading(false);
		}
	}, []);

	const onCloseProfile = useCallback(() => {
		setProfileUser(null);
		setProfile(null);
		setProfileError(null);
		setProfileLoading(false);
	}, []);

	useEffect(() => {
		if (storyProfileInitializedRef.current) return;
		if (!storyState?.profileUserId || items.length === 0) return;
		const target = items.find((user) => user.id === storyState.profileUserId);
		if (!target) return;
		storyProfileInitializedRef.current = true;
		void onOpenProfile(target);
	}, [items, onOpenProfile, storyState?.profileUserId]);

	useEffect(() => {
		if (storyConfirmInitializedRef.current) return;
		if (!storyState?.pendingAdminConfirmUserId || items.length === 0) return;
		const target = items.find(
			(user) => user.id === storyState.pendingAdminConfirmUserId,
		);
		if (!target) return;
		storyConfirmInitializedRef.current = true;
		setPendingAdminConfirm({
			user: target,
			nextIsAdmin: !target.is_admin,
		});
	}, [items, storyState?.pendingAdminConfirmUserId]);

	return (
		<Card>
			<CardHeader>
				<CardTitle>用户管理</CardTitle>
				<CardDescription>
					管理账号角色与状态：支持筛选、升降管理员、启用/禁用。
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-4">
				<div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_180px_180px_auto]">
					<Input
						type="text"
						value={queryInput}
						onChange={(e) => setQueryInput(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") onApplyFilters();
						}}
						placeholder="搜索 login / name / email"
						aria-label="搜索 login、name 或 email"
					/>
					<Select
						value={role}
						onValueChange={(value) => {
							setRole(value as AdminRole);
							setPage(1);
						}}
					>
						<SelectTrigger className="w-full" aria-label="按角色筛选">
							<SelectValue placeholder="角色：全部" />
						</SelectTrigger>
						<SelectContent>
							{ROLE_OPTIONS.map((option) => (
								<SelectItem key={option.value} value={option.value}>
									{option.label}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					<Select
						value={status}
						onValueChange={(value) => {
							setStatus(value as AdminStatus);
							setPage(1);
						}}
					>
						<SelectTrigger className="w-full" aria-label="按状态筛选">
							<SelectValue placeholder="状态：全部" />
						</SelectTrigger>
						<SelectContent>
							{STATUS_OPTIONS.map((option) => (
								<SelectItem key={option.value} value={option.value}>
									{option.label}
								</SelectItem>
							))}
						</SelectContent>
					</Select>
					<Button onClick={onApplyFilters} disabled={loading}>
						筛选
					</Button>
				</div>

				{error ? <p className="text-destructive text-sm">{error}</p> : null}

				<div className="space-y-2">
					{loading ? (
						<p className="text-muted-foreground text-sm">正在加载用户列表...</p>
					) : items.length === 0 ? (
						<p className="text-muted-foreground text-sm">没有匹配的用户。</p>
					) : (
						items.map((user) => {
							const busy = actionBusyUserId === user.id;
							const isSelf = user.id === currentUserId;
							const isLastAdmin =
								user.is_admin && guardSummary.admin_total <= 1;
							const isLastActiveAdmin =
								user.is_admin &&
								!user.is_disabled &&
								guardSummary.active_admin_total <= 1;
							const adminActionBlocked = user.is_admin
								? isLastAdmin || isLastActiveAdmin
								: false;
							const disableActionBlocked =
								(isSelf && !user.is_disabled) || isLastActiveAdmin;
							const adminActionHint =
								user.is_admin && isLastAdmin
									? "唯一管理员，不能撤销"
									: user.is_admin && isLastActiveAdmin
										? "最后一名启用管理员，不能撤销"
										: null;
							const disableActionHint =
								!user.is_disabled && isSelf
									? "不能禁用自己"
									: !user.is_disabled && isLastActiveAdmin
										? "最后一名启用管理员，不能禁用"
										: null;
							const guardHint = adminActionHint ?? disableActionHint;
							return (
								<div
									key={user.id}
									className="bg-card/70 flex flex-col gap-3 rounded-lg border p-3 md:flex-row md:items-center md:justify-between"
								>
									<div className="min-w-0">
										<p className="font-medium text-sm">
											{user.login}
											{user.id === currentUserId ? "（你）" : ""}
										</p>
										<p className="text-muted-foreground truncate text-xs">
											{user.name ?? "-"} · {user.email ?? "-"}
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											最后活动：{formatLocalHm(user.last_active_at)}
										</p>
										<div className="mt-1 flex flex-wrap gap-1">
											<Badge
												variant="outline"
												className="font-mono text-[11px]"
											>
												UID:{user.id}
											</Badge>
											<Badge
												variant="outline"
												className={userRoleBadgeClass(user.is_admin)}
											>
												{user.is_admin ? "管理员" : "普通用户"}
											</Badge>
											<Badge
												variant="outline"
												className={userStatusBadgeClass(user.is_disabled)}
											>
												{user.is_disabled ? "已禁用" : "已启用"}
											</Badge>
										</div>
										{guardHint ? (
											<p className="text-muted-foreground mt-1 text-xs">
												{guardHint}
											</p>
										) : null}
									</div>
									<div className="flex flex-wrap gap-2">
										<Button
											variant="outline"
											disabled={busy}
											onClick={() => void onOpenProfile(user)}
										>
											详情
										</Button>
										<Button
											variant="outline"
											disabled={busy || adminActionBlocked}
											onClick={() => void onToggleAdmin(user)}
										>
											{user.is_admin ? "撤销管理员" : "设为管理员"}
										</Button>
										<Button
											variant={user.is_disabled ? "secondary" : "destructive"}
											disabled={busy || disableActionBlocked}
											onClick={() => void onToggleDisabled(user)}
										>
											{user.is_disabled ? "启用" : "禁用"}
										</Button>
									</div>
								</div>
							);
						})
					)}
				</div>

				<div className="flex items-center justify-between">
					<p className="text-muted-foreground text-xs">
						共 {total} 人 · 第 {page}/{totalPages} 页
					</p>
					<div className="flex gap-2">
						<Button
							variant="outline"
							size="sm"
							disabled={page <= 1 || loading}
							onClick={() => setPage((prev) => Math.max(1, prev - 1))}
						>
							上一页
						</Button>
						<Button
							variant="outline"
							size="sm"
							disabled={page >= totalPages || loading}
							onClick={() => setPage((prev) => Math.min(totalPages, prev + 1))}
						>
							下一页
						</Button>
					</div>
				</div>
			</CardContent>

			<Sheet
				open={Boolean(profileUser)}
				onOpenChange={(open) => {
					if (!open) onCloseProfile();
				}}
			>
				<SheetContent
					side="right"
					className="w-full max-w-md gap-0 p-0 sm:max-w-md"
				>
					<SheetHeader className="px-5 pt-5">
						<SheetTitle>用户详情</SheetTitle>
						<SheetDescription>
							{profileUser
								? `${profileUser.login}${profileUser.id === currentUserId ? "（你）" : ""}`
								: "用户详情加载中"}
						</SheetDescription>
					</SheetHeader>
					<div className="space-y-4 px-5 pb-5">
						<div className="space-y-3 text-sm">
							<div className="rounded-lg border p-3">
								<p className="text-muted-foreground text-xs">
									最后活动（本地时区）
								</p>
								<p className="mt-1 font-medium">
									{formatLocalHm(
										profile?.last_active_at ?? profileUser?.last_active_at,
									)}
								</p>
							</div>
							<div className="rounded-lg border p-3">
								<p className="text-muted-foreground text-xs">
									日报时间（本地时区）
								</p>
								<p className="mt-1 font-medium">
									{formatUtcClockToLocalHm(profile?.daily_brief_utc_time)}
								</p>
								<p className="text-muted-foreground mt-1 text-xs">
									UTC 原值：{profile?.daily_brief_utc_time ?? "-"}
								</p>
							</div>
							<div className="rounded-lg border p-3">
								<p className="text-muted-foreground text-xs">账号状态</p>
								<p className="mt-1 font-medium">
									{profileUser?.is_disabled ? "已禁用" : "已启用"} ·{" "}
									{profileUser?.is_admin ? "管理员" : "普通用户"}
								</p>
							</div>
						</div>

						{profileLoading ? (
							<p className="text-muted-foreground text-sm">详情加载中...</p>
						) : null}
						{profileError ? (
							<p className="text-destructive text-sm">{profileError}</p>
						) : null}

						<div className="flex justify-end">
							<Button variant="outline" onClick={onCloseProfile}>
								关闭
							</Button>
						</div>
					</div>
				</SheetContent>
			</Sheet>

			<AlertDialog
				open={Boolean(pendingAdminConfirm)}
				onOpenChange={(open) => {
					if (!open) setPendingAdminConfirm(null);
				}}
			>
				<AlertDialogContent>
					<AlertDialogHeader>
						<AlertDialogTitle>确认管理员变更</AlertDialogTitle>
						<AlertDialogDescription>
							{pendingAdminConfirm
								? pendingAdminConfirm.nextIsAdmin
									? `确认将 ${pendingAdminConfirm.user.login} 设为管理员吗？`
									: `确认撤销 ${pendingAdminConfirm.user.login} 的管理员权限吗？`
								: "确认管理员权限变更"}
						</AlertDialogDescription>
					</AlertDialogHeader>
					<p className="text-muted-foreground text-xs">
						此操作属于高权限变更，需要二次确认后才会提交。
					</p>
					<AlertDialogFooter>
						<AlertDialogCancel>取消</AlertDialogCancel>
						<AlertDialogAction onClick={() => void onConfirmToggleAdmin()}>
							确认更改
						</AlertDialogAction>
					</AlertDialogFooter>
				</AlertDialogContent>
			</AlertDialog>
		</Card>
	);
}
