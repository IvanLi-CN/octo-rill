import { RefreshCcw } from "lucide-react";

import { Button } from "@/components/ui/button";

export type DashboardHeaderProps = {
	feedCount: number;
	inboxCount: number;
	briefCount: number;
	login: string;
	isAdmin: boolean;
	aiDisabledHint?: boolean;
	busy?: boolean;
	syncingAll?: boolean;
	onSyncAll?: () => void;
	logoutHref?: string;
};

export function DashboardHeader({
	feedCount,
	inboxCount,
	briefCount,
	login,
	isAdmin,
	aiDisabledHint = false,
	busy = false,
	syncingAll = false,
	onSyncAll,
	logoutHref = "/auth/logout",
}: DashboardHeaderProps) {
	return (
		<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
			<div className="min-w-0">
				<div className="flex flex-wrap items-center gap-2">
					<h1 className="text-xl font-semibold tracking-tight">OctoRill</h1>
					<span className="text-muted-foreground font-mono text-xs">
						Loaded {feedCount} · {inboxCount} inbox · {briefCount} briefs
					</span>
				</div>
				<p className="text-muted-foreground mt-1 text-sm">
					Logged in as{" "}
					<span className="text-foreground font-medium">{login}</span>
					{isAdmin ? " · Admin" : ""}
					{aiDisabledHint ? " · AI 未配置，将只显示原文" : ""}
				</p>
			</div>

			<div className="flex flex-wrap gap-2">
				<Button disabled={busy} onClick={onSyncAll}>
					<RefreshCcw
						className={syncingAll ? "size-4 animate-spin" : "size-4"}
					/>
					同步
				</Button>
				<Button asChild variant="ghost">
					<a href={logoutHref}>Logout</a>
				</Button>
			</div>
		</div>
	);
}
