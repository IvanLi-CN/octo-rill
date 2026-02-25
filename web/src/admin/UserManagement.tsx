import { useCallback, useEffect, useMemo, useState } from "react";

import { ApiError, apiGet, apiPatchJson } from "@/api";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

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
	created_at: string;
	updated_at: string;
};

type AdminUsersListResponse = {
	items: AdminUserItem[];
	page: number;
	page_size: number;
	total: number;
};

type UserManagementProps = {
	currentUserId: number;
};

const PAGE_SIZE = 20;

export function UserManagement({ currentUserId }: UserManagementProps) {
	const [queryInput, setQueryInput] = useState("");
	const [query, setQuery] = useState("");
	const [role, setRole] = useState<AdminRole>("all");
	const [status, setStatus] = useState<AdminStatus>("all");
	const [page, setPage] = useState(1);

	const [loading, setLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [items, setItems] = useState<AdminUserItem[]>([]);
	const [total, setTotal] = useState(0);
	const [actionBusyUserId, setActionBusyUserId] = useState<number | null>(null);

	const totalPages = useMemo(
		() => Math.max(1, Math.ceil(total / PAGE_SIZE)),
		[total],
	);

	const loadUsers = useCallback(async () => {
		setLoading(true);
		setError(null);
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
		} catch (err) {
			if (err instanceof ApiError && err.code === "forbidden_admin_only") {
				setError("你当前没有管理员权限。请刷新页面确认账号状态。");
				setItems([]);
				setTotal(0);
				return;
			}
			setError(err instanceof Error ? err.message : String(err));
		} finally {
			setLoading(false);
		}
	}, [page, query, role, status]);

	useEffect(() => {
		void loadUsers();
	}, [loadUsers]);

	const onApplyFilters = useCallback(() => {
		setPage(1);
		setQuery(queryInput.trim());
	}, [queryInput]);

	const onToggleAdmin = useCallback(
		async (user: AdminUserItem) => {
			setActionBusyUserId(user.id);
			setError(null);
			try {
				await apiPatchJson<AdminUserItem>(`/api/admin/users/${user.id}`, {
					is_admin: !user.is_admin,
				});
				await loadUsers();
			} catch (err) {
				setError(err instanceof Error ? err.message : String(err));
			} finally {
				setActionBusyUserId(null);
			}
		},
		[loadUsers],
	);

	const onToggleDisabled = useCallback(
		async (user: AdminUserItem) => {
			setActionBusyUserId(user.id);
			setError(null);
			try {
				await apiPatchJson<AdminUserItem>(`/api/admin/users/${user.id}`, {
					is_disabled: !user.is_disabled,
				});
				await loadUsers();
			} catch (err) {
				setError(err instanceof Error ? err.message : String(err));
			} finally {
				setActionBusyUserId(null);
			}
		},
		[loadUsers],
	);

	return (
		<Card>
			<CardHeader>
				<CardTitle>用户管理</CardTitle>
				<CardDescription>
					管理账号角色与状态：支持筛选、升降管理员、启用/禁用。
				</CardDescription>
			</CardHeader>
			<CardContent className="space-y-4">
				<div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_160px_160px_auto]">
					<input
						type="text"
						value={queryInput}
						onChange={(e) => setQueryInput(e.target.value)}
						onKeyDown={(e) => {
							if (e.key === "Enter") onApplyFilters();
						}}
						placeholder="搜索 login / name / email"
						className="bg-background h-9 rounded-md border px-3 text-sm outline-none"
					/>
					<select
						value={role}
						onChange={(e) => {
							setRole(e.target.value as AdminRole);
							setPage(1);
						}}
						className="bg-background h-9 rounded-md border px-2 text-sm outline-none"
					>
						<option value="all">角色：全部</option>
						<option value="admin">角色：管理员</option>
						<option value="user">角色：普通用户</option>
					</select>
					<select
						value={status}
						onChange={(e) => {
							setStatus(e.target.value as AdminStatus);
							setPage(1);
						}}
						className="bg-background h-9 rounded-md border px-2 text-sm outline-none"
					>
						<option value="all">状态：全部</option>
						<option value="enabled">状态：启用</option>
						<option value="disabled">状态：禁用</option>
					</select>
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
										<div className="mt-1 flex flex-wrap gap-1">
											<span className="bg-muted rounded px-2 py-0.5 font-mono text-[11px]">
												UID:{user.id}
											</span>
											<span
												className={
													user.is_admin
														? "rounded bg-emerald-100 px-2 py-0.5 text-[11px] text-emerald-800"
														: "bg-muted rounded px-2 py-0.5 text-[11px]"
												}
											>
												{user.is_admin ? "管理员" : "普通用户"}
											</span>
											<span
												className={
													user.is_disabled
														? "rounded bg-red-100 px-2 py-0.5 text-[11px] text-red-800"
														: "rounded bg-sky-100 px-2 py-0.5 text-[11px] text-sky-800"
												}
											>
												{user.is_disabled ? "已禁用" : "已启用"}
											</span>
										</div>
									</div>
									<div className="flex flex-wrap gap-2">
										<Button
											variant="outline"
											disabled={busy}
											onClick={() => void onToggleAdmin(user)}
										>
											{user.is_admin ? "撤销管理员" : "设为管理员"}
										</Button>
										<Button
											variant={user.is_disabled ? "secondary" : "destructive"}
											disabled={busy}
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
		</Card>
	);
}
