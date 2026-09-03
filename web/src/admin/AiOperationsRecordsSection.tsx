import { ArrowLeft, ChevronRight, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { AiRecordDetailRoute } from "@/admin/jobsRouteState";
import {
	type AdminCollectionAttempt,
	type AdminCollectionRecordDetail,
	type AdminCollectionRecordItem,
	type AdminCollectionTaskSummary,
	type AdminLlmCallDetailResponse,
	apiGetAdminCollectionRecordDetail,
	apiGetAdminCollectionRecords,
	apiGetAdminLlmCallDetail,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import { Tabs, TabsList, TabsTrigger } from "@/components/ui/tabs";

type CollectionTab = AdminCollectionRecordItem["kind"];
type TimeRangePreset = "24h" | "7d" | "30d" | "custom";
const PAGE_SIZE = 20;

function recordNow() {
	return __OCTO_RILL_DEMO_APP__
		? new Date("2026-07-08T10:30:00+08:00")
		: new Date();
}

function initialRange() {
	const end = recordNow();
	return {
		from: new Date(end.getTime() - 24 * 60 * 60 * 1000).toISOString(),
		before: end.toISOString(),
	};
}

function formatDateTime(value: string | null | undefined, missing = "-") {
	if (!value) return missing;
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return missing;
	return new Intl.DateTimeFormat("zh-CN", {
		month: "2-digit",
		day: "2-digit",
		hour: "2-digit",
		minute: "2-digit",
		hour12: false,
	}).format(date);
}

function toLocalInput(value: string) {
	const date = new Date(value);
	if (Number.isNaN(date.getTime())) return "";
	const offset = date.getTimezoneOffset() * 60_000;
	return new Date(date.getTime() - offset).toISOString().slice(0, 16);
}

function localInputToIso(value: string) {
	if (!value) return "";
	const date = new Date(value);
	return Number.isNaN(date.getTime()) ? "" : date.toISOString();
}

function useCompactLayout() {
	const [compact, setCompact] = useState(
		() =>
			typeof window !== "undefined" &&
			window.matchMedia("(max-width: 767px)").matches,
	);
	useEffect(() => {
		const media = window.matchMedia("(max-width: 767px)");
		const update = () => setCompact(media.matches);
		update();
		media.addEventListener("change", update);
		return () => media.removeEventListener("change", update);
	}, []);
	return compact;
}

function statusLabel(status: string) {
	switch (status) {
		case "queued":
			return "排队中";
		case "batched":
			return "已批处理";
		case "running":
			return "进行中";
		case "completed":
		case "succeeded":
		case "ready":
			return "已完成";
		case "failed":
		case "error":
			return "失败";
		case "missing":
			return "缺少结果";
		case "disabled":
			return "已停用";
		case "retry_scheduled":
			return "已安排重试";
		case "not_recorded":
			return "历史未记录";
		default:
			return status || "未记录";
	}
}

function statusTone(status: string) {
	switch (status) {
		case "failed":
		case "error":
			return "border-red-300 bg-red-100/90 text-red-900 dark:border-red-500/60 dark:bg-red-500/20 dark:text-red-100";
		case "running":
			return "border-sky-300 bg-sky-100/90 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100";
		case "queued":
		case "batched":
		case "retry_scheduled":
			return "border-amber-300 bg-amber-100/90 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100";
		case "completed":
		case "succeeded":
		case "ready":
			return "border-emerald-300 bg-emerald-100/90 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100";
		default:
			return "border-border bg-muted/60 text-foreground";
	}
}

function RecordStatus({ status }: { status: string }) {
	return (
		<Badge className={statusTone(status)} variant="outline">
			{statusLabel(status)}
		</Badge>
	);
}

function TaskSummaryHeader({ label }: { label: string }) {
	return (
		<TableHead className="whitespace-normal">
			<div className="space-y-0.5">
				<span className="block">{label}</span>
				<span className="text-muted-foreground block font-mono text-xs font-medium">
					状态 · 重试 / 开始 · 上次 · 完成
				</span>
			</div>
		</TableHead>
	);
}

function TaskSummaryCell({ summary }: { summary: AdminCollectionTaskSummary }) {
	return (
		<TableCell className="whitespace-normal">
			<div className="min-w-0 space-y-2">
				<div className="flex flex-wrap items-center gap-x-2 gap-y-1">
					<RecordStatus status={summary.status} />
					<span className="text-muted-foreground text-xs">
						{summary.retry_count} 次
					</span>
				</div>
				<div className="grid grid-cols-3 gap-2 text-xs tabular-nums">
					<span>{formatDateTime(summary.started_at, "未开始")}</span>
					<span>{formatDateTime(summary.last_attempt_at, "未尝试")}</span>
					<span>{formatDateTime(summary.finished_at, "未完成")}</span>
				</div>
			</div>
		</TableCell>
	);
}

function ItemHeading({
	item,
	showRepository = true,
}: {
	item: AdminCollectionRecordItem;
	showRepository?: boolean;
}) {
	return (
		<div className="min-w-0">
			<p className="max-w-60 truncate font-medium">{item.title}</p>
			{showRepository && item.repository ? (
				<p className="text-muted-foreground mt-1 truncate text-xs">
					{item.repository}
				</p>
			) : null}
		</div>
	);
}

function recordTime(item: AdminCollectionRecordItem) {
	return item.kind === "brief" ? item.generated_at : item.occurred_at;
}

function Paging({
	page,
	total,
	loading,
	onPage,
}: {
	page: number;
	total: number;
	loading: boolean;
	onPage: (page: number) => void;
}) {
	const totalPages = Math.max(1, Math.ceil(total / PAGE_SIZE));
	return (
		<div className="flex items-center justify-between gap-3 border-t pt-3">
			<p className="text-muted-foreground text-xs">
				共 {total} 条 · 第 {page}/{totalPages} 页
			</p>
			<div className="flex gap-2">
				<Button
					variant="outline"
					size="sm"
					disabled={loading || page <= 1}
					onClick={() => onPage(page - 1)}
				>
					上一页
				</Button>
				<Button
					variant="outline"
					size="sm"
					disabled={loading || page >= totalPages}
					onClick={() => onPage(page + 1)}
				>
					下一页
				</Button>
			</div>
		</div>
	);
}

function CollectionTable({
	items,
	tab,
	onOpen,
}: {
	items: AdminCollectionRecordItem[];
	tab: CollectionTab;
	onOpen: (item: AdminCollectionRecordItem) => void;
}) {
	const isBrief = tab === "brief";
	const titleHeading = isBrief
		? "日报日期"
		: tab === "release"
			? "Release 标题"
			: "公告标题";
	const sourceHeading = isBrief ? "生成时间" : "来源时间";
	return (
		<div className="hidden min-[1180px]:block">
			<Table
				containerClassName="overflow-x-hidden rounded-lg border"
				className="w-full table-fixed text-sm"
			>
				<colgroup>
					{!isBrief ? <col className="w-[14%]" /> : null}
					<col className={isBrief ? "w-[30%]" : "w-[19%]"} />
					<col className={isBrief ? "w-[24%]" : "w-[14%]"} />
					{!isBrief ? <col className="w-[24%]" /> : null}
					<col className={isBrief ? "w-[38%]" : "w-[24%]"} />
					<col className="w-14" />
				</colgroup>
				<TableHeader>
					<TableRow>
						{!isBrief ? <TableHead>仓库</TableHead> : null}
						<TableHead className="whitespace-normal">{titleHeading}</TableHead>
						<TableHead className="whitespace-normal">
							<div className="space-y-0.5">
								<span className="block">{sourceHeading}</span>
								{!isBrief ? (
									<span className="text-muted-foreground block font-mono text-xs font-medium">
										{tab === "release" ? "发布 · 发现" : "发生 · 发现"}
									</span>
								) : null}
							</div>
						</TableHead>
						{!isBrief ? <TaskSummaryHeader label="翻译" /> : null}
						<TaskSummaryHeader label="润色" />
						<TableHead className="text-right">
							<span className="sr-only">详情</span>
						</TableHead>
					</TableRow>
				</TableHeader>
				<TableBody>
					{items.map((item) => (
						<TableRow
							key={item.id}
							className="cursor-pointer focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
							tabIndex={0}
							onClick={() => onOpen(item)}
							onKeyDown={(event) => {
								if (event.key === "Enter" || event.key === " ") {
									event.preventDefault();
									onOpen(item);
								}
							}}
						>
							{!isBrief ? (
								<TableCell className="break-words text-xs whitespace-normal">
									{item.repository}
								</TableCell>
							) : null}
							<TableCell className="whitespace-normal">
								<ItemHeading item={item} showRepository={isBrief} />
							</TableCell>
							<TableCell className="whitespace-normal tabular-nums">
								<div className="space-y-1">
									<p>{formatDateTime(recordTime(item), "未记录")}</p>
									{!isBrief ? (
										<p className="text-muted-foreground">
											{formatDateTime(item.detected_at, "历史未记录")}
										</p>
									) : null}
								</div>
							</TableCell>
							{!isBrief && item.translation ? (
								<TaskSummaryCell summary={item.translation} />
							) : null}
							<TaskSummaryCell summary={item.polish} />
							<TableCell className="text-right">
								<Button
									variant="ghost"
									size="icon"
									className="size-9"
									aria-label={`查看 ${item.title} 详情`}
									onClick={(event) => {
										event.stopPropagation();
										onOpen(item);
									}}
								>
									<ChevronRight className="size-4" />
								</Button>
							</TableCell>
						</TableRow>
					))}
				</TableBody>
			</Table>
		</div>
	);
}

function CompactTask({
	label,
	summary,
}: {
	label: string;
	summary: AdminCollectionTaskSummary;
}) {
	return (
		<div className="border-t pt-3">
			<div className="flex items-center justify-between gap-2">
				<span className="text-muted-foreground text-xs">{label}</span>
				<RecordStatus status={summary.status} />
			</div>
			<p className="text-muted-foreground mt-2 text-xs">
				重试 {summary.retry_count} · 上次{" "}
				{formatDateTime(summary.last_attempt_at, "未尝试")}
			</p>
		</div>
	);
}

function CompactRecordList({
	items,
	tab,
	onOpen,
}: {
	items: AdminCollectionRecordItem[];
	tab: CollectionTab;
	onOpen: (item: AdminCollectionRecordItem) => void;
}) {
	return (
		<div className="space-y-3 min-[1180px]:hidden">
			{items.map((item) => (
				<button
					key={item.id}
					type="button"
					className="block w-full rounded-lg border p-4 text-left transition-colors hover:bg-muted/50 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
					onClick={() => onOpen(item)}
				>
					<div className="flex items-start justify-between gap-3">
						<ItemHeading item={item} />
						<ChevronRight className="mt-0.5 size-4 shrink-0 text-muted-foreground" />
					</div>
					<div className="text-muted-foreground mt-3 grid grid-cols-2 gap-x-3 gap-y-1.5 text-xs">
						<span>
							{tab === "brief" ? "生成" : tab === "release" ? "发布" : "发生"}{" "}
							{formatDateTime(recordTime(item), "未记录")}
						</span>
						{tab !== "brief" ? (
							<span>发现 {formatDateTime(item.detected_at, "历史未记录")}</span>
						) : null}
					</div>
					<div
						className={`mt-4 grid gap-3 ${tab === "brief" ? "grid-cols-1" : "grid-cols-2"}`}
					>
						{item.translation ? (
							<CompactTask label="翻译" summary={item.translation} />
						) : null}
						<CompactTask label="润色" summary={item.polish} />
					</div>
				</button>
			))}
		</div>
	);
}

function ProcessingSummary({
	label,
	summary,
}: {
	label: string;
	summary: AdminCollectionTaskSummary;
}) {
	return (
		<div className="space-y-2 border-t pt-3">
			<div className="flex items-center justify-between gap-3">
				<h3 className="font-medium text-sm">{label}</h3>
				<RecordStatus status={summary.status} />
			</div>
			<div className="text-muted-foreground grid grid-cols-2 gap-x-3 gap-y-2 text-xs">
				<span>重试 {summary.retry_count} 次</span>
				<span>开始 {formatDateTime(summary.started_at, "未开始")}</span>
				<span>
					上次尝试 {formatDateTime(summary.last_attempt_at, "未尝试")}
				</span>
				<span>完成 {formatDateTime(summary.finished_at, "未完成")}</span>
			</div>
		</div>
	);
}

function AttemptDetail({
	attempt,
	onOpenLlm,
}: {
	attempt: AdminCollectionAttempt;
	onOpenLlm: (id: string) => void;
}) {
	return (
		<div className="space-y-5">
			<div className="flex flex-wrap items-center gap-2">
				<span className="font-medium text-sm">
					{attempt.pipeline === "translation" ? "翻译" : "润色"} · 第{" "}
					{attempt.attempt_no} 次
				</span>
				<RecordStatus status={attempt.status} />
			</div>
			<div className="text-muted-foreground grid gap-2 border-y py-3 text-sm sm:grid-cols-2">
				<p>触发：{attempt.trigger}</p>
				<p>上次尝试：{formatDateTime(attempt.last_attempt_at)}</p>
				<p>开始：{formatDateTime(attempt.started_at, "未开始")}</p>
				<p>完成：{formatDateTime(attempt.finished_at, "未完成")}</p>
				{attempt.next_retry_at ? (
					<p className="text-amber-700 dark:text-amber-200">
						下次重试：{formatDateTime(attempt.next_retry_at)}
					</p>
				) : null}
			</div>
			{attempt.error_summary || attempt.error_code || attempt.failure_class ? (
				<div className="border border-red-500/35 bg-red-500/5 p-3 text-sm">
					<p className="text-destructive break-words">
						{attempt.error_summary ?? "处理失败"}
					</p>
					<p className="text-muted-foreground mt-1 break-all font-mono text-xs">
						{[attempt.error_code, attempt.failure_class]
							.filter(Boolean)
							.join(" · ")}
					</p>
				</div>
			) : null}
			<section className="space-y-3">
				<h3 className="font-semibold text-sm">底层模型调用</h3>
				{attempt.llm_calls.length === 0 ? (
					<p className="text-muted-foreground border-y py-4 text-sm">
						此尝试没有可关联的模型调用记录。
					</p>
				) : (
					<div className="divide-y border-y">
						{attempt.llm_calls.map((call) => (
							<button
								key={call.id}
								type="button"
								className="flex w-full items-center justify-between gap-3 py-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
								onClick={() => onOpenLlm(call.id)}
							>
								<span className="min-w-0">
									<span className="block truncate font-medium text-sm">
										{call.model}
									</span>
									<span className="text-muted-foreground block truncate text-xs">
										{call.source}
									</span>
								</span>
								<div className="flex items-center gap-2">
									<RecordStatus status={call.status} />
									<ChevronRight className="size-4 text-muted-foreground" />
								</div>
							</button>
						))}
					</div>
				)}
			</section>
		</div>
	);
}

function RecordDetail({
	detail,
	onOpenAttempt,
}: {
	detail: AdminCollectionRecordDetail;
	onOpenAttempt: (id: string) => void;
}) {
	const { record, attempts } = detail;
	return (
		<div className="space-y-5">
			<div className="space-y-1">
				<p className="font-semibold text-base">{record.title}</p>
				{record.repository ? (
					<p className="text-muted-foreground text-sm">{record.repository}</p>
				) : null}
				<p className="text-muted-foreground text-xs">
					{record.kind === "brief"
						? `生成：${formatDateTime(record.generated_at, "历史未记录")}`
						: `${record.kind === "release" ? "发布" : "发生"}：${formatDateTime(record.occurred_at, "未记录")} · 发现：${formatDateTime(record.detected_at, "历史未记录")}`}
				</p>
			</div>
			{record.translation ? (
				<ProcessingSummary label="翻译" summary={record.translation} />
			) : null}
			<ProcessingSummary label="润色" summary={record.polish} />
			<section className="space-y-3">
				<div className="flex items-center justify-between gap-3">
					<h3 className="font-semibold text-sm">尝试记录</h3>
					<span className="text-muted-foreground text-xs">
						{attempts.length} 次
					</span>
				</div>
				{attempts.length === 0 ? (
					<p className="text-muted-foreground border-y py-5 text-sm">
						{record.kind === "brief"
							? "这份日报未关联润色调用；历史记录未补造。"
							: "该记录没有已保留的处理尝试。"}
					</p>
				) : (
					<div className="divide-y border-y">
						{attempts.map((attempt) => {
							const models = [
								...new Set(attempt.llm_calls.map((call) => call.model)),
							];
							const error =
								attempt.error_summary ??
								[attempt.error_code, attempt.failure_class]
									.filter(Boolean)
									.join(" · ");
							return (
								<button
									key={attempt.id}
									type="button"
									aria-label={`查看${attempt.pipeline === "translation" ? "翻译" : "润色"}第 ${attempt.attempt_no} 次尝试详情`}
									className="flex w-full items-start justify-between gap-3 py-3 text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
									onClick={() => onOpenAttempt(attempt.id)}
								>
									<span className="min-w-0">
										<span className="block font-medium text-sm">
											{attempt.pipeline === "translation" ? "翻译" : "润色"} ·
											第 {attempt.attempt_no} 次
										</span>
										<span className="text-muted-foreground mt-1 block truncate text-xs">
											{models.join(" · ") || "未关联模型调用"}
										</span>
										<span className="text-muted-foreground mt-1 block text-xs">
											{attempt.trigger} ·{" "}
											{formatDateTime(attempt.last_attempt_at)}
										</span>
										{error ? (
											<span className="text-destructive mt-1 line-clamp-2 block text-xs">
												{error}
											</span>
										) : null}
									</span>
									<span className="flex shrink-0 items-center gap-2">
										<RecordStatus status={attempt.status} />
										<ChevronRight className="size-4 text-muted-foreground" />
									</span>
								</button>
							);
						})}
					</div>
				)}
			</section>
		</div>
	);
}

function LlmDetail({ detail }: { detail: AdminLlmCallDetailResponse }) {
	return (
		<div className="space-y-5">
			<div className="space-y-1">
				<p className="font-semibold text-base">{detail.model}</p>
				<div className="flex items-center gap-2">
					<RecordStatus status={detail.status} />
					<span className="text-muted-foreground text-xs">{detail.source}</span>
				</div>
			</div>
			<div className="text-muted-foreground grid gap-2 border-y py-3 text-sm sm:grid-cols-2">
				<p>开始：{formatDateTime(detail.started_at, "未开始")}</p>
				<p>上次尝试：{formatDateTime(detail.updated_at)}</p>
				<p>完成：{formatDateTime(detail.finished_at, "未完成")}</p>
				<p>调用尝试：{detail.attempt_count}</p>
			</div>
			{detail.error_text ? (
				<p className="border border-red-500/35 bg-red-500/5 p-3 text-destructive text-sm">
					{detail.error_text}
				</p>
			) : null}
			<section className="space-y-3">
				<h3 className="font-semibold text-sm">调用事件</h3>
				<div className="divide-y border-y">
					{detail.attempt_history.map((attempt, index) => (
						<div key={`${attempt.created_at}:${index}`} className="py-3">
							<div className="flex flex-wrap items-center gap-2">
								<RecordStatus status={attempt.status} />
								<span className="text-sm">{attempt.model ?? detail.model}</span>
							</div>
							<p className="text-muted-foreground mt-1 text-xs">
								{attempt.event_type} · {formatDateTime(attempt.created_at)}
							</p>
							{attempt.failure_class ? (
								<p className="text-destructive mt-1 text-xs">
									{attempt.failure_class}
								</p>
							) : null}
						</div>
					))}
				</div>
			</section>
		</div>
	);
}

export function AiOperationsRecordsSection({
	detailRoute,
	onOpenRecord,
	onOpenAttempt,
	onOpenLlm,
	onCloseRecord,
}: {
	detailRoute: AiRecordDetailRoute | null;
	onOpenRecord: (kind: CollectionTab, id: string) => void;
	onOpenAttempt: (attemptId: string) => void;
	onOpenLlm: (callId: string) => void;
	onCloseRecord: () => void;
}) {
	const compact = useCompactLayout();
	const [tab, setTab] = useState<CollectionTab>("release");
	const [preset, setPreset] = useState<TimeRangePreset>("24h");
	const [range, setRange] = useState(initialRange);
	const [page, setPage] = useState(1);
	const [items, setItems] = useState<AdminCollectionRecordItem[]>([]);
	const [total, setTotal] = useState(0);
	const [loading, setLoading] = useState(false);
	const [reloadNonce, setReloadNonce] = useState(0);
	const [error, setError] = useState<string | null>(null);
	const [detail, setDetail] = useState<AdminCollectionRecordDetail | null>(
		null,
	);
	const [llmDetail, setLlmDetail] = useState<AdminLlmCallDetailResponse | null>(
		null,
	);
	const [detailLoading, setDetailLoading] = useState(false);
	const [detailError, setDetailError] = useState<string | null>(null);
	const listRequestRef = useRef(0);
	const detailRequestRef = useRef(0);
	const selectedRange = useMemo(() => {
		if (preset === "custom") return range;
		const end = recordNow();
		const hours = preset === "7d" ? 7 * 24 : preset === "30d" ? 30 * 24 : 24;
		return {
			from: new Date(end.getTime() - hours * 60 * 60 * 1000).toISOString(),
			before: end.toISOString(),
		};
	}, [preset, range]);
	const setRangePreset = useCallback((next: TimeRangePreset) => {
		setPreset(next);
		if (next !== "custom") {
			const end = recordNow();
			const hours = next === "7d" ? 7 * 24 : next === "30d" ? 30 * 24 : 24;
			setRange({
				from: new Date(end.getTime() - hours * 60 * 60 * 1000).toISOString(),
				before: end.toISOString(),
			});
		}
		setPage(1);
	}, []);
	useEffect(() => {
		setPage(1);
	}, [tab, selectedRange.before, selectedRange.from]);
	useEffect(() => {
		const requestId = listRequestRef.current + 1;
		listRequestRef.current = requestId;
		setLoading(true);
		setError(null);
		const params = new URLSearchParams({
			page: String(page),
			page_size: String(PAGE_SIZE),
			from: selectedRange.from,
			before: selectedRange.before,
		});
		void apiGetAdminCollectionRecords(tab, params)
			.then((response) => {
				if (requestId !== listRequestRef.current) return;
				setItems(response.items);
				setTotal(response.total);
			})
			.catch((cause: unknown) => {
				if (requestId !== listRequestRef.current) return;
				setError(cause instanceof Error ? cause.message : "无法读取采集记录。");
			})
			.finally(() => {
				if (requestId === listRequestRef.current) setLoading(false);
			});
	}, [page, reloadNonce, selectedRange.before, selectedRange.from, tab]);
	useEffect(() => {
		if (!detailRoute) {
			setDetail(null);
			setLlmDetail(null);
			setDetailError(null);
			return;
		}
		const requestId = detailRequestRef.current + 1;
		detailRequestRef.current = requestId;
		setDetailLoading(true);
		setDetailError(null);
		void apiGetAdminCollectionRecordDetail(detailRoute.kind, detailRoute.id)
			.then(async (recordDetail) => {
				if (requestId !== detailRequestRef.current) return;
				setDetail(recordDetail);
				if (detailRoute.llmCallId) {
					const call = await apiGetAdminLlmCallDetail(detailRoute.llmCallId);
					if (requestId === detailRequestRef.current) setLlmDetail(call);
				} else {
					setLlmDetail(null);
				}
			})
			.catch((cause: unknown) => {
				if (requestId !== detailRequestRef.current) return;
				setDetailError(
					cause instanceof Error ? cause.message : "无法读取详情。",
				);
			})
			.finally(() => {
				if (requestId === detailRequestRef.current) setDetailLoading(false);
			});
	}, [detailRoute]);
	const selectedAttempt = detailRoute?.attemptId
		? (detail?.attempts.find(
				(attempt) => attempt.id === detailRoute.attemptId,
			) ?? null)
		: null;
	const detailContent = detailLoading ? (
		<p className="text-muted-foreground py-8 text-sm">正在加载详情...</p>
	) : detailError ? (
		<p className="text-destructive py-8 text-sm">{detailError}</p>
	) : detailRoute?.llmCallId && llmDetail ? (
		<LlmDetail detail={llmDetail} />
	) : selectedAttempt ? (
		<AttemptDetail attempt={selectedAttempt} onOpenLlm={onOpenLlm} />
	) : detail ? (
		<RecordDetail detail={detail} onOpenAttempt={onOpenAttempt} />
	) : null;
	const detailTitle = detailRoute?.llmCallId
		? "模型调用详情"
		: selectedAttempt
			? "尝试详情"
			: "记录详情";
	const closeOrBack = () => {
		if (detailRoute?.llmCallId) {
			onOpenAttempt(detailRoute.attemptId ?? "");
		} else if (selectedAttempt) {
			onOpenAttempt("");
		} else {
			onCloseRecord();
		}
	};
	if (compact && detailRoute)
		return (
			<section aria-label="采集记录详情" className="space-y-4">
				<div className="flex items-center gap-2 border-b pb-3">
					<Button
						variant="ghost"
						size="icon"
						className="size-11"
						onClick={closeOrBack}
						aria-label="返回"
					>
						<ArrowLeft className="size-5" />
					</Button>
					<div className="min-w-0">
						<h2 className="font-semibold text-base">{detailTitle}</h2>
						<p className="text-muted-foreground truncate text-xs">
							{detail?.record.title ?? "正在加载"}
						</p>
					</div>
				</div>
				{detailContent}
			</section>
		);
	return (
		<>
			<Card>
				<CardHeader className="gap-4">
					<div className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
						<CardTitle>采集与处理记录</CardTitle>
						<div className="flex items-center gap-2">
							<Select
								value={preset}
								onValueChange={(value) =>
									setRangePreset(value as TimeRangePreset)
								}
							>
								<SelectTrigger
									className="min-h-11 w-[138px]"
									aria-label="记录时间范围"
								>
									<SelectValue />
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="24h">最近 24 小时</SelectItem>
									<SelectItem value="7d">最近 7 天</SelectItem>
									<SelectItem value="30d">最近 30 天</SelectItem>
									<SelectItem value="custom">自定义</SelectItem>
								</SelectContent>
							</Select>
							<Button
								variant="outline"
								size="icon"
								className="size-11"
								onClick={() => setReloadNonce((current) => current + 1)}
								disabled={loading}
								aria-label="刷新记录"
							>
								<RefreshCw className="size-4" />
							</Button>
						</div>
					</div>
					{preset === "custom" ? (
						<div className="grid gap-2 sm:grid-cols-2">
							<Input
								aria-label="开始时间"
								className="min-h-11"
								type="datetime-local"
								value={toLocalInput(range.from)}
								onChange={(event) =>
									setRange((current) => ({
										...current,
										from: localInputToIso(event.target.value),
									}))
								}
							/>
							<Input
								aria-label="结束时间"
								className="min-h-11"
								type="datetime-local"
								value={toLocalInput(range.before)}
								onChange={(event) =>
									setRange((current) => ({
										...current,
										before: localInputToIso(event.target.value),
									}))
								}
							/>
						</div>
					) : null}
					<Tabs
						value={tab}
						onValueChange={(value) => setTab(value as CollectionTab)}
					>
						<TabsList className="grid w-full grid-cols-3">
							<TabsTrigger value="release">Release</TabsTrigger>
							<TabsTrigger value="announcement">公告</TabsTrigger>
							<TabsTrigger value="brief">日报</TabsTrigger>
						</TabsList>
					</Tabs>
				</CardHeader>
				<CardContent className="space-y-3">
					{error ? <p className="text-destructive text-sm">{error}</p> : null}
					{loading ? (
						<p className="text-muted-foreground py-8 text-sm">
							正在加载记录...
						</p>
					) : null}
					{!loading && !error && items.length === 0 ? (
						<p className="text-muted-foreground border-y py-8 text-sm">
							当前时间范围没有
							{tab === "brief" ? "生成的日报" : "发现的采集记录"}。
						</p>
					) : null}
					{!loading && !error && items.length > 0 ? (
						<>
							<CollectionTable
								items={items}
								tab={tab}
								onOpen={(item) => onOpenRecord(item.kind, item.id)}
							/>
							<CompactRecordList
								items={items}
								tab={tab}
								onOpen={(item) => onOpenRecord(item.kind, item.id)}
							/>
							<Paging
								page={page}
								total={total}
								loading={loading}
								onPage={setPage}
							/>
						</>
					) : null}
				</CardContent>
			</Card>
			<Sheet
				open={Boolean(detailRoute)}
				onOpenChange={(open) => {
					if (!open) onCloseRecord();
				}}
			>
				<SheetContent
					side="right"
					showCloseButton={false}
					className="w-full gap-0 overflow-y-auto p-0 sm:max-w-3xl"
				>
					<SheetHeader className="gap-3 border-b px-5 py-4 text-left">
						<div className="flex items-start justify-between gap-3">
							<div className="min-w-0">
								<SheetTitle className="text-lg">{detailTitle}</SheetTitle>
								<SheetDescription>
									数据来源、处理状态与可保留的尝试链。
								</SheetDescription>
							</div>
							<Button variant="outline" onClick={closeOrBack}>
								{selectedAttempt || detailRoute?.llmCallId ? "返回" : "关闭"}
							</Button>
						</div>
					</SheetHeader>
					<div className="px-5 py-4">{detailContent}</div>
				</SheetContent>
			</Sheet>
		</>
	);
}
