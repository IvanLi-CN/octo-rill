import { useCallback, useEffect, useMemo, useState } from "react";

import { apiGet, apiPost } from "@/api";
import { Button } from "@/components/ui/button";
import { FeedList } from "@/feed/FeedList";
import type { FeedItem } from "@/feed/types";
import { useAutoTranslate } from "@/feed/useAutoTranslate";
import { useFeed } from "@/feed/useFeed";
import { InboxList } from "@/inbox/InboxList";
import { AppShell } from "@/layout/AppShell";
import { BriefListCard } from "@/sidebar/BriefListCard";
import {
	InboxQuickList,
	type NotificationItem,
} from "@/sidebar/InboxQuickList";
import { type BriefItem, ReleaseDailyCard } from "@/sidebar/ReleaseDailyCard";

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

export function Dashboard(props: { me: MeResponse }) {
	const { me } = props;

	const [bootError, setBootError] = useState<string | null>(null);
	const [busy, setBusy] = useState<string | null>(null);

	type Tab = "all" | "releases" | "briefs" | "inbox";
	const [tab, setTab] = useState<Tab>("all");

	const feed = useFeed();

	const [showOriginalByKey, setShowOriginalByKey] = useState<
		Record<string, boolean>
	>({});
	const [selectedBriefDate, setSelectedBriefDate] = useState<string | null>(
		null,
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

	return (
		<AppShell
			header={
				<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
					<div className="min-w-0">
						<div className="flex flex-wrap items-center gap-2">
							<h1 className="text-xl font-semibold tracking-tight">OctoRill</h1>
							<span className="text-muted-foreground font-mono text-xs">
								Loaded {feed.items.length} · {notifications.length} inbox ·{" "}
								{briefs.length} briefs
							</span>
						</div>
						<p className="text-muted-foreground mt-1 text-sm">
							Logged in as{" "}
							<span className="text-foreground font-medium">
								{me.user.login}
							</span>
							{aiDisabledHint ? " · AI 未配置，将只显示原文" : ""}
						</p>
					</div>

					<div className="flex flex-wrap gap-2">
						<Button
							variant="secondary"
							disabled={Boolean(busy)}
							onClick={() => void refreshAll()}
						>
							Refresh
						</Button>
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
						<Button asChild variant="ghost">
							<a href="/auth/logout">Logout</a>
						</Button>
					</div>
				</div>
			}
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
						onClick={() => setTab("all")}
					>
						全部
					</Button>
					<Button
						variant={tab === "releases" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("releases")}
					>
						Releases
					</Button>
					<Button
						variant={tab === "briefs" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("briefs")}
					>
						日报
					</Button>
					<Button
						variant={tab === "inbox" ? "default" : "outline"}
						size="sm"
						className="font-mono text-xs"
						onClick={() => setTab("inbox")}
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
		</AppShell>
	);
}
