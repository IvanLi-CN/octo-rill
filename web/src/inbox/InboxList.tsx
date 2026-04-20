import { ArrowUpRight, Inbox, RefreshCcw } from "lucide-react";

import { ErrorStatePanel } from "@/components/feedback/ErrorStatePanel";
import { Button } from "@/components/ui/button";
import { formatIsoShortLocal } from "@/lib/datetime";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { resolveNotificationHref } from "@/inbox/notificationLink";
import type { NotificationItem } from "@/sidebar/InboxQuickList";

export function InboxList(props: {
	notifications: NotificationItem[];
	loading?: boolean;
	busy?: boolean;
	syncing?: boolean;
	error?: string | null;
	onSync?: () => void;
	onRetry?: () => void;
}) {
	const {
		notifications,
		loading = false,
		busy = false,
		syncing = false,
		error = null,
		onSync,
		onRetry,
	} = props;
	const showSync = Boolean(onSync);

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<div className="flex items-start justify-between gap-3">
					<div>
						<CardTitle className="inline-flex items-center gap-2">
							<Inbox className="size-4" />
							Inbox
						</CardTitle>
						<CardDescription className="font-mono text-xs">
							{notifications.length} threads
						</CardDescription>
					</div>
					<div className="flex items-center gap-2">
						{showSync ? (
							<Button
								variant="outline"
								size="sm"
								className="h-8 w-8 px-0 font-mono text-xs sm:w-auto sm:px-3"
								disabled={busy}
								onClick={onSync}
							>
								<RefreshCcw
									className={syncing ? "size-4 animate-spin" : "size-4"}
								/>
								<span className="sr-only sm:not-sr-only">Sync inbox</span>
							</Button>
						) : null}
						<Button
							asChild
							variant="outline"
							size="sm"
							className="h-8 w-8 px-0 font-mono text-xs sm:w-auto sm:px-3"
						>
							<a
								href="https://github.com/notifications"
								target="_blank"
								rel="noreferrer"
							>
								<ArrowUpRight className="size-4" />
								<span className="sr-only sm:not-sr-only">GitHub</span>
							</a>
						</Button>
					</div>
				</div>
			</CardHeader>

			<CardContent className="pt-0">
				{error && notifications.length === 0 ? (
					<ErrorStatePanel
						title="Inbox 加载失败"
						summary={error}
						size="compact"
						actionLabel={onRetry ? "重试" : undefined}
						onAction={onRetry}
					/>
				) : loading && notifications.length === 0 ? (
					<p className="text-muted-foreground text-sm">正在加载收件箱…</p>
				) : notifications.length === 0 ? (
					showSync ? (
						<p className="text-muted-foreground text-sm">
							暂无通知。可以点击 <span className="font-mono">Sync inbox</span>{" "}
							拉取最新数据。
						</p>
					) : (
						<p className="text-muted-foreground text-sm">
							暂无通知。请点击顶部的 <span className="font-mono">同步</span>{" "}
							拉取最新数据。
						</p>
					)
				) : (
					<div className="space-y-3">
						{notifications.map((n) => (
							<a
								key={n.thread_id}
								className="group block rounded-lg border bg-background/40 px-3 py-2 transition-colors hover:bg-background"
								href={resolveNotificationHref(n)}
								target="_blank"
								rel="noreferrer"
							>
								<div className="min-w-0">
									<div className="flex items-center gap-2">
										{n.unread ? (
											<span className="bg-primary size-1.5 shrink-0 rounded-full" />
										) : (
											<span className="bg-muted size-1.5 shrink-0 rounded-full" />
										)}
										<span className="font-mono text-muted-foreground truncate text-[11px]">
											{n.repo_full_name ?? "(unknown repo)"}
										</span>
									</div>
									<div className="mt-1 line-clamp-2 text-sm font-medium">
										{n.subject_title ?? "(no title)"}
									</div>
									<div className="text-muted-foreground mt-1 font-mono text-[11px]">
										{n.reason ? n.reason : "update"}
										{n.subject_type ? ` · ${n.subject_type}` : ""}
										{n.updated_at
											? ` · ${formatIsoShortLocal(n.updated_at)}`
											: ""}
									</div>
								</div>
							</a>
						))}
					</div>
				)}
			</CardContent>
		</Card>
	);
}
