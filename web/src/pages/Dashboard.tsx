import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { ApiError, apiGet, apiPost, apiPostJson, apiPutJson } from "@/api";
import { Button } from "@/components/ui/button";
import { FeedList } from "@/feed/FeedList";
import type {
	FeedItem,
	ReactionContent,
	ToggleReleaseReactionResponse,
} from "@/feed/types";
import { useAutoTranslate } from "@/feed/useAutoTranslate";
import { useFeed } from "@/feed/useFeed";
import { InboxList } from "@/inbox/InboxList";
import { AppMetaFooter } from "@/layout/AppMetaFooter";
import { AppShell } from "@/layout/AppShell";
import { DashboardHeader } from "@/pages/DashboardHeader";
import { ReleaseDetailDrawer } from "@/releases/ReleaseDetailDrawer";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";

type Tab = "all" | "releases" | "briefs" | "inbox";

type MeResponse = {
	user: {
		id: number;
		github_user_id: number;
		login: string;
		name: string | null;
		avatar_url: string | null;
		email: string | null;
	};
};

type SyncStarredResult = { repos: number };
type SyncReleasesResult = { repos: number; releases: number };
type SyncNotificationsResult = { notifications: number; since: string | null };

type BriefGenerateResponse = {
	date: string;
	window_start: string | null;
	window_end: string | null;
	content_markdown: string;
};

type ReactionTokenStatusResponse = {
	configured: boolean;
	masked_token: string | null;
	check: {
		state: "idle" | "valid" | "invalid" | "error";
		message: string | null;
		checked_at: string | null;
	};
};

type ReactionTokenCheckResponse = {
	state: "valid" | "invalid";
	message: string;
};

function sortNotifications(items: NotificationItem[]) {
	return items.slice().sort((a, b) => {
		if (a.unread !== b.unread) return b.unread - a.unread;
		const at = a.updated_at ?? "";
		const bt = b.updated_at ?? "";
		return bt.localeCompare(at);
	});
}

function parseDashboardQuery() {
	const params = new URLSearchParams(window.location.search);
	const rawTab = params.get("tab");
	const tab: Tab =
		rawTab === "releases" || rawTab === "briefs" || rawTab === "inbox"
			? rawTab
			: "all";
	const rawRelease = params.get("release");
	const releaseId = rawRelease && /^\d+$/.test(rawRelease) ? rawRelease : null;
	return { tab, releaseId };
}

function itemKey(item: Pick<FeedItem, "kind" | "id">) {
	return `${item.kind}:${item.id}`;
}

