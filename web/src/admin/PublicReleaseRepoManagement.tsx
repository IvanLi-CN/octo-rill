import { Trash2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";

import {
	ApiError,
	type AdminPublicReleaseCacheCleanup,
	type AdminPublicReleaseRepoItem,
	apiDeleteAdminPublicReleaseRepo,
	apiGetAdminPublicReleaseRepos,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	AlertDialog,
	AlertDialogAction,
	AlertDialogCancel,
	AlertDialogContent,
	AlertDialogDescription,
	AlertDialogFooter,
	AlertDialogHeader,
	AlertDialogTitle,
	AlertDialogTrigger,
} from "@/components/ui/alert-dialog";
import {
	ListBlockingErrorState,
	ListEmptyState,
	ListInlineError,
	ListRefreshingNotice,
	ListSurfaceShell,
} from "@/components/feedback/listSurface";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { useListSurfaceState } from "@/hooks/useListSurfaceState";

const NUMBER_FORMATTER = new Intl.NumberFormat();
const DATE_FORMATTER = new Intl.DateTimeFormat(undefined, {
	month: "2-digit",
	day: "2-digit",
	hour: "2-digit",
	minute: "2-digit",
});

function formatCount(value: number) {
	return NUMBER_FORMATTER.format(value);
}

function formatDate(value: string | null) {
	if (!value) return "-";
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return "-";
	return DATE_FORMATTER.format(date);
}

function statusLabel(status: AdminPublicReleaseRepoItem["last_sync_status"]) {
	switch (status) {
		case "ready":
			return "已同步";
		case "pending":
			return "等待同步";
		case "failed":
			return "同步失败";
		case "inaccessible":
			return "不可访问";
		default:
			return status;
	}
}

export function PublicReleaseRepoManagement() {
	const [items, setItems] = useState<AdminPublicReleaseRepoItem[]>([]);
	const [query, setQuery] = useState("");
	const [loading, setLoading] = useState(true);
	const [error, setError] = useState<string | null>(null);
	const [notice, setNotice] = useState<string | null>(null);
	const [deletingId, setDeletingId] = useState<string | null>(null);
	const [confirmingId, setConfirmingId] = useState<string | null>(null);

	const load = useCallback(async () => {
		setLoading(true);
		setError(null);
		try {
			const params = new URLSearchParams();
			params.set("page", "1");
			params.set("page_size", "100");
			if (query.trim()) params.set("query", query.trim());
			const response = await apiGetAdminPublicReleaseRepos(params);
			setItems(response.items);
		} catch (err) {
			setError(
				err instanceof ApiError ? err.message : "公开仓库登记列表加载失败",
			);
		} finally {
			setLoading(false);
		}
	}, [query]);

	useEffect(() => {
		void load();
	}, [load]);

	const listSurface = useListSurfaceState({
		loading,
		hasData: items.length > 0,
		hasError: error !== null,
	});

	const totals = useMemo(
		() => ({
			repos: items.length,
			releases: items.reduce((sum, item) => sum + item.release_count, 0),
			requests: items.reduce(
				(sum, item) =>
					sum +
					item.api_list_requests +
					item.api_detail_requests +
					item.page_list_requests +
					item.page_detail_requests,
				0,
			),
		}),
		[items],
	);

	const deleteItem = async (item: AdminPublicReleaseRepoItem) => {
		setDeletingId(item.id);
		setConfirmingId(null);
		setError(null);
		setNotice(null);
		try {
			const response = await apiDeleteAdminPublicReleaseRepo(item.id);
			setItems(response.items);
			setNotice(cacheCleanupMessage(response.cache_cleanup));
		} catch (err) {
			setError(err instanceof ApiError ? err.message : "删除登记记录失败");
		} finally {
			setDeletingId(null);
		}
	};

	return (
		<Card>
			<CardHeader>
				<div className="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<CardTitle>公开端点仓库</CardTitle>
						<CardDescription>
							由公开 Release 页面或 API
							自动登记；删除登记后，若没有登录用户或其他公开端点继续使用该仓库，会同步清理共享
							Release 与 AI 缓存。
						</CardDescription>
					</div>
					<div className="grid grid-cols-3 gap-2 text-right text-sm">
						<div>
							<p className="font-semibold">{formatCount(totals.repos)}</p>
							<p className="text-muted-foreground">仓库</p>
						</div>
						<div>
							<p className="font-semibold">{formatCount(totals.releases)}</p>
							<p className="text-muted-foreground">Release</p>
						</div>
						<div>
							<p className="font-semibold">{formatCount(totals.requests)}</p>
							<p className="text-muted-foreground">访问</p>
						</div>
					</div>
				</div>
			</CardHeader>
			<CardContent className="space-y-3">
				<div className="flex flex-col gap-2 sm:flex-row">
					<Input
						value={query}
						onChange={(event) => setQuery(event.target.value)}
						placeholder="搜索 owner/repo"
						className="sm:max-w-xs"
					/>
					<Button type="button" variant="outline" onClick={() => void load()}>
						刷新
					</Button>
				</div>

				{notice ? (
					<p className="text-sm text-muted-foreground">{notice}</p>
				) : null}
				<ListSurfaceShell
					state={listSurface.state}
					refreshing={listSurface.showRefreshing}
					className="space-y-3"
				>
					{listSurface.showRefreshing ? (
						<ListRefreshingNotice label="登记仓库更新中..." />
					) : null}
					{error && items.length > 0 ? (
						<ListInlineError
							title="登记仓库刷新失败"
							summary={error}
							actionLabel="重试"
							onAction={() => void load()}
						/>
					) : null}

					{listSurface.state === "blocking-error" ? (
						<ListBlockingErrorState
							title="公开仓库登记列表加载失败"
							summary={error ?? "当前无法读取登记仓库列表。"}
							actionLabel="重试"
							onAction={() => void load()}
						/>
					) : listSurface.state === "empty" ? (
						<ListEmptyState
							title="还没有公开端点登记仓库"
							description="公开 Release 页面或 API 首次访问仓库后，这里会自动出现对应登记与共享缓存占用线索。"
						/>
					) : (
						<div className="overflow-x-auto rounded-md border">
							<Table data-list-table="public-release-repos">
								<TableHeader>
									<TableRow>
										<TableHead>仓库</TableHead>
										<TableHead>状态</TableHead>
										<TableHead className="text-right">访问</TableHead>
										<TableHead className="text-right">数据量</TableHead>
										<TableHead>最近访问</TableHead>
										<TableHead className="text-right">操作</TableHead>
									</TableRow>
								</TableHeader>
								<TableBody>
									{listSurface.state === "initial-loading"
										? PUBLIC_RELEASE_REPO_SKELETON_KEYS.map((key) => (
												<TableRow key={key}>
													<TableCell>
														<div className="min-w-48 space-y-2">
															<div className="bg-muted h-4 w-32 animate-pulse rounded-full" />
															<div className="bg-muted h-3 w-24 animate-pulse rounded-full" />
														</div>
													</TableCell>
													<TableCell>
														<div className="space-y-2">
															<div className="bg-muted h-5 w-20 animate-pulse rounded-full" />
															<div className="bg-muted h-3 w-28 animate-pulse rounded-full" />
														</div>
													</TableCell>
													<TableCell className="text-right">
														<div className="ml-auto space-y-2">
															<div className="bg-muted ml-auto h-4 w-16 animate-pulse rounded-full" />
															<div className="bg-muted ml-auto h-3 w-16 animate-pulse rounded-full" />
														</div>
													</TableCell>
													<TableCell className="text-right">
														<div className="ml-auto space-y-2">
															<div className="bg-muted ml-auto h-4 w-18 animate-pulse rounded-full" />
															<div className="bg-muted ml-auto h-3 w-18 animate-pulse rounded-full" />
														</div>
													</TableCell>
													<TableCell>
														<div className="bg-muted h-4 w-20 animate-pulse rounded-full" />
													</TableCell>
													<TableCell className="text-right">
														<div className="bg-muted ml-auto h-9 w-9 animate-pulse rounded-xl" />
													</TableCell>
												</TableRow>
											))
										: items.map((item) => (
												<TableRow key={item.id}>
													<TableCell>
														<div className="min-w-48">
															<p className="font-medium">{item.full_name}</p>
															<p className="text-xs text-muted-foreground">
																登记 {formatDate(item.first_registered_at)}
															</p>
														</div>
													</TableCell>
													<TableCell>
														<Badge variant="secondary">
															{statusLabel(item.last_sync_status)}
														</Badge>
														{item.last_sync_error ? (
															<p className="mt-1 max-w-56 truncate text-xs text-muted-foreground">
																{item.last_sync_error}
															</p>
														) : null}
													</TableCell>
													<TableCell className="text-right text-sm">
														<p>
															API{" "}
															{formatCount(
																item.api_list_requests +
																	item.api_detail_requests,
															)}
														</p>
														<p className="text-muted-foreground">
															页面{" "}
															{formatCount(
																item.page_list_requests +
																	item.page_detail_requests,
															)}
														</p>
													</TableCell>
													<TableCell className="text-right text-sm">
														<p>{formatCount(item.release_count)} releases</p>
														<p className="text-muted-foreground">
															翻译 {formatCount(item.translated_ready_count)} /
															润色 {formatCount(item.polished_ready_count)}
														</p>
													</TableCell>
													<TableCell className="text-sm">
														{formatDate(item.last_requested_at)}
													</TableCell>
													<TableCell className="text-right">
														<AlertDialog
															open={confirmingId === item.id}
															onOpenChange={(open) =>
																setConfirmingId(open ? item.id : null)
															}
														>
															<AlertDialogTrigger asChild>
																<Button
																	type="button"
																	variant="ghost"
																	size="icon"
																	disabled={deletingId === item.id}
																	aria-label={`删除 ${item.full_name} 登记记录`}
																>
																	<Trash2 className="size-4" />
																</Button>
															</AlertDialogTrigger>
															<AlertDialogContent>
																<AlertDialogHeader>
																	<AlertDialogTitle>
																		删除 {item.full_name} 的公开登记？
																	</AlertDialogTitle>
																	<AlertDialogDescription>
																		会移除公开端点登记与访问统计。删除后如果没有登录用户视图或其他公开端点继续使用该仓库，系统会同时清理共享
																		Release 与 AI 缓存。
																	</AlertDialogDescription>
																</AlertDialogHeader>
																<AlertDialogFooter>
																	<AlertDialogCancel>取消</AlertDialogCancel>
																	<AlertDialogAction
																		variant="destructive"
																		onClick={() => void deleteItem(item)}
																	>
																		删除登记
																	</AlertDialogAction>
																</AlertDialogFooter>
															</AlertDialogContent>
														</AlertDialog>
													</TableCell>
												</TableRow>
											))}
								</TableBody>
							</Table>
						</div>
					)}
				</ListSurfaceShell>
			</CardContent>
		</Card>
	);
}

const PUBLIC_RELEASE_REPO_SKELETON_KEYS = [
	"public-release-repo-skeleton-1",
	"public-release-repo-skeleton-2",
	"public-release-repo-skeleton-3",
	"public-release-repo-skeleton-4",
] as const;

function cacheCleanupMessage(
	cleanup: AdminPublicReleaseCacheCleanup | null | undefined,
) {
	if (!cleanup) return "登记记录已删除。";
	if (cleanup.skipped_reason === "repo_not_resolved") {
		return `已删除 ${cleanup.full_name} 的登记记录；仓库尚未解析，无共享缓存需要清理。`;
	}
	if (cleanup.skipped_reason === "still_used_by_public_endpoint") {
		return `已删除 ${cleanup.full_name} 的登记记录；仍有其他公开端点登记使用该仓库，缓存已保留。`;
	}
	if (cleanup.skipped_reason === "still_used_by_user_release_visibility") {
		return `已删除 ${cleanup.full_name} 的登记记录；仍有登录用户视图使用该仓库，缓存已保留。`;
	}
	if (
		cleanup.deleted_release_count === 0 &&
		cleanup.deleted_ai_cache_count === 0
	) {
		return `已删除 ${cleanup.full_name} 的登记记录；没有可清理的共享缓存。`;
	}
	return `已删除 ${cleanup.full_name} 的登记记录，并清理 ${formatCount(
		cleanup.deleted_release_count,
	)} 条 Release 缓存、${formatCount(cleanup.deleted_ai_cache_count)} 条 AI 缓存。`;
}
