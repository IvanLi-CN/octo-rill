import { ArrowUpRight, Inbox } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

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

function formatIsoShort(iso: string | null) {
	if (!iso) return "";
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

function threadUrl(threadId: string) {
	return `https://github.com/notifications/thread/${threadId}`;
}

export function InboxQuickList(props: { notifications: NotificationItem[] }) {
	const { notifications } = props;
	const top = notifications.slice(0, 8);

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
				{top.length === 0 ? (
					<p className="text-muted-foreground text-sm">暂无通知</p>
				) : (
					<div className="space-y-3">
						{top.map((n) => (
							<a
								key={n.thread_id}
								className="group block rounded-lg border bg-background/40 px-3 py-2 transition-colors hover:bg-background"
								href={threadUrl(n.thread_id)}
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
										</div>
										<div className="text-muted-foreground mt-1 font-mono text-[11px]">
											{n.reason ? n.reason : "update"}
											{n.subject_type ? ` · ${n.subject_type}` : ""}
											{n.updated_at ? ` · ${formatIsoShort(n.updated_at)}` : ""}
										</div>
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
