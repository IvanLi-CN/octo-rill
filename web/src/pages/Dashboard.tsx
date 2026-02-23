import { useCallback, useEffect, useMemo, useState } from "react";

import { apiGet, apiPost, apiPostJson } from "@/api";
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

	const [notifications, setNotifications] = useState<NotificationItem[]>([]);
	const [briefs, setBriefs] = useState<BriefItem[]>([]);

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
	}, [feed.loadInitial, refreshSidebar]);

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

	const onToggleReaction = useCallback(
		(item: FeedItem, content: ReactionContent) => {
			const key = itemKey(item);
			let shouldRun = false;
			setReactionBusyKeys((prev) => {
				if (prev.has(key)) return prev;
				shouldRun = true;
				const next = new Set(prev);
				next.add(key);
				return next;
			});
			if (!shouldRun) return;

			void apiPostJson<ToggleReleaseReactionResponse>(
				"/api/release/reactions/toggle",
				{
					release_id: item.id,
					content,
				},
			)
				.then((res) => {
					feed.applyReactions(item, res.reactions);
				})
				.catch((err) => {
					setBootError(err instanceof Error ? err.message : String(err));
				})
				.finally(() => {
					setReactionBusyKeys((prev) => {
						const next = new Set(prev);
						next.delete(key);
						return next;
					});
				});
		},
		[feed],
	);

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
		</AppShell>
	);
}
