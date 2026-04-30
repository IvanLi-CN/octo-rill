import { ArrowUpRight, Inbox } from "lucide-react";

import { Button } from "@/components/ui/button";
import { formatIsoShortLocal } from "@/lib/datetime";
import { cn } from "@/lib/utils";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { resolveNotificationHref } from "@/inbox/notificationLink";

export type NotificationItem = {
	thread_id: string;
	repo_full_name: string | null;
	subject_title: string | null;
	subject_type: string | null;
	reason: string | null;
	updated_at: string | null;
	unread: number;
	html_url: string | null;
};

export function InboxQuickList(props: {
	notifications: NotificationItem[];
	freshKeys?: Set<string>;
}) {
	const { notifications, freshKeys = new Set<string>() } = props;
	const top = notifications.slice(0, 8);

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader className="px-4 pb-3 pt-4 sm:px-6 sm:pb-4 sm:pt-6">
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
					<Button
						asChild
						variant="outline"
						size="sm"
						className="font-mono text-xs"
					>
						<a
							href="https://github.com/notifications"
							target="_blank"
							rel="noreferrer"
						>
							<ArrowUpRight className="size-4" />
							GitHub
						</a>
					</Button>
				</div>
			</CardHeader>

			<CardContent className="px-4 pb-4 pt-0 sm:px-6 sm:pb-6">
				{top.length === 0 ? (
					<p className="text-muted-foreground text-sm">暂无通知</p>
				) : (
					<div className="space-y-2.5 sm:space-y-3">
						{top.map((n) => {
							const isFresh = freshKeys.has(`notification:${n.thread_id}`);
							return (
								<a
									key={n.thread_id}
									className={cn(
										"group block rounded-lg border bg-background/40 px-3 py-2 transition-[background-color,border-color,box-shadow] duration-200 hover:bg-background",
										isFresh && "dashboard-fresh-surface hover:bg-background/70",
									)}
									data-inbox-item-fresh={isFresh ? "true" : "false"}
									href={resolveNotificationHref(n)}
									target="_blank"
									rel="noreferrer"
								>
									<div className="flex items-center justify-between gap-2">
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
												{isFresh ? (
													<span
														className="dashboard-fresh-cue ml-2 inline-flex size-2 rounded-full align-middle"
														title="刚刚同步"
													>
														<span className="sr-only">刚刚同步</span>
													</span>
												) : null}
											</div>
											<div className="text-muted-foreground mt-1 font-mono text-[11px]">
												{n.reason ? n.reason : "update"}
												{n.subject_type ? ` · ${n.subject_type}` : ""}
												{n.updated_at
													? ` · ${formatIsoShortLocal(n.updated_at)}`
													: ""}
											</div>
										</div>
									</div>
								</a>
							);
						})}
					</div>
				)}
			</CardContent>
		</Card>
	);
}
