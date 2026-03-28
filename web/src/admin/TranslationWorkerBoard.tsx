import type * as React from "react";
import { useEffect, useMemo, useState } from "react";

import type { AdminTranslationWorkerStatus } from "@/api";
import { Badge } from "@/components/ui/badge";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

function formatElapsedDuration(
	value: string | null | undefined,
	nowMs: number,
) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) {
		return "-";
	}
	const diffMs = Math.max(0, nowMs - parsed.getTime());
	const totalSeconds = Math.floor(diffMs / 1000);
	const hours = Math.floor(totalSeconds / 3600);
	const minutes = Math.floor((totalSeconds % 3600) / 60);
	const seconds = totalSeconds % 60;

	if (hours > 0) {
		return `${hours}h ${minutes}m`;
	}
	if (minutes > 0) {
		return `${minutes}m ${seconds}s`;
	}
	return `${seconds}s`;
}

function translationWorkerKindLabel(kind: string) {
	switch (kind) {
		case "general":
			return "通用";
		case "user_dedicated":
			return "用户专用";
		default:
			return kind || "-";
	}
}

function translationWorkerSlotLabel(workerSlot: number | null | undefined) {
	if (typeof workerSlot !== "number") return "-";
	if (workerSlot <= 0) return "历史";
	return `W${workerSlot}`;
}

function translationWorkerStatusLabel(status: string) {
	switch (status) {
		case "queued":
			return "排队中";
		case "running":
			return "运行中";
		case "completed":
			return "已完成";
		case "failed":
			return "失败";
		case "idle":
			return "idle";
		case "error":
			return "error";
		default:
			return status || "-";
	}
}

function translationWorkerStatusTone(status: string) {
	switch (status) {
		case "running":
			return {
				badgeClass:
					"border-sky-300 bg-sky-100/90 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100",
				dotClass: "bg-sky-500",
			};
		case "failed":
		case "error":
			return {
				badgeClass:
					"border-red-300 bg-red-100/90 text-red-900 dark:border-red-500/60 dark:bg-red-500/20 dark:text-red-100",
				dotClass: "bg-red-500",
			};
		case "completed":
			return {
				badgeClass:
					"border-emerald-300 bg-emerald-100/90 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100",
				dotClass: "bg-emerald-500",
			};
		default:
			return {
				badgeClass:
					"border-border bg-muted/60 text-foreground dark:border-border dark:bg-muted/50 dark:text-foreground",
				dotClass: "bg-muted-foreground",
			};
	}
}

function WorkerStatusBadge(props: { status: string }) {
	const { status } = props;
	const tone = translationWorkerStatusTone(status);
	return (
		<Badge
			variant="outline"
			className={`gap-1.5 border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tone.badgeClass}`}
		>
			<span className={`size-1.5 rounded-full ${tone.dotClass}`} />
			{translationWorkerStatusLabel(status)}
		</Badge>
	);
}

export function TranslationWorkerBoard(props: {
	workers: AdminTranslationWorkerStatus[];
	loading?: boolean;
	title?: string;
	description?: string;
	headerAction?: React.ReactNode;
	onWorkerClick?: (worker: AdminTranslationWorkerStatus) => void;
}) {
	const {
		workers,
		loading = false,
		title = "工作者板",
		description,
		headerAction,
		onWorkerClick,
	} = props;
	const [nowMs, setNowMs] = useState(() => Date.now());
	const resolvedDescription = useMemo(() => {
		if (description) return description;
		const generalWorkers = workers.filter(
			(worker) => worker.worker_kind === "general",
		).length;
		const dedicatedWorkers = workers.filter(
			(worker) => worker.worker_kind === "user_dedicated",
		).length;
		return `当前展示 ${generalWorkers} 个通用 worker 与 ${dedicatedWorkers} 个用户专用 worker 的实时槽位状态。`;
	}, [description, workers]);
	const runningWorkerClockKey = useMemo(
		() =>
			workers
				.filter((worker) => worker.status === "running")
				.map((worker) => `${worker.worker_id}:${worker.updated_at}`)
				.join("|"),
		[workers],
	);

	useEffect(() => {
		if (!runningWorkerClockKey) {
			return;
		}
		setNowMs(Date.now());
		const timer = window.setInterval(() => {
			setNowMs(Date.now());
		}, 1000);
		return () => window.clearInterval(timer);
	}, [runningWorkerClockKey]);

	return (
		<Card>
			<CardHeader className="flex flex-row items-start justify-between gap-4">
				<div className="space-y-1.5">
					<CardTitle>{title}</CardTitle>
					<CardDescription>{resolvedDescription}</CardDescription>
				</div>
				{headerAction ? <div className="shrink-0">{headerAction}</div> : null}
			</CardHeader>
			<CardContent className="space-y-3">
				{loading && workers.length === 0 ? (
					<p className="text-muted-foreground inline-flex items-center gap-2 text-sm">
						<span className="bg-primary size-2 animate-pulse rounded-full" />
						正在加载翻译调度状态...
					</p>
				) : null}
				{!loading && workers.length === 0 ? (
					<p className="text-muted-foreground text-sm">暂无工作者数据。</p>
				) : null}
				{workers.length > 0 ? (
					<div className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
						{workers.map((worker) => {
							const cardContent = (
								<>
									<div className="flex items-start justify-between gap-3">
										<div className="min-w-0">
											<p className="truncate whitespace-nowrap font-semibold text-sm">
												{translationWorkerSlotLabel(worker.worker_slot)} ·{" "}
												{translationWorkerKindLabel(worker.worker_kind)}
											</p>
										</div>
										<WorkerStatusBadge status={worker.status} />
									</div>
									<div className="mt-4">
										<p className="text-muted-foreground text-xs">已工作时长</p>
										<p className="mt-1 font-semibold text-lg">
											{worker.status === "running"
												? formatElapsedDuration(worker.updated_at, nowMs)
												: "-"}
										</p>
									</div>
								</>
							);

							if (onWorkerClick) {
								return (
									<button
										key={worker.worker_id}
										type="button"
										onClick={() => onWorkerClick(worker)}
										className="bg-card/70 hover:border-foreground/20 focus-visible:ring-ring/50 flex rounded-lg border p-4 text-left transition-colors focus-visible:ring-[3px] focus-visible:outline-none"
										aria-label={`打开 ${translationWorkerSlotLabel(worker.worker_slot)} · ${translationWorkerKindLabel(worker.worker_kind)} 详情`}
									>
										<div className="w-full">{cardContent}</div>
									</button>
								);
							}

							return (
								<div
									key={worker.worker_id}
									className="bg-card/70 rounded-lg border p-4"
								>
									{cardContent}
								</div>
							);
						})}
					</div>
				) : null}
			</CardContent>
		</Card>
	);
}
