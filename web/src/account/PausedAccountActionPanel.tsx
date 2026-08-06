import {
	AlertTriangle,
	ArrowRight,
	CheckCircle2,
	LoaderCircle,
	LogOut,
	Pause,
	Play,
	RefreshCw,
} from "lucide-react";

import { Button } from "@/components/ui/button";

export type PausedAccountActionState =
	| "idle"
	| "resuming"
	| "queued"
	| "syncing"
	| "succeeded"
	| "enqueue_failed"
	| "failed";

type PausedAccountActionPanelProps = {
	login: string;
	state: PausedAccountActionState;
	error: string | null;
	onResume: () => void;
	onHome: () => void;
	onLogout: () => void;
};

const STATE_COPY: Record<PausedAccountActionState, string> = {
	idle: "等待恢复",
	resuming: "正在恢复账号",
	queued: "访问同步已排队",
	syncing: "访问同步进行中",
	succeeded: "访问同步已完成",
	enqueue_failed: "账号已恢复，同步等待重试",
	failed: "访问同步失败",
};

export function PausedAccountActionPanel({
	login,
	state,
	error,
	onResume,
	onHome,
	onLogout,
}: PausedAccountActionPanelProps) {
	const isBusy =
		state === "resuming" || state === "queued" || state === "syncing";
	const isSuccess = state === "succeeded";
	const isEnqueueFailure = state === "enqueue_failed";

	return (
		<section
			className="w-full max-w-2xl rounded-xl border border-border/70 bg-card/95 p-5 shadow-sm sm:p-7"
			aria-labelledby="paused-account-title"
			data-paused-account-panel
		>
			<div className="flex items-start gap-4">
				<div className="flex size-11 shrink-0 items-center justify-center rounded-lg border border-amber-400/40 bg-amber-400/10 text-amber-700 dark:text-amber-300">
					<Pause aria-hidden="true" className="size-5" />
				</div>
				<div className="min-w-0 space-y-1">
					<h1
						id="paused-account-title"
						className="text-xl font-semibold tracking-tight sm:text-2xl"
					>
						账号已暂停
					</h1>
					<p className="text-muted-foreground text-sm leading-6">
						{login}{" "}
						长时间未活动，账号已暂时暂停。恢复后会重新同步访问权限，已有数据不会被删除。
					</p>
				</div>
			</div>

			<div className="mt-6 border-y border-border/60 py-4" aria-live="polite">
				<div className="flex items-center justify-between gap-4 text-sm">
					<span className="text-muted-foreground">当前状态</span>
					<span className="inline-flex items-center gap-2 font-medium">
						{isBusy ? <LoaderCircle className="size-4 animate-spin" /> : null}
						{isSuccess ? (
							<CheckCircle2 className="size-4 text-emerald-600" />
						) : null}
						{isEnqueueFailure || state === "failed" ? (
							<AlertTriangle className="size-4 text-amber-600" />
						) : null}
						{STATE_COPY[state]}
					</span>
				</div>
				{error ? (
					<p className="mt-2 text-sm text-destructive">{error}</p>
				) : null}
			</div>

			<div className="mt-5 flex flex-col gap-2 sm:flex-row sm:flex-wrap">
				{isSuccess ? (
					<Button type="button" className="sm:min-w-36" onClick={onHome}>
						进入首页
						<ArrowRight />
					</Button>
				) : (
					<>
						<Button
							type="button"
							className="sm:min-w-36"
							disabled={isBusy}
							onClick={onResume}
						>
							{isEnqueueFailure || state === "failed" ? (
								<RefreshCw />
							) : (
								<Play />
							)}
							{isEnqueueFailure || state === "failed" ? "重试同步" : "恢复账号"}
						</Button>
						<Button type="button" variant="outline" onClick={onHome}>
							返回首页
							<ArrowRight />
						</Button>
					</>
				)}
				<Button type="button" variant="ghost" onClick={onLogout}>
					<LogOut />
					退出登录
				</Button>
			</div>
		</section>
	);
}
