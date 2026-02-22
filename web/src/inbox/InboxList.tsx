import { ArrowUpRight, Inbox } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import type { NotificationItem } from "@/sidebar/InboxQuickList";

function formatIsoShort(iso: string | null) {
	if (!iso) return "";
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

function threadUrl(threadId: string) {
	return `https://github.com/notifications/thread/${threadId}`;
}

export function InboxList(props: { notifications: NotificationItem[] }) {
	const { notifications } = props;

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

			<CardContent className="pt-0">
				{notifications.length === 0 ? (
					<p className="text-muted-foreground text-sm">
						暂无通知。请点击上方的 <span className="font-mono">Sync inbox</span>{" "}
						同步数据。
					</p>
				) : (
					<div className="space-y-3">
						{notifications.map((n) => (
							<a
								key={n.thread_id}
								className="group block rounded-lg border bg-background/40 px-3 py-2 transition-colors hover:bg-background"
								href={threadUrl(n.thread_id)}
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
										{n.updated_at ? ` · ${formatIsoShort(n.updated_at)}` : ""}
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