export function Dashboard(props: { me: MeResponse }) {
	const { me } = props;

	const [bootError, setBootError] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);

	const [tab, setTab] = useState<Tab>(() => parseDashboardQuery().tab);
	const [activeReleaseId, setActiveReleaseId] = useState<string | null>(
		() => parseDashboardQuery().releaseId,
	);

	const feed = useFeed();

	const [showOriginalByKey, setShowOriginalByKey] = useState<
		Record<string, boolean>
	>({});
	const [selectedBriefDate, setSelectedBriefDate] = useState<string | null>(
		null,
	);
	const [reactionBusyKeys, setReactionBusyKeys] = useState<Set<string>>(
		() => new Set<string>(),
	);
	const reactionBusyKeysRef = useRef<Set<string>>(new Set<string>());
	const [reactionErrorByKey, setReactionErrorByKey] = useState<
		Record<string, string>
	>({});
	const [reactionTokenConfigured, setReactionTokenConfigured] =
		useState<boolean>(false);
	const [reactionTokenMasked, setReactionTokenMasked] = useState<string | null>(
		null,
	);
	const [patDialogOpen, setPatDialogOpen] = useState<boolean>(false);
	const [patInput, setPatInput] = useState<string>("");
	const [patCheckState, setPatCheckState] = useState<
		"idle" | "checking" | "valid" | "invalid"
	>("idle");
	const [patCheckMessage, setPatCheckMessage] = useState<string | null>(null);
	const [patSaving, setPatSaving] = useState<boolean>(false);
	const [pendingReaction, setPendingReaction] = useState<{
		item: FeedItem;
		content: ReactionContent;
	} | null>(null);
	const patCheckSeqRef = useRef(0);

	const [notifications, setNotifications] = useState<NotificationItem[]>([]);
	const [briefs, setBriefs] = useState<BriefItem[]>([]);

	const loadReactionTokenStatus = useCallback(async () => {
		const res = await apiGet<ReactionTokenStatusResponse>(
			"/api/reaction-token/status",
		);
		setReactionTokenConfigured(res.configured && res.check.state === "valid");
		setReactionTokenMasked(res.masked_token);
		if (!res.configured) {
			setPatCheckState("idle");
			setPatCheckMessage(null);
		}
	}, []);

	const refreshSidebar = useCallback(async () => {
		const [n, b] = await Promise.all([
			apiGet<NotificationItem[]>("/api/notifications"),
			apiGet<BriefItem[]>("/api/briefs"),
		]);
		setNotifications(sortNotifications(n));
		setBriefs(b);
		setSelectedBriefDate((prev) => {
			if (prev && b.some((x) => x.date === prev)) return prev;
			return b[0]?.date ?? null;
		});
	}, []);

	const refreshAll = useCallback(async () => {
		setBootError(null);
		await Promise.all([feed.refresh(), refreshSidebar()]);
	}, [feed, refreshSidebar]);

	const run = useCallback(async <T,>(label: string, fn: () => Promise<T>) => {
		setBusy(label);
		setBootError(null);
		try {
			return await fn();
		} catch (err) {
			setBootError(err instanceof Error ? err.message : String(err));
			throw err;
		} finally {
			setBusy(null);
		}
	}, []);

	const { register, translateNow, inFlightKeys } = useAutoTranslate({
		enabled: true,
		onTranslated: feed.applyTranslation,
	});

	useEffect(() => {
		void feed.loadInitial();
		void refreshSidebar().catch((err) => {
			setBootError(err instanceof Error ? err.message : String(err));
		});
		void loadReactionTokenStatus().catch((err) => {
			setBootError(err instanceof Error ? err.message : String(err));
		});
	}, [feed.loadInitial, loadReactionTokenStatus, refreshSidebar]);

	const onToggleOriginal = useCallback((key: string) => {
		setShowOriginalByKey((prev) => ({ ...prev, [key]: !prev[key] }));
	}, []);

	const onTranslateNow = useCallback(
		(item: FeedItem) => {
			void translateNow(item).catch((err) => {
				setBootError(err instanceof Error ? err.message : String(err));
			});
		},
		[translateNow],
	);

	const performReactionToggle = useCallback(
		(item: FeedItem, content: ReactionContent) => {
			const key = itemKey(item);
			if (reactionBusyKeysRef.current.has(key)) return;

			setReactionErrorByKey((prev) => {
				if (!(key in prev)) return prev;
				const next = { ...prev };
				delete next[key];
				return next;
			});
			const nextBusy = new Set(reactionBusyKeysRef.current);
			nextBusy.add(key);
			reactionBusyKeysRef.current = nextBusy;
			setReactionBusyKeys(nextBusy);

			void apiPostJson<ToggleReleaseReactionResponse>(
				"/api/release/reactions/toggle",
				{
					release_id: item.id,
					content,
				},
			)
				.then((res) => {
					feed.applyReactions(item, res.reactions);
					setReactionErrorByKey((prev) => {
						if (!(key in prev)) return prev;
						const next = { ...prev };
						delete next[key];
						return next;
					});
					setReactionTokenConfigured(true);
					setPatCheckState("valid");
					setPatCheckMessage("PAT 可用");
				})
				.catch((err) => {
					if (err instanceof ApiError) {
						if (err.code === "pat_required" || err.code === "pat_invalid") {
							setReactionTokenConfigured(false);
							setPendingReaction({ item, content });
							setPatDialogOpen(true);
							setPatCheckState(err.code === "pat_invalid" ? "invalid" : "idle");
							setPatCheckMessage(
								err.code === "pat_invalid"
									? "PAT 无效或已过期，请重新填写并校验。"
									: "需要先配置 PAT 才能使用站内反馈。",
							);
							return;
						}
					}

					const raw = err instanceof Error ? err.message : String(err);
					let message = raw;
					if (
						raw.includes("OAuth app restrictions") ||
						raw.includes(
							"organization has enabled OAuth App access restrictions",
						)
					) {
						message = "该仓库限制了站内反馈，请在 GitHub 页面操作。";
					}
					setReactionErrorByKey((prev) => ({ ...prev, [key]: message }));
				})
				.finally(() => {
					const nextBusy = new Set(reactionBusyKeysRef.current);
					nextBusy.delete(key);
					reactionBusyKeysRef.current = nextBusy;
					setReactionBusyKeys(nextBusy);
				});
		},
		[feed],
	);

	const onToggleReaction = useCallback(
		(item: FeedItem, content: ReactionContent) => {
			if (!reactionTokenConfigured) {
				setPendingReaction({ item, content });
				setPatDialogOpen(true);
				setPatCheckState("idle");
				setPatCheckMessage("需要先配置 PAT 才能使用站内反馈。");
				return;
			}
			performReactionToggle(item, content);
		},
		[performReactionToggle, reactionTokenConfigured],
	);

	useEffect(() => {
		if (!patDialogOpen) return;
		const token = patInput.trim();
		patCheckSeqRef.current += 1;
		const seq = patCheckSeqRef.current;
		if (!token) {
			setPatCheckState("idle");
			setPatCheckMessage(null);
			return;
		}

		setPatCheckState("checking");
		setPatCheckMessage("正在检查 PAT 可用性…");
		const timer = window.setTimeout(() => {
			void apiPostJson<ReactionTokenCheckResponse>(
				"/api/reaction-token/check",
				{
					token,
				},
			)
				.then((res) => {
					if (seq !== patCheckSeqRef.current) return;
					setPatCheckState(res.state);
					setPatCheckMessage(res.message);
				})
				.catch((err) => {
					if (seq !== patCheckSeqRef.current) return;
					const message = err instanceof Error ? err.message : String(err);
					setPatCheckState("invalid");
					setPatCheckMessage(message);
				});
		}, 800);

		return () => window.clearTimeout(timer);
	}, [patDialogOpen, patInput]);

	const onSavePat = useCallback(() => {
		if (patCheckState !== "valid") return;
		const token = patInput.trim();
		if (!token) return;

		setPatSaving(true);
		void apiPutJson<ReactionTokenStatusResponse>("/api/reaction-token", {
			token,
		})
			.then((res) => {
				setReactionTokenConfigured(res.configured);
				setReactionTokenMasked(res.masked_token);
				setPatDialogOpen(false);
				setPatInput("");
				setPatCheckState("idle");
				setPatCheckMessage(null);
				if (pendingReaction) {
					const next = pendingReaction;
					setPendingReaction(null);
					performReactionToggle(next.item, next.content);
				}
			})
			.catch((err) => {
				const message = err instanceof Error ? err.message : String(err);
				setPatCheckState("invalid");
				setPatCheckMessage(message);
			})
			.finally(() => {
				setPatSaving(false);
			});
	}, [patCheckState, patInput, pendingReaction, performReactionToggle]);

	const onGenerateBrief = useCallback(() => {
		void run("Generate brief", async () => {
			await apiPost<BriefGenerateResponse>("/api/briefs/generate");
			await refreshSidebar();
		});
	}, [refreshSidebar, run]);

	const onSyncStarred = useCallback(() => {
		void run("Sync starred", async () => {
			await apiPost<SyncStarredResult>("/api/sync/starred");
			await refreshAll();
		});
	}, [refreshAll, run]);

	const onSyncReleases = useCallback(() => {
		void run("Sync releases", async () => {
			await apiPost<SyncReleasesResult>("/api/sync/releases");
			await refreshAll();
		});
	}, [refreshAll, run]);

	const onSyncInbox = useCallback(() => {
		void run("Sync inbox", async () => {
			await apiPost<SyncNotificationsResult>("/api/sync/notifications");
			await refreshAll();
		});
	}, [refreshAll, run]);

	const onSyncAll = useCallback(() => {
		void run("Sync all", async () => {
			await apiPost<SyncStarredResult>("/api/sync/starred");
			await apiPost<SyncReleasesResult>("/api/sync/releases");
			await apiPost<SyncNotificationsResult>("/api/sync/notifications");
			await refreshAll();
		});
	}, [refreshAll, run]);

	const aiDisabledHint = useMemo(() => {
		const any = feed.items.find((it) => it.translated?.status === "disabled");
		return Boolean(any);
	}, [feed.items]);

	const onSelectTab = useCallback((nextTab: Tab) => {
		setTab(nextTab);
		if (nextTab !== "briefs") {
			setActiveReleaseId(null);
		}
	}, []);

	const onOpenReleaseDetail = useCallback((releaseId: string) => {
		setTab("briefs");
		setActiveReleaseId(releaseId);
	}, []);

	const onCloseReleaseDetail = useCallback(() => {
		setActiveReleaseId(null);
	}, []);

	useEffect(() => {
		const params = new URLSearchParams(window.location.search);
		if (tab === "all") {
			params.delete("tab");
		} else {
			params.set("tab", tab);
		}
		if (activeReleaseId) {
			params.set("release", activeReleaseId);
		} else {
			params.delete("release");
		}
		const query = params.toString();
		const nextUrl = `${window.location.pathname}${query ? `?${query}` : ""}`;
		if (nextUrl !== `${window.location.pathname}${window.location.search}`) {
			window.history.replaceState({}, "", nextUrl);
		}
	}, [tab, activeReleaseId]);

	return (
		<AppShell
			header={
				<DashboardHeader
					feedCount={feed.items.length}
					inboxCount={notifications.length}
					briefCount={briefs.length}
					login={me.user.login}
					aiDisabledHint={aiDisabledHint}
					busy={Boolean(busy)}
					onRefresh={() => void refreshAll()}
					onSyncAll={onSyncAll}
					onSyncStarred={onSyncStarred}
					onSyncReleases={onSyncReleases}
					onSyncInbox={onSyncInbox}
				/>
			}
			footer={<AppMetaFooter />}
		>
			{bootError ? (
				<p className="text-destructive mb-4 text-sm">{bootError}</p>
			) : null}

			<div className="mb-4 flex flex-wrap items-center justify-between gap-2">
				<div className="flex flex-wrap items-center gap-2">
					<Button
						variant={tab === "all" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => onSelectTab("all")}
					>
						全部
					</Button>
					<Button
						variant={tab === "releases" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => onSelectTab("releases")}
					>
						Releases
					</Button>
					<Button
						variant={tab === "briefs" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => onSelectTab("briefs")}
					>
						日报
					</Button>
					<Button
						variant={tab === "inbox" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => onSelectTab("inbox")}
					>
						Inbox
					</Button>
				</div>

				{busy ? (
					<span className="text-muted-foreground font-mono text-xs">
						{busy}…
					</span>
				) : null}
			</div>

			<div className="grid gap-6 md:grid-cols-[minmax(0,1fr)_360px]">
				<section className="min-w-0">
					{tab === "all" || tab === "releases" ? (
						<>
							{!feed.loadingInitial && feed.items.length === 0 ? (
								<div className="bg-card/70 mb-4 rounded-xl border p-6 shadow-sm">
									<h2 className="text-base font-semibold tracking-tight">
										还没有内容
									</h2>
									<p className="text-muted-foreground mt-1 text-sm">
										先同步 starred，再同步 releases；右侧是 Inbox 快捷入口。
										或者直接点 <span className="font-mono">Sync all</span>。
									</p>
									<div className="mt-4 flex flex-wrap gap-2">
										<Button disabled={Boolean(busy)} onClick={onSyncAll}>
											Sync all
										</Button>
										<Button
											variant="outline"
											disabled={Boolean(busy)}
											onClick={onSyncStarred}
										>
											Sync starred
										</Button>
										<Button
											variant="outline"
											disabled={Boolean(busy)}
											onClick={onSyncReleases}
										>
											Sync releases
										</Button>
										<Button
											variant="outline"
											disabled={Boolean(busy)}
											onClick={onSyncInbox}
										>
											Sync inbox
										</Button>
									</div>
								</div>
							) : null}

							<FeedList
								items={feed.items}
								error={feed.error}
								loadingInitial={feed.loadingInitial}
								loadingMore={feed.loadingMore}
								hasMore={feed.hasMore}
								inFlightKeys={inFlightKeys}
								registerItemRef={register}
								onLoadMore={feed.loadMore}
								showOriginalByKey={showOriginalByKey}
								onToggleOriginal={onToggleOriginal}
								onTranslateNow={onTranslateNow}
								reactionBusyKeys={reactionBusyKeys}
								reactionErrorByKey={reactionErrorByKey}
								onToggleReaction={onToggleReaction}
								onSyncReleases={onSyncReleases}
							/>
						</>
					) : null}

					{tab === "briefs" ? (
						<div className="space-y-6">
							<ReleaseDailyCard
								briefs={briefs}
								selectedDate={selectedBriefDate}
								busy={busy === "Generate brief"}
								onGenerate={onGenerateBrief}
								onOpenRelease={onOpenReleaseDetail}
							/>
						</div>
					) : null}

					{tab === "inbox" ? <InboxList notifications={notifications} /> : null}
				</section>

				<aside className="space-y-6">
					{tab === "briefs" ? (
						<BriefListCard
							briefs={briefs}
							selectedDate={selectedBriefDate}
							onSelectDate={(d) => setSelectedBriefDate(d)}
						/>
					) : null}
					<InboxQuickList notifications={notifications} />
				</aside>
			</div>

			<ReleaseDetailDrawer
				releaseId={activeReleaseId}
				onClose={onCloseReleaseDetail}
			/>
			{patDialogOpen ? (
				<div className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4">
					<div className="bg-card w-full max-w-2xl rounded-xl border p-5 shadow-2xl">
						<h2 className="text-lg font-semibold tracking-tight">
							配置 GitHub PAT 以启用反馈表情
						</h2>
						<p className="text-muted-foreground mt-1 text-sm">
							当前 OAuth 登录仅用于读取与同步。站内点按反馈需要额外配置 PAT。
						</p>

						<div className="bg-muted/40 mt-4 rounded-lg border p-3">
							<p className="font-medium text-sm">创建路径（不限仓库口径）</p>
							<p className="text-muted-foreground mt-1 font-mono text-xs">
								Settings → Developer settings → Personal access tokens → Tokens
								(classic)
							</p>
							<p className="text-muted-foreground mt-2 text-xs">
								最小权限：公共仓库用{" "}
								<span className="font-mono">public_repo</span>
								；私有仓库用 <span className="font-mono">repo</span>。
							</p>
							{reactionTokenMasked ? (
								<p className="text-muted-foreground mt-2 text-xs">
									当前已保存：
									<span className="font-mono">{reactionTokenMasked}</span>
								</p>
							) : null}
						</div>

						<label
							htmlFor="reaction-pat"
							className="mt-4 block font-medium text-sm"
						>
							GitHub PAT
						</label>
						<input
							id="reaction-pat"
							type="password"
							value={patInput}
							onChange={(e) => setPatInput(e.target.value)}
							placeholder="粘贴 PAT 后将自动校验（800ms 防抖）"
							className="bg-background mt-1 w-full rounded-md border px-3 py-2 font-mono text-sm outline-none"
						/>

						<p
							className={
								patCheckState === "valid"
									? "mt-2 text-xs text-emerald-600"
									: patCheckState === "invalid"
										? "mt-2 text-xs text-red-600"
										: "text-muted-foreground mt-2 text-xs"
							}
						>
							{patCheckMessage ??
								"输入后会自动检查 PAT 是否可用；仅最后一次输入结果生效。"}
						</p>

						<div className="mt-5 flex items-center justify-end gap-2">
							<Button
								variant="outline"
								onClick={() => {
									setPatDialogOpen(false);
									setPatInput("");
									setPatCheckState("idle");
									setPatCheckMessage(null);
								}}
							>
								稍后再说
							</Button>
							<Button
								onClick={onSavePat}
								disabled={patSaving || patCheckState !== "valid"}
							>
								{patSaving ? "保存中…" : "保存并继续"}
							</Button>
						</div>
					</div>
				</div>
			) : null}
		</AppShell>
	);
}
