import { CircleHelp, Settings2 } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { TaskTypeDetailSection } from "@/admin/TaskTypeDetailSection";
import { TranslationWorkerBoard } from "@/admin/TranslationWorkerBoard";
import {
	ADMIN_JOBS_BASE_PATH,
	buildAdminJobsRouteUrl,
	parseAdminJobsRoute,
	type AdminJobsPrimaryTab,
	type AdminJobsRouteState,
	type TranslationViewTab,
} from "@/admin/jobsRouteState";
import {
	type AdminLlmCallDetailResponse,
	type AdminLlmCallItem,
	type AdminLlmCallStreamEvent,
	type AdminLlmSchedulerStatusResponse,
	type AdminJobsOverviewResponse,
	type AdminJobsStreamEvent,
	type AdminRealtimeTaskDetailResponse,
	type AdminRealtimeTaskItem,
	type AdminTaskEventItem,
	type AdminTranslationBatchDetailResponse,
	type AdminTranslationStreamEvent,
	type AdminTranslationBatchListItem,
	type AdminTranslationRequestDetailResponse,
	type AdminTranslationRequestListItem,
	type AdminTranslationStatusResponse,
	type LocalUserId,
	ApiError,
	apiCancelAdminRealtimeTask,
	apiGetAdminLlmCallDetail,
	apiGetAdminLlmCalls,
	apiGetAdminLlmSchedulerStatus,
	apiGetAdminJobsOverview,
	apiGetAdminRealtimeTaskDetail,
	apiGetAdminRealtimeTasks,
	apiGetAdminTranslationBatchDetail,
	apiGetAdminTranslationBatches,
	apiGetAdminTranslationRequestDetail,
	apiGetAdminTranslationRequests,
	apiGetAdminTranslationStatus,
	apiOpenAdminJobsEventsStream,
	apiPatchAdminLlmRuntimeConfig,
	apiPatchAdminTranslationRuntimeConfig,
	apiRetryAdminRealtimeTask,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogFooter,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
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
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
	Table,
	TableBody,
	TableCell,
	TableHead,
	TableHeader,
	TableRow,
} from "@/components/ui/table";
import {
	Tooltip,
	TooltipContent,
	TooltipTrigger,
} from "@/components/ui/tooltip";

const HM_FORMATTER = new Intl.DateTimeFormat(undefined, {
	hour: "2-digit",
	minute: "2-digit",
	hour12: false,
});

const DATETIME_FORMATTER = new Intl.DateTimeFormat(undefined, {
	month: "2-digit",
	day: "2-digit",
	hour: "2-digit",
	minute: "2-digit",
	second: "2-digit",
	hour12: false,
});
const NUMBER_FORMATTER = new Intl.NumberFormat();

function formatLocalHm(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) {
		return "-";
	}
	return HM_FORMATTER.format(parsed);
}

function formatLocalDateTime(value: string | null | undefined) {
	if (!value) return "-";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) {
		return "-";
	}
	return DATETIME_FORMATTER.format(parsed);
}

function formatCount(value: number | null | undefined) {
	if (typeof value !== "number") return "-";
	return NUMBER_FORMATTER.format(value);
}

function formatDurationMs(value: number | null | undefined) {
	if (typeof value !== "number") return "-";
	if (value < 1000) return `${value}ms`;
	return `${(value / 1000).toFixed(2)}s`;
}

function parsePositiveIntegerInput(value: string) {
	if (!/^\d+$/.test(value.trim())) {
		return null;
	}
	const parsed = Number(value);
	if (!Number.isSafeInteger(parsed) || parsed <= 0) {
		return null;
	}
	return parsed;
}

function formatModelInputLimitSource(source: string | null | undefined) {
	switch (source) {
		case "admin_override":
			return "后台覆盖";
		case "synced_catalog":
			return "同步模型目录";
		case "builtin_catalog":
			return "内置模型目录";
		case "unknown_fallback":
			return "默认兜底";
		default:
			return source ?? "-";
	}
}

function formatTranslationWorkerBoardDescription(
	generalWorkerConcurrency: number | null | undefined,
	dedicatedWorkerConcurrency: number | null | undefined,
) {
	if (
		typeof generalWorkerConcurrency !== "number" ||
		typeof dedicatedWorkerConcurrency !== "number"
	) {
		return "当前展示翻译工作者的实时槽位状态。";
	}
	return `目标配置为 ${generalWorkerConcurrency} 个通用 worker 与 ${dedicatedWorkerConcurrency} 个用户专用 worker；下方展示实时槽位状态。`;
}

function localInputToUtc(value: string) {
	if (!value) return "";
	const parsed = new Date(value);
	if (Number.isNaN(parsed.getTime())) return "";
	return parsed.toISOString();
}

type LlmConversationMessage = {
	role: string;
	content: string;
};
type LlmConversationTimelineItem = {
	key: string;
	turn: number;
	source: "input" | "output";
	role: string;
	content: string;
};

function normalizeLlmMessageContent(raw: unknown): string {
	if (typeof raw === "string") {
		return raw;
	}
	if (Array.isArray(raw)) {
		return raw
			.map((item) => {
				if (typeof item === "string") return item;
				if (item && typeof item === "object" && "text" in item) {
					const text = (item as { text?: unknown }).text;
					return typeof text === "string" ? text : "";
				}
				return "";
			})
			.filter(Boolean)
			.join("\n");
	}
	return "";
}

function parseLlmConversationMessages(
	raw: string | null | undefined,
): LlmConversationMessage[] {
	if (!raw) return [];
	try {
		const parsed = JSON.parse(raw) as unknown;
		if (!Array.isArray(parsed)) return [];
		return parsed
			.map((item) => {
				if (!item || typeof item !== "object") return null;
				const roleRaw = (item as { role?: unknown }).role;
				const contentRaw = (item as { content?: unknown }).content;
				const role = typeof roleRaw === "string" ? roleRaw : "unknown";
				const content = normalizeLlmMessageContent(contentRaw);
				if (!content) return null;
				return { role, content };
			})
			.filter((item): item is LlmConversationMessage => item !== null);
	} catch {
		return [];
	}
}

function llmRoleLabel(role: string) {
	switch (role) {
		case "system":
			return "系统";
		case "user":
			return "用户";
		case "assistant":
			return "助手";
		case "tool":
			return "工具";
		case "input":
			return "输入文本";
		default:
			return role;
	}
}

type LlmRoleTone = {
	containerClass: string;
	badgeClass: string;
};

function llmRoleTone(role: string, isAssistantOutput: boolean): LlmRoleTone {
	if (isAssistantOutput) {
		return {
			containerClass:
				"border-primary/35 bg-primary/5 dark:border-primary/40 dark:bg-primary/10",
			badgeClass:
				"border-primary/35 bg-primary/10 text-primary dark:border-primary/40 dark:bg-primary/20 dark:text-primary-foreground",
		};
	}
	switch (role) {
		case "system":
			return {
				containerClass:
					"border-zinc-300/80 bg-zinc-50/80 dark:border-zinc-600/60 dark:bg-zinc-900/40",
				badgeClass:
					"border-zinc-300/90 bg-zinc-100 text-zinc-700 dark:border-zinc-500/70 dark:bg-zinc-800/70 dark:text-zinc-100",
			};
		case "user":
		case "input":
			return {
				containerClass:
					"border-border/80 bg-background/70 dark:border-border/80 dark:bg-background/30",
				badgeClass:
					"border-border bg-muted text-muted-foreground dark:border-border/80 dark:bg-muted/50 dark:text-foreground",
			};
		case "tool":
			return {
				containerClass:
					"border-border/80 bg-muted/50 dark:border-border/80 dark:bg-muted/30",
				badgeClass:
					"border-border bg-muted text-muted-foreground dark:border-border/80 dark:bg-muted/50 dark:text-foreground",
			};
		default:
			return {
				containerClass: "border-border/80 bg-card/80",
				badgeClass: "border-border bg-muted text-foreground",
			};
	}
}

function LlmCallDetailSection(props: {
	detail: AdminLlmCallDetailResponse;
	onOpenParentTask: (taskId: string | null) => void;
}) {
	const { detail, onOpenParentTask } = props;
	const llmInputMessages = useMemo(() => {
		const parsed = parseLlmConversationMessages(detail.input_messages_json);
		if (parsed.length > 0) return parsed;
		if (detail.prompt_text.trim()) {
			return [{ role: "input", content: detail.prompt_text }];
		}
		return [];
	}, [detail.input_messages_json, detail.prompt_text]);
	const llmOutputMessages = useMemo(() => {
		const parsed = parseLlmConversationMessages(detail.output_messages_json);
		if (parsed.length > 0) return parsed;
		if (detail.response_text?.trim()) {
			return [{ role: "assistant", content: detail.response_text }];
		}
		return [];
	}, [detail.output_messages_json, detail.response_text]);
	const llmConversationTimeline = useMemo<LlmConversationTimelineItem[]>(() => {
		const timeline: LlmConversationTimelineItem[] = [];
		const turnCount = Math.max(
			llmInputMessages.length,
			llmOutputMessages.length,
		);
		for (let index = 0; index < turnCount; index += 1) {
			const turn = index + 1;
			const inputMessage = llmInputMessages[index];
			if (inputMessage) {
				timeline.push({
					key: `input-${turn}-${inputMessage.role}`,
					turn,
					source: "input",
					role: inputMessage.role,
					content: inputMessage.content,
				});
			}
			const outputMessage = llmOutputMessages[index];
			if (outputMessage) {
				timeline.push({
					key: `output-${turn}-${outputMessage.role}`,
					turn,
					source: "output",
					role: outputMessage.role,
					content: outputMessage.content,
				});
			}
		}
		return timeline;
	}, [llmInputMessages, llmOutputMessages]);
	const llmConversationTurnCount = useMemo(
		() =>
			llmConversationTimeline.reduce(
				(maxTurn, item) => Math.max(maxTurn, item.turn),
				0,
			),
		[llmConversationTimeline],
	);
	const llmAssistantMessageCount = useMemo(
		() =>
			llmConversationTimeline.filter((item) => item.role === "assistant")
				.length,
		[llmConversationTimeline],
	);
	const lastAssistantTimelineIndex = useMemo(() => {
		for (
			let index = llmConversationTimeline.length - 1;
			index >= 0;
			index -= 1
		) {
			if (llmConversationTimeline[index]?.role === "assistant") {
				return index;
			}
		}
		return -1;
	}, [llmConversationTimeline]);

	return (
		<>
			<div className="mt-4 grid gap-2 md:grid-cols-2">
				<div className="rounded-lg border p-3">
					<p className="text-muted-foreground text-xs">状态 / 模型</p>
					<p className="mt-1 font-medium">
						{taskStatusLabel(detail.status)} · {detail.model}
					</p>
				</div>
				<div className="rounded-lg border p-3">
					<p className="text-muted-foreground text-xs">用户 / 父任务</p>
					<p className="mt-1 font-medium">用户 #{detail.requested_by ?? "-"}</p>
					<p className="text-muted-foreground mt-1 text-xs">
						父任务 {detail.parent_task_id ?? "-"}
					</p>
				</div>
				<div className="rounded-lg border p-3">
					<p className="text-muted-foreground text-xs">耗时 / 重试</p>
					<p className="mt-1 font-medium">
						{formatDurationMs(detail.duration_ms)} /{" "}
						{formatCount(detail.attempt_count)}
					</p>
				</div>
				<div className="rounded-lg border p-3">
					<p className="text-muted-foreground text-xs">
						Token（输入 / 输出 / 缓存）
					</p>
					<p className="mt-1 font-medium">
						{formatCount(detail.input_tokens)} /{" "}
						{formatCount(detail.output_tokens)} /{" "}
						{formatCount(detail.cached_input_tokens)}
					</p>
					<p className="text-muted-foreground mt-1 text-xs">
						总计 {formatCount(detail.total_tokens)}
					</p>
				</div>
				<div className="rounded-lg border p-3">
					<p className="text-muted-foreground text-xs">创建 / 完成</p>
					<p className="mt-1 font-medium">
						{formatLocalDateTime(detail.created_at)}
					</p>
					<p className="text-muted-foreground mt-1 text-xs">
						完成 {formatLocalDateTime(detail.finished_at)}
					</p>
				</div>
			</div>

			<div className="mt-3 flex flex-wrap gap-2">
				<Button
					variant="outline"
					disabled={!detail.parent_task_id}
					onClick={() => void onOpenParentTask(detail.parent_task_id)}
				>
					查看父任务
				</Button>
			</div>

			<div className="mt-4 space-y-3 border-t pt-4">
				<div>
					<p className="text-muted-foreground text-xs">Conversation Timeline</p>
					{llmConversationTimeline.length === 0 ? (
						<pre className="bg-muted/40 mt-1 max-h-[24vh] overflow-auto rounded-md border p-2 text-[11px] whitespace-pre-wrap break-all">
							-
						</pre>
					) : (
						<div className="bg-muted/20 mt-1 rounded-xl border">
							<div className="border-b px-3 py-2">
								<div className="flex flex-wrap items-center justify-between gap-2">
									<p className="text-muted-foreground text-[11px] font-medium">
										多轮消息
									</p>
									<div className="flex flex-wrap items-center gap-1 text-[10px]">
										<span className="bg-background text-muted-foreground inline-flex items-center rounded-full border px-2 py-0.5">
											消息 {formatCount(llmConversationTimeline.length)}
										</span>
										<span className="bg-background text-muted-foreground inline-flex items-center rounded-full border px-2 py-0.5">
											轮次 {formatCount(llmConversationTurnCount)}
										</span>
										<span className="bg-background text-muted-foreground inline-flex items-center rounded-full border px-2 py-0.5">
											助手 {formatCount(llmAssistantMessageCount)}
										</span>
									</div>
								</div>
							</div>
							<div className="max-h-[31vh] space-y-2.5 overflow-auto px-3 py-3 pr-2">
								{llmConversationTimeline.map((message, index) => {
									const isAssistantOutput =
										message.source === "output" && message.role === "assistant";
									const showAnswerLatency =
										index === lastAssistantTimelineIndex;
									const tone = llmRoleTone(message.role, isAssistantOutput);
									return (
										<div
											key={message.key}
											className={`flex ${
												isAssistantOutput
													? "justify-end pl-5"
													: "justify-start pr-5"
											}`}
										>
											<article
												className={`w-full max-w-[95%] rounded-2xl border px-3.5 py-2.5 shadow-sm ${tone.containerClass}`}
											>
												<div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px]">
													<span
														className={`inline-flex items-center rounded-md border px-1.5 py-0.5 font-semibold ${tone.badgeClass}`}
													>
														{llmRoleLabel(message.role)}
													</span>
													<span className="text-muted-foreground">
														第 {formatCount(message.turn)} 轮
													</span>
													{showAnswerLatency ? (
														<span className="text-muted-foreground ml-auto text-[10px] font-medium">
															等待 {formatDurationMs(detail.scheduler_wait_ms)}{" "}
															· 首字{" "}
															{formatDurationMs(detail.first_token_wait_ms)}
														</span>
													) : null}
												</div>
												<div className="mt-1.5 text-[13px] leading-5 whitespace-pre-wrap break-words">
													{message.content}
												</div>
											</article>
										</div>
									);
								})}
							</div>
						</div>
					)}
				</div>
				{llmConversationTimeline.length === 0 ? (
					<>
						<div>
							<p className="text-muted-foreground text-xs">Input Messages</p>
							<pre className="bg-muted/40 mt-1 max-h-[18vh] overflow-auto rounded-md border p-2 text-[11px] whitespace-pre-wrap break-all">
								{detail.prompt_text || "-"}
							</pre>
						</div>
						<div>
							<p className="text-muted-foreground text-xs">Output Messages</p>
							<pre className="bg-muted/40 mt-1 max-h-[18vh] overflow-auto rounded-md border p-2 text-[11px] whitespace-pre-wrap break-all">
								{detail.response_text ?? "-"}
							</pre>
						</div>
					</>
				) : null}
				<div>
					<p className="text-muted-foreground text-xs">Error</p>
					<pre className="bg-muted/40 mt-1 max-h-[12vh] overflow-auto rounded-md border p-2 text-[11px] whitespace-pre-wrap break-all">
						{detail.error_text ?? "-"}
					</pre>
				</div>
			</div>
		</>
	);
}

function normalizeErrorMessage(err: unknown) {
	if (err instanceof ApiError) {
		switch (err.code) {
			case "forbidden_admin_only":
				return "当前账号没有管理员权限。";
			case "not_found":
				return "目标任务不存在。";
			case "invalid_task_state":
				return "当前任务状态不允许执行该操作。";
			default:
				return err.message;
		}
	}
	return err instanceof Error ? err.message : String(err);
}

function taskStatusLabel(status: string) {
	switch (status) {
		case "queued":
			return "排队中";
		case "running":
			return "运行中";
		case "succeeded":
			return "成功";
		case "failed":
			return "失败";
		case "canceled":
			return "已取消";
		default:
			return status;
	}
}

type TaskStatusTone = {
	cardAccentClass: string;
	badgeClass: string;
	dotClass: string;
};

function llmCallStatusSortOrder(status: string) {
	switch (status) {
		case "running":
			return 0;
		case "queued":
			return 1;
		default:
			return 2;
	}
}

function llmCallCreatedAtSortKey(createdAt: string) {
	const timestamp = Date.parse(createdAt);
	return Number.isNaN(timestamp) ? Number.MIN_SAFE_INTEGER : timestamp;
}

function sortLlmCallsForDisplay(calls: AdminLlmCallItem[]) {
	return [...calls].sort((left, right) => {
		const statusDiff =
			llmCallStatusSortOrder(left.status) -
			llmCallStatusSortOrder(right.status);
		if (statusDiff !== 0) {
			return statusDiff;
		}

		const createdDiff =
			llmCallCreatedAtSortKey(right.created_at) -
			llmCallCreatedAtSortKey(left.created_at);
		if (createdDiff !== 0) {
			return createdDiff;
		}

		const createdAtDiff = right.created_at.localeCompare(left.created_at);
		if (createdAtDiff !== 0) {
			return createdAtDiff;
		}

		return right.id.localeCompare(left.id);
	});
}

function taskStatusTone(status: string): TaskStatusTone {
	switch (status) {
		case "queued":
			return {
				cardAccentClass: "border-l-amber-500",
				badgeClass:
					"border-amber-300 bg-amber-100/90 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100",
				dotClass: "bg-amber-500",
			};
		case "running":
			return {
				cardAccentClass: "border-l-sky-500",
				badgeClass:
					"border-sky-300 bg-sky-100/90 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100",
				dotClass: "bg-sky-500",
			};
		case "succeeded":
			return {
				cardAccentClass: "border-l-emerald-500",
				badgeClass:
					"border-emerald-300 bg-emerald-100/90 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100",
				dotClass: "bg-emerald-500",
			};
		case "failed":
			return {
				cardAccentClass: "border-l-red-500",
				badgeClass:
					"border-red-300 bg-red-100/90 text-red-900 dark:border-red-500/60 dark:bg-red-500/20 dark:text-red-100",
				dotClass: "bg-red-500",
			};
		case "canceled":
			return {
				cardAccentClass: "border-l-slate-500",
				badgeClass:
					"border-slate-300 bg-slate-100/90 text-slate-900 dark:border-slate-500/60 dark:bg-slate-500/20 dark:text-slate-100",
				dotClass: "bg-slate-500",
			};
		default:
			return {
				cardAccentClass: "border-l-border",
				badgeClass:
					"border-border bg-muted/60 text-foreground dark:border-border dark:bg-muted/50 dark:text-foreground",
				dotClass: "bg-muted-foreground",
			};
	}
}

type BadgeTone = Pick<TaskStatusTone, "badgeClass" | "dotClass">;

const REALTIME_STATUS_FILTER_OPTIONS: Array<{
	value: RealtimeStatusFilter;
	label: string;
}> = [
	{ value: "all", label: "状态：全部" },
	{ value: "queued", label: "状态：排队" },
	{ value: "running", label: "状态：运行中" },
	{ value: "failed", label: "状态：失败" },
	{ value: "succeeded", label: "状态：成功" },
	{ value: "canceled", label: "状态：取消" },
];

const LLM_STATUS_FILTER_OPTIONS: Array<{
	value: LlmStatusFilter;
	label: string;
}> = [
	{ value: "all", label: "状态：全部" },
	{ value: "queued", label: "状态：排队" },
	{ value: "running", label: "状态：运行中" },
	{ value: "failed", label: "状态：失败" },
	{ value: "succeeded", label: "状态：成功" },
];

function streamStatusTone(
	status: "connecting" | "connected" | "reconnecting",
): BadgeTone {
	switch (status) {
		case "connected":
			return {
				badgeClass:
					"border-emerald-300 bg-emerald-100/80 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100",
				dotClass: "bg-emerald-500",
			};
		case "reconnecting":
			return {
				badgeClass:
					"border-amber-300 bg-amber-100/80 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100",
				dotClass: "bg-amber-500",
			};
		default:
			return {
				badgeClass:
					"border-sky-300 bg-sky-100/80 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100",
				dotClass: "bg-sky-500",
			};
	}
}

function StatusBadge(props: {
	label: string;
	tone: BadgeTone;
	className?: string;
}) {
	const { label, tone, className } = props;
	return (
		<Badge
			variant="outline"
			className={`gap-1.5 border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tone.badgeClass}${className ? ` ${className}` : ""}`}
		>
			<span className={`size-1.5 rounded-full ${tone.dotClass}`} />
			{label}
		</Badge>
	);
}

function FlagBadge(props: { label: string; className: string }) {
	const { label, className } = props;
	return (
		<Badge
			variant="outline"
			className={`border px-2 py-0.5 text-[11px] font-medium ${className}`}
		>
			{label}
		</Badge>
	);
}

function FilterSelect<T extends string>(props: {
	value: T;
	onValueChange: (value: T) => void;
	options: Array<{ value: T; label: string }>;
	placeholder: string;
	ariaLabel: string;
	className?: string;
}) {
	const { value, onValueChange, options, placeholder, ariaLabel, className } =
		props;
	return (
		<Select
			value={value}
			onValueChange={(nextValue) => onValueChange(nextValue as T)}
		>
			<SelectTrigger className={className ?? "w-full"} aria-label={ariaLabel}>
				<SelectValue placeholder={placeholder} />
			</SelectTrigger>
			<SelectContent>
				{options.map((option) => (
					<SelectItem key={option.value} value={option.value}>
						{option.label}
					</SelectItem>
				))}
			</SelectContent>
		</Select>
	);
}

function LoadingMessage(props: { children: string }) {
	return (
		<p className="text-muted-foreground inline-flex items-center gap-2 text-sm">
			<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
			{props.children}
		</p>
	);
}

function taskTypeLabel(taskType: string) {
	switch (taskType) {
		case "brief.daily_slot":
			return "定时日报";
		case "sync.subscriptions":
			return "订阅同步";
		case "brief.generate":
			return "日报生成";
		case "brief.refresh_content":
			return "日报内容修复";
		case "sync.all":
			return "全量同步";
		case "sync.access_refresh":
			return "访问增量同步";
		case "sync.starred":
			return "同步 Star";
		case "sync.releases":
			return "同步 Release";
		case "sync.notifications":
			return "同步通知";
		case "translate.release":
			return "翻译 Release";
		case "translate.release.batch":
			return "批量翻译 Release";
		case "summarize.release.smart.batch":
			return "批量润色 Release";
		case "translate.release_detail":
			return "翻译 Release 详情";
		case "translate.notification":
			return "翻译通知";
		default:
			return taskType;
	}
}

type EventLevel = "normal" | "success" | "warning" | "danger";

type EventPresentation = {
	title: string;
	description: string;
	level: EventLevel;
	payload: Record<string, unknown> | null;
};

function asObject(payload: unknown): Record<string, unknown> | null {
	if (!payload || typeof payload !== "object" || Array.isArray(payload)) {
		return null;
	}
	return payload as Record<string, unknown>;
}

function readString(payload: Record<string, unknown> | null, key: string) {
	if (!payload) return null;
	const value = payload[key];
	if (typeof value === "string") return value;
	if (typeof value === "number" || typeof value === "boolean") {
		return String(value);
	}
	return null;
}

function readNumber(payload: Record<string, unknown> | null, key: string) {
	if (!payload) return null;
	const value = payload[key];
	return typeof value === "number" ? value : null;
}

function readBoolean(payload: Record<string, unknown> | null, key: string) {
	if (!payload) return null;
	const value = payload[key];
	if (typeof value === "boolean") return value;
	if (typeof value === "string") {
		if (value === "true" || value === "1") return true;
		if (value === "false" || value === "0") return false;
	}
	if (typeof value === "number") return value !== 0;
	return null;
}

function formatEventPresentation(event: AdminTaskEventItem): EventPresentation {
	let payload: Record<string, unknown> | null = null;
	try {
		payload = asObject(JSON.parse(event.payload_json));
	} catch {
		payload = null;
	}

	if (event.event_type === "task.created") {
		const source = readString(payload, "source");
		return {
			title: "任务已创建",
			description: source ? `触发来源：${source}` : "任务已进入队列等待执行。",
			level: "normal",
			payload,
		};
	}

	if (event.event_type === "task.running") {
		return {
			title: "任务开始执行",
			description: "任务已被执行器领取并开始处理。",
			level: "normal",
			payload,
		};
	}

	if (event.event_type === "task.cancel_requested") {
		return {
			title: "已请求取消",
			description: "任务运行中收到取消请求，将在安全点停止。",
			level: "warning",
			payload,
		};
	}

	if (event.event_type === "task.canceled") {
		return {
			title: "任务已取消",
			description: "任务在排队阶段被取消。",
			level: "warning",
			payload,
		};
	}

	if (event.event_type === "task.progress") {
		const stage = readString(payload, "stage");
		if (stage === "collect") {
			const totalUsers = readNumber(payload, "total_users");
			const totalReleases = readNumber(payload, "total_releases");
			const hourUtc = readNumber(payload, "hour_utc");
			return {
				title: "收集执行对象",
				description:
					totalUsers !== null && hourUtc !== null
						? `UTC ${hourUtc.toString().padStart(2, "0")}:00 收集到 ${totalUsers} 位用户。`
						: totalReleases !== null
							? `收集到 ${totalReleases} 条 Release 待处理。`
							: "任务正在收集本轮执行对象。",
				level: "normal",
				payload,
			};
		}
		if (stage === "star_summary") {
			const totalUsers = readNumber(payload, "total_users");
			const succeededUsers = readNumber(payload, "succeeded_users");
			const failedUsers = readNumber(payload, "failed_users");
			return {
				title: "Star 阶段完成",
				description:
					totalUsers !== null
						? `用户 ${totalUsers} · 成功 ${succeededUsers ?? "-"} · 失败 ${failedUsers ?? "-"}`
						: "Star 阶段已输出汇总。",
				level: failedUsers !== null && failedUsers > 0 ? "warning" : "success",
				payload,
			};
		}
		if (stage === "repo_collect") {
			const totalRepos = readNumber(payload, "total_repos");
			return {
				title: "聚合仓库队列",
				description:
					totalRepos !== null
						? `本轮聚合出 ${totalRepos} 个待抓取 Release 的仓库。`
						: "已完成仓库聚合。",
				level: "normal",
				payload,
			};
		}
		if (stage === "release_summary") {
			const totalRepos = readNumber(payload, "total_repos");
			const succeededRepos = readNumber(payload, "succeeded_repos");
			const failedRepos = readNumber(payload, "failed_repos");
			const releasesWritten = readNumber(payload, "releases_written");
			return {
				title: "Release 阶段完成",
				description:
					totalRepos !== null
						? `仓库 ${totalRepos} · 成功 ${succeededRepos ?? "-"} · 失败 ${failedRepos ?? "-"} · 写入 ${releasesWritten ?? "-"}`
						: "Release 阶段已输出汇总。",
				level: failedRepos !== null && failedRepos > 0 ? "warning" : "success",
				payload,
			};
		}
		if (stage === "skipped") {
			const skipReason = readString(payload, "skip_reason");
			return {
				title: "本轮已跳过",
				description: skipReason
					? `定时任务未执行，原因：${skipReason}`
					: "定时任务被跳过。",
				level: "warning",
				payload,
			};
		}
		if (stage === "generate") {
			const index = readNumber(payload, "index");
			const total = readNumber(payload, "total");
			const userId = readString(payload, "user_id");
			return {
				title: "串行执行中",
				description:
					index !== null && total !== null && userId !== null
						? `正在处理第 ${index}/${total} 位用户（#${userId}）。`
						: "正在串行执行当前批次用户。",
				level: "normal",
				payload,
			};
		}
		if (stage === "release") {
			const releaseId = readString(payload, "release_id");
			const itemStatus = readString(payload, "item_status");
			const itemError = readString(payload, "item_error");
			const level =
				itemStatus === "error"
					? "danger"
					: itemStatus === "ready"
						? "success"
						: itemStatus === "missing" || itemStatus === "disabled"
							? "warning"
							: "normal";
			return {
				title: "Release 处理结果",
				description: releaseId
					? `Release #${releaseId} · ${itemStatus ?? "unknown"}${itemError ? ` · ${itemError}` : ""}`
					: "记录了单条 Release 处理结果。",
				level,
				payload,
			};
		}
		if (stage === "user_succeeded") {
			const userId = readString(payload, "user_id");
			const keyDate = readString(payload, "key_date");
			const contentLength = readNumber(payload, "content_length");
			return {
				title: "单用户执行成功",
				description:
					userId !== null
						? `用户 #${userId} 生成成功${keyDate ? ` · key_date=${keyDate}` : ""}${
								contentLength !== null ? ` · ${contentLength} chars` : ""
							}`
						: "有用户日报生成成功。",
				level: "success",
				payload,
			};
		}
		if (stage === "user_failed") {
			const userId = readString(payload, "user_id");
			const error = readString(payload, "error");
			return {
				title: "单用户执行失败",
				description:
					userId !== null
						? `用户 #${userId} 失败${error ? `：${error}` : ""}`
						: "有用户执行失败，任务继续处理后续用户。",
				level: "danger",
				payload,
			};
		}
		if (stage === "summary") {
			const total = readNumber(payload, "total");
			const succeeded = readNumber(payload, "succeeded");
			const failed = readNumber(payload, "failed");
			const canceled = readBoolean(payload, "canceled");
			return {
				title: "任务汇总",
				description:
					total !== null || succeeded !== null || failed !== null
						? `总计 ${total ?? "-"} · 成功 ${succeeded ?? "-"} · 失败 ${failed ?? "-"}${
								canceled === true ? " · 已取消" : ""
							}`
						: "任务输出了汇总统计。",
				level:
					failed !== null && failed > 0
						? "warning"
						: canceled === true
							? "warning"
							: "success",
				payload,
			};
		}
	}

	if (event.event_type === "task.completed") {
		const status = readString(payload, "status") ?? "";
		if (status === "succeeded") {
			return {
				title: "任务执行完成",
				description: "任务已完成并写入结果。",
				level: "success",
				payload,
			};
		}
		if (status === "failed") {
			const error = readString(payload, "error");
			return {
				title: "任务执行失败",
				description: error ? `失败原因：${error}` : "任务执行失败。",
				level: "danger",
				payload,
			};
		}
		if (status === "canceled") {
			return {
				title: "任务已终止",
				description: "任务被取消并结束。",
				level: "warning",
				payload,
			};
		}
	}

	return {
		title: event.event_type,
		description: "记录了一条任务事件。",
		level: "normal",
		payload,
	};
}

function eventLevelClass(level: EventLevel) {
	switch (level) {
		case "success":
			return "border-emerald-500/40 bg-emerald-500/5";
		case "warning":
			return "border-amber-500/40 bg-amber-500/5";
		case "danger":
			return "border-red-500/40 bg-red-500/5";
		default:
			return "border-border bg-card/70";
	}
}

function businessOutcomeBannerClass(code: string) {
	switch (code) {
		case "failed":
			return "border-red-500/40 bg-red-500/5 text-red-900 dark:text-red-100";
		case "partial":
			return "border-amber-500/40 bg-amber-500/5 text-amber-900 dark:text-amber-100";
		case "disabled":
			return "border-slate-500/40 bg-slate-500/5 text-slate-900 dark:text-slate-100";
		default:
			return "border-border bg-muted/30 text-foreground";
	}
}

function sourceLabel(source: string) {
	switch (source) {
		case "scheduler":
			return "定时调度";
		case "retry":
			return "手动重试";
		default:
			return source;
	}
}

type RealtimeStatusFilter =
	| "all"
	| "queued"
	| "running"
	| "failed"
	| "succeeded"
	| "canceled";

type LlmStatusFilter = "all" | "queued" | "running" | "failed" | "succeeded";

const TASK_PAGE_SIZE = 20;
const SCHEDULED_TASK_TYPES = new Set([
	"brief.daily_slot",
	"sync.subscriptions",
]);
const STREAM_REFRESH_DELAY_MS = 600;
const STREAM_RECONNECT_DELAY_MS = 1500;

type StreamStatus = "connecting" | "connected" | "reconnecting";

function normalizePathname(pathname: string) {
	return pathname.replace(/\/+$/, "") || "/";
}

type JobManagementProps = {
	currentUserId: LocalUserId;
	routeState?: AdminJobsRouteState;
	onNavigateRoute?: (
		nextRoute: AdminJobsRouteState,
		options?: {
			replace?: boolean;
		},
	) => void;
};

type LoadOptions = {
	background?: boolean;
};

type ListLoadPhase = "idle" | "initial" | "refreshing";

function resolveListLoadPhase(
	hasLoadedOnce: boolean,
	options?: LoadOptions,
): ListLoadPhase {
	if (!hasLoadedOnce || !options?.background) {
		return "initial";
	}
	return "refreshing";
}

function shouldReuseInitialListRequest(
	hasLoadedOnce: boolean,
	initialRequestInFlight: boolean,
	activeRequestKey: string | null,
	nextRequestKey: string,
): boolean {
	return (
		!hasLoadedOnce &&
		initialRequestInFlight &&
		activeRequestKey === nextRequestKey
	);
}

type TranslationStatusFilter =
	| "all"
	| "queued"
	| "running"
	| "completed"
	| "failed";

type TranslationDrawerState =
	| { kind: "request"; id: string }
	| { kind: "batch"; id: string }
	| { kind: "worker"; id: string }
	| null;

const TRANSLATION_STATUS_FILTER_OPTIONS: Array<{
	value: TranslationStatusFilter;
	label: string;
}> = [
	{ value: "all", label: "状态：全部" },
	{ value: "queued", label: "状态：排队" },
	{ value: "running", label: "状态：运行中" },
	{ value: "completed", label: "状态：已完成" },
	{ value: "failed", label: "状态：失败" },
];

function translationRunStatusLabel(status: string) {
	switch (status) {
		case "completed":
			return "已完成";
		case "queued":
			return "排队中";
		case "running":
			return "运行中";
		case "failed":
			return "失败";
		default:
			return status;
	}
}

function translationItemStatusLabel(status: string) {
	switch (status) {
		case "ready":
			return "就绪";
		case "disabled":
			return "已禁用";
		case "missing":
			return "缺失";
		case "error":
			return "错误";
		case "queued":
			return "排队中";
		case "running":
			return "处理中";
		default:
			return status;
	}
}

function translationRunTone(status: string): TaskStatusTone {
	switch (status) {
		case "completed":
			return taskStatusTone("succeeded");
		case "error":
			return taskStatusTone("failed");
		case "idle":
			return taskStatusTone("canceled");
		default:
			return taskStatusTone(status);
	}
}

function translationBusinessOutcomeTone(code: string): BadgeTone {
	switch (code) {
		case "ok":
			return taskStatusTone("succeeded");
		case "partial":
			return taskStatusTone("canceled");
		case "failed":
			return taskStatusTone("failed");
		case "disabled":
			return {
				badgeClass:
					"border-slate-300 bg-slate-100/90 text-slate-900 dark:border-slate-500/60 dark:bg-slate-500/20 dark:text-slate-100",
				dotClass: "bg-slate-500",
			};
		default:
			return {
				badgeClass:
					"border-border bg-muted/60 text-foreground dark:border-border dark:bg-muted/50 dark:text-foreground",
				dotClass: "bg-muted-foreground",
			};
	}
}

function translationBatchResultSummaryText(
	summary:
		| AdminTranslationBatchListItem["result_summary"]
		| AdminTranslationBatchDetailResponse["batch"]["result_summary"],
) {
	const segments = [
		{ label: "就绪", count: summary.ready },
		{ label: "错误", count: summary.error },
		{ label: "缺失", count: summary.missing },
		{ label: "禁用", count: summary.disabled },
		{ label: "排队", count: summary.queued },
		{ label: "运行中", count: summary.running },
	]
		.filter((item) => item.count > 0)
		.map((item) => `${item.label} ${formatCount(item.count)}`);
	return segments.length > 0 ? segments.join(" · ") : "暂无条目汇总";
}

function translationItemTone(status: string): BadgeTone {
	switch (status) {
		case "ready":
			return taskStatusTone("succeeded");
		case "disabled":
			return taskStatusTone("canceled");
		case "missing":
			return taskStatusTone("queued");
		case "error":
			return taskStatusTone("failed");
		case "queued":
			return taskStatusTone("queued");
		case "running":
			return taskStatusTone("running");
		default:
			return taskStatusTone(status);
	}
}

function translationResultBadge(status: string, runStatus?: string) {
	if (status === "queued" && runStatus === "running") {
		return {
			label: "处理中",
			tone: taskStatusTone("running"),
		};
	}

	return {
		label: translationItemStatusLabel(status),
		tone: translationItemTone(status),
	};
}

function translationRequestOriginLabel(origin: string) {
	switch (origin) {
		case "user":
			return "用户";
		case "system":
			return "系统";
		default:
			return origin || "-";
	}
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

function translationErrorSummaryText(error: {
	error_summary?: string | null;
	error?: string | null;
	error_text?: string | null;
}) {
	return error.error_summary ?? error.error ?? error.error_text ?? null;
}

function translationErrorDetailText(error: {
	error_detail?: string | null;
	error_text?: string | null;
	error?: string | null;
}) {
	const detail = error.error_detail ?? error.error_text ?? null;
	const summary = translationErrorSummaryText(error);
	if (!detail || detail === summary) return null;
	return detail;
}

function TranslationSchedulerSection(props: {
	viewTab: TranslationViewTab;
	onViewTabChange: (nextValue: TranslationViewTab) => void;
	refreshNonce: number;
	onOpenLlmCallDetail: (callId: string) => void;
}) {
	const { refreshNonce, onOpenLlmCallDetail, onViewTabChange, viewTab } = props;
	const [status, setStatus] = useState<AdminTranslationStatusResponse | null>(
		null,
	);
	const [statusLoading, setStatusLoading] = useState(false);
	const [requestStatusFilter, setRequestStatusFilter] =
		useState<TranslationStatusFilter>("all");
	const [requests, setRequests] = useState<AdminTranslationRequestListItem[]>(
		[],
	);
	const [requestTotal, setRequestTotal] = useState(0);
	const [requestPage, setRequestPage] = useState(1);
	const [requestLoadPhase, setRequestLoadPhase] =
		useState<ListLoadPhase>("idle");
	const [batchStatusFilter, setBatchStatusFilter] =
		useState<TranslationStatusFilter>("all");
	const [batches, setBatches] = useState<AdminTranslationBatchListItem[]>([]);
	const [batchTotal, setBatchTotal] = useState(0);
	const [batchPage, setBatchPage] = useState(1);
	const [batchLoadPhase, setBatchLoadPhase] = useState<ListLoadPhase>("idle");
	const [drawer, setDrawer] = useState<TranslationDrawerState>(null);
	const [requestDetail, setRequestDetail] =
		useState<AdminTranslationRequestDetailResponse | null>(null);
	const [batchDetail, setBatchDetail] =
		useState<AdminTranslationBatchDetailResponse | null>(null);
	const [drawerLoading, setDrawerLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [settingsDialogOpen, setSettingsDialogOpen] = useState(false);
	const [generalWorkerInput, setGeneralWorkerInput] = useState("");
	const [dedicatedWorkerInput, setDedicatedWorkerInput] = useState("");
	const [settingsSaveError, setSettingsSaveError] = useState<string | null>(
		null,
	);
	const [settingsSaving, setSettingsSaving] = useState(false);
	const requestLoadedRef = useRef(false);
	const batchLoadedRef = useRef(false);
	const requestTotalPages = useMemo(
		() => Math.max(1, Math.ceil(requestTotal / TASK_PAGE_SIZE)),
		[requestTotal],
	);
	const batchTotalPages = useMemo(
		() => Math.max(1, Math.ceil(batchTotal / TASK_PAGE_SIZE)),
		[batchTotal],
	);
	const requestsRefreshing = requestLoadPhase === "refreshing";
	const requestsInitialLoading = requestLoadPhase === "initial";
	const batchesRefreshing = batchLoadPhase === "refreshing";
	const batchesInitialLoading = batchLoadPhase === "initial";
	const selectedWorker = useMemo(() => {
		if (drawer?.kind !== "worker") {
			return null;
		}
		return (
			status?.workers.find((worker) => worker.worker_id === drawer.id) ?? null
		);
	}, [drawer, status]);
	const workerBoardDescription = useMemo(
		() =>
			formatTranslationWorkerBoardDescription(
				status?.target_general_worker_concurrency,
				status?.target_dedicated_worker_concurrency,
			),
		[status],
	);

	useEffect(() => {
		if (drawer?.kind !== "worker") {
			return;
		}
		if (!status || selectedWorker) {
			return;
		}
		setDrawer(null);
	}, [drawer, selectedWorker, status]);

	const loadStatus = useCallback(async () => {
		setStatusLoading(true);
		try {
			setStatus(await apiGetAdminTranslationStatus());
		} finally {
			setStatusLoading(false);
		}
	}, []);

	const loadRequests = useCallback(
		async (background = false) => {
			setRequestLoadPhase(
				resolveListLoadPhase(requestLoadedRef.current, { background }),
			);
			try {
				const params = new URLSearchParams();
				if (requestStatusFilter !== "all") {
					params.set("status", requestStatusFilter);
				}
				params.set("page", String(requestPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				const res = await apiGetAdminTranslationRequests(params);
				setRequests(res.items);
				setRequestTotal(res.total);
				requestLoadedRef.current = true;
			} finally {
				setRequestLoadPhase("idle");
			}
		},
		[requestPage, requestStatusFilter],
	);

	const loadBatches = useCallback(
		async (background = false) => {
			setBatchLoadPhase(
				resolveListLoadPhase(batchLoadedRef.current, { background }),
			);
			try {
				const params = new URLSearchParams();
				if (batchStatusFilter !== "all") {
					params.set("status", batchStatusFilter);
				}
				params.set("page", String(batchPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				const res = await apiGetAdminTranslationBatches(params);
				setBatches(res.items);
				setBatchTotal(res.total);
				batchLoadedRef.current = true;
			} finally {
				setBatchLoadPhase("idle");
			}
		},
		[batchPage, batchStatusFilter],
	);

	const openRequestDetail = useCallback(async (requestId: string) => {
		setDrawer({ kind: "request", id: requestId });
		setDrawerLoading(true);
		setError(null);
		try {
			setBatchDetail(null);
			setRequestDetail(await apiGetAdminTranslationRequestDetail(requestId));
		} catch (err) {
			setError(normalizeErrorMessage(err));
		} finally {
			setDrawerLoading(false);
		}
	}, []);

	const openBatchDetail = useCallback(async (batchId: string) => {
		setDrawer({ kind: "batch", id: batchId });
		setDrawerLoading(true);
		setError(null);
		try {
			setRequestDetail(null);
			setBatchDetail(await apiGetAdminTranslationBatchDetail(batchId));
		} catch (err) {
			setError(normalizeErrorMessage(err));
		} finally {
			setDrawerLoading(false);
		}
	}, []);

	const openWorkerDetail = useCallback((workerId: string) => {
		setDrawer({ kind: "worker", id: workerId });
		setRequestDetail(null);
		setBatchDetail(null);
		setDrawerLoading(false);
		setError(null);
	}, []);

	const openSettingsDialog = useCallback(() => {
		setSettingsSaveError(null);
		setGeneralWorkerInput(
			String(status?.target_general_worker_concurrency ?? ""),
		);
		setDedicatedWorkerInput(
			String(status?.target_dedicated_worker_concurrency ?? ""),
		);
		setSettingsDialogOpen(true);
	}, [status]);

	const saveSettings = useCallback(async () => {
		const generalWorkerConcurrency =
			parsePositiveIntegerInput(generalWorkerInput);
		if (!generalWorkerConcurrency) {
			setSettingsSaveError("通用 worker 数量必须是大于 0 的整数。");
			return;
		}
		const dedicatedWorkerConcurrency =
			parsePositiveIntegerInput(dedicatedWorkerInput);
		if (!dedicatedWorkerConcurrency) {
			setSettingsSaveError("用户专用 worker 数量必须是大于 0 的整数。");
			return;
		}

		setSettingsSaving(true);
		setSettingsSaveError(null);
		try {
			const nextStatus = await apiPatchAdminTranslationRuntimeConfig({
				general_worker_concurrency: generalWorkerConcurrency,
				dedicated_worker_concurrency: dedicatedWorkerConcurrency,
			});
			setStatus(nextStatus);
			setSettingsDialogOpen(false);
		} catch (err) {
			setSettingsSaveError(normalizeErrorMessage(err));
		} finally {
			setSettingsSaving(false);
		}
	}, [dedicatedWorkerInput, generalWorkerInput]);

	useEffect(() => {
		if (refreshNonce < 0) return;
		setError(null);
		void loadStatus().catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadStatus, refreshNonce]);

	useEffect(() => {
		setError(null);
		void loadRequests(refreshNonce > 0).catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadRequests, refreshNonce]);

	useEffect(() => {
		setError(null);
		void loadBatches(refreshNonce > 0).catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadBatches, refreshNonce]);

	useEffect(() => {
		if (refreshNonce < 0) return;
		if (!drawer) {
			setRequestDetail(null);
			setBatchDetail(null);
			setDrawerLoading(false);
			return;
		}
		if (drawer.kind === "worker") {
			setRequestDetail(null);
			setBatchDetail(null);
			setDrawerLoading(false);
			return;
		}
		let canceled = false;
		setDrawerLoading(true);
		const load = async () => {
			try {
				if (drawer.kind === "request") {
					const detail = await apiGetAdminTranslationRequestDetail(drawer.id);
					if (!canceled) {
						setRequestDetail(detail);
						setBatchDetail(null);
					}
				} else {
					const detail = await apiGetAdminTranslationBatchDetail(drawer.id);
					if (!canceled) {
						setBatchDetail(detail);
						setRequestDetail(null);
					}
				}
			} catch (err) {
				if (!canceled) {
					setError(normalizeErrorMessage(err));
				}
			} finally {
				if (!canceled) {
					setDrawerLoading(false);
				}
			}
		};
		void load();
		return () => {
			canceled = true;
		};
	}, [drawer, refreshNonce]);

	return (
		<>
			{error ? <p className="text-destructive text-sm">{error}</p> : null}
			<Card>
				<CardHeader>
					<CardTitle>翻译调度</CardTitle>
					<CardDescription>
						统一查看真实工作者槽位、需求队列与任务记录，并保留现有详情抽屉链路。
					</CardDescription>
				</CardHeader>
				<CardContent className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4">
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">调度器 / LLM</p>
						<p className="mt-1 text-sm font-semibold">
							{status?.scheduler_enabled ? "已启用" : "未启用"} /{" "}
							{status?.llm_enabled ? "可翻译" : "AI未配置"}
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							扫描 {formatDurationMs(status?.scan_interval_ms ?? null)}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">请求 / 工作项</p>
						<p className="mt-1 text-sm font-semibold">
							{formatCount(status?.queued_requests)} /{" "}
							{formatCount(status?.queued_work_items)}
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							待处理请求 / 待处理 work item
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">运行中批次</p>
						<p className="mt-1 text-sm font-semibold">
							{formatCount(status?.running_batches)}
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							预算 {formatCount(status?.batch_token_threshold)} tokens ·
							模型输入 {formatCount(status?.effective_model_input_limit)} tokens
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">近24h 批次</p>
						<p className="mt-1 text-sm font-semibold">
							{formatCount(status?.clean_completed_batches_24h)} /{" "}
							{formatCount(status?.completed_with_issues_batches_24h)} /{" "}
							{formatCount(status?.failed_batches_24h)}
						</p>
						<p className="text-muted-foreground mt-1 text-xs">
							干净完成 / 带问题完成 / 失败 · error items{" "}
							{formatCount(status?.error_work_items_24h)} · missing items{" "}
							{formatCount(status?.missing_work_items_24h)} · 平均等待{" "}
							{formatDurationMs(status?.avg_wait_ms_24h ?? null)}
						</p>
					</div>
				</CardContent>
			</Card>

			<TranslationWorkerBoard
				workers={status?.workers ?? []}
				loading={statusLoading && !status}
				description={workerBoardDescription}
				headerAction={
					<Button
						type="button"
						variant="outline"
						size="icon"
						aria-label="配置翻译 worker 数量"
						onClick={openSettingsDialog}
						disabled={!status || settingsSaving}
					>
						<Settings2 />
					</Button>
				}
				onWorkerClick={(worker) => openWorkerDetail(worker.worker_id)}
			/>

			<Tabs
				value={viewTab}
				onValueChange={(nextValue) =>
					onViewTabChange(nextValue as TranslationViewTab)
				}
				className="space-y-4"
			>
				<TabsList>
					<TabsTrigger value="queue">需求队列</TabsTrigger>
					<TabsTrigger value="history">任务记录</TabsTrigger>
				</TabsList>

				<TabsContent value="queue">
					<Card>
						<CardHeader>
							<CardTitle>需求队列</CardTitle>
							<CardDescription>
								按请求对象查看全部翻译需求，默认按活跃优先排序。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
								<FilterSelect
									value={requestStatusFilter}
									onValueChange={(value) => {
										setRequestPage(1);
										setRequestStatusFilter(value);
									}}
									options={TRANSLATION_STATUS_FILTER_OPTIONS}
									placeholder="状态筛选"
									ariaLabel="翻译请求状态筛选"
									className="w-full sm:w-[220px]"
								/>
								<span className="text-muted-foreground text-xs">
									共 {formatCount(requestTotal)} 条
								</span>
							</div>
							{requestsRefreshing ? (
								<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
									<span className="size-2 rounded-full bg-amber-500/80" />
									需求队列更新中...
								</p>
							) : null}
							{requestsInitialLoading ? (
								<LoadingMessage>正在加载需求队列...</LoadingMessage>
							) : requests.length === 0 ? (
								<p className="text-muted-foreground text-sm">暂无需求。</p>
							) : (
								<>
									<div className="hidden md:block">
										<Table
											containerClassName="rounded-lg border"
											className="w-full table-fixed text-sm"
										>
											<TableHeader>
												<TableRow>
													<TableHead className="w-[18%]">来源</TableHead>
													<TableHead className="w-[16%]">请求</TableHead>
													<TableHead className="w-[10%]">状态</TableHead>
													<TableHead className="w-[17%]">请求ID</TableHead>
													<TableHead className="w-[13%]">请求人</TableHead>
													<TableHead className="w-[13%]">作用域</TableHead>
													<TableHead className="w-[11%]">更新时间</TableHead>
													<TableHead className="w-[88px] text-right">
														操作
													</TableHead>
												</TableRow>
											</TableHeader>
											<TableBody>
												{requests.map((request) => (
													<TableRow key={request.id}>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap">
																{request.source}
															</div>
															<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
																{request.producer_ref}
															</p>
														</TableCell>
														<TableCell className="px-3 py-3">
															<p className="truncate whitespace-nowrap">
																{request.kind} · {request.variant}
															</p>
															<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
																entity {request.entity_id} ·{" "}
																{translationRequestOriginLabel(
																	request.request_origin,
																)}
															</p>
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap">
															<StatusBadge
																label={translationRunStatusLabel(
																	request.status,
																)}
																tone={translationRunTone(request.status)}
															/>
														</TableCell>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap font-mono text-xs">
																{request.id}
															</div>
														</TableCell>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap font-mono text-xs">
																{request.requested_by ?? "-"}
															</div>
														</TableCell>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap font-mono text-xs">
																#{request.scope_user_id}
															</div>
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap text-xs">
															{formatLocalDateTime(request.updated_at)}
														</TableCell>
														<TableCell className="px-3 py-3 text-right">
															<Button
																variant="outline"
																size="sm"
																onClick={() =>
																	void openRequestDetail(request.id)
																}
															>
																详情
															</Button>
														</TableCell>
													</TableRow>
												))}
											</TableBody>
										</Table>
									</div>
									<div className="space-y-2 md:hidden">
										{requests.map((request) => (
											<div key={request.id} className="rounded-lg border p-3">
												<div className="flex items-center gap-2">
													<p className="truncate whitespace-nowrap font-medium text-sm">
														{request.source}
													</p>
													<StatusBadge
														label={translationRunStatusLabel(request.status)}
														tone={translationRunTone(request.status)}
													/>
												</div>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													{request.kind} · {request.variant} · entity{" "}
													{request.entity_id}
												</p>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													{translationRequestOriginLabel(
														request.request_origin,
													)}{" "}
													· requested_by {request.requested_by ?? "-"}
												</p>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													scope #{request.scope_user_id} · producer_ref{" "}
													{request.producer_ref}
												</p>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													{request.batch_id
														? `batch ${request.batch_id} · `
														: ""}
													更新 {formatLocalDateTime(request.updated_at)}
												</p>
												<div className="mt-2 flex justify-end">
													<Button
														variant="outline"
														size="sm"
														onClick={() => void openRequestDetail(request.id)}
													>
														详情
													</Button>
												</div>
											</div>
										))}
									</div>
								</>
							)}

							<div className="flex items-center justify-between">
								<p className="text-muted-foreground text-xs">
									第 {requestPage}/{requestTotalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										variant="outline"
										size="sm"
										disabled={requestPage <= 1 || requestLoadPhase !== "idle"}
										onClick={() =>
											setRequestPage((prev) => Math.max(1, prev - 1))
										}
									>
										上一页
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={
											requestPage >= requestTotalPages ||
											requestLoadPhase !== "idle"
										}
										onClick={() =>
											setRequestPage((prev) =>
												Math.min(requestTotalPages, prev + 1),
											)
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				</TabsContent>

				<TabsContent value="history">
					<Card>
						<CardHeader>
							<CardTitle>任务记录</CardTitle>
							<CardDescription>
								按真实 translation batch 历史查看 worker 执行结果。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
								<FilterSelect
									value={batchStatusFilter}
									onValueChange={(value) => {
										setBatchPage(1);
										setBatchStatusFilter(value);
									}}
									options={TRANSLATION_STATUS_FILTER_OPTIONS}
									placeholder="状态筛选"
									ariaLabel="翻译批次状态筛选"
									className="w-full sm:w-[220px]"
								/>
								<span className="text-muted-foreground text-xs">
									共 {formatCount(batchTotal)} 条
								</span>
							</div>
							{batchesRefreshing ? (
								<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
									<span className="size-2 rounded-full bg-amber-500/80" />
									任务记录更新中...
								</p>
							) : null}
							{batchesInitialLoading ? (
								<LoadingMessage>正在加载任务记录...</LoadingMessage>
							) : batches.length === 0 ? (
								<p className="text-muted-foreground text-sm">暂无任务记录。</p>
							) : (
								<>
									<div className="hidden md:block">
										<Table
											containerClassName="rounded-lg border"
											className="w-full table-fixed text-sm"
										>
											<TableHeader>
												<TableRow>
													<TableHead className="w-[11%]">触发原因</TableHead>
													<TableHead className="w-[11%]">状态</TableHead>
													<TableHead className="w-[7%]">槽位</TableHead>
													<TableHead className="w-[7%]">请求数</TableHead>
													<TableHead className="w-[7%]">Items</TableHead>
													<TableHead className="w-[9%]">预算</TableHead>
													<TableHead className="w-[20%]">批次ID</TableHead>
													<TableHead className="w-[10%]">更新时间</TableHead>
													<TableHead className="w-[88px] text-right">
														操作
													</TableHead>
												</TableRow>
											</TableHeader>
											<TableBody>
												{batches.map((batch) => (
													<TableRow key={batch.id}>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap">
																{batch.trigger_reason}
															</div>
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap">
															<div className="flex flex-wrap items-center gap-2">
																<StatusBadge
																	label={translationRunStatusLabel(
																		batch.status,
																	)}
																	tone={translationRunTone(batch.status)}
																/>
																<StatusBadge
																	label={batch.business_outcome.label}
																	tone={translationBusinessOutcomeTone(
																		batch.business_outcome.code,
																	)}
																/>
															</div>
															<p className="text-muted-foreground mt-1 whitespace-normal text-[11px] leading-5">
																{translationBatchResultSummaryText(
																	batch.result_summary,
																)}
															</p>
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap">
															{translationWorkerSlotLabel(batch.worker_slot)}
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap text-sm">
															{formatCount(batch.request_count)}
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap text-sm">
															{formatCount(batch.item_count)}
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap text-sm">
															{formatCount(batch.estimated_input_tokens)}
														</TableCell>
														<TableCell className="px-3 py-3">
															<div className="truncate whitespace-nowrap font-mono text-xs">
																{batch.id}
															</div>
														</TableCell>
														<TableCell className="px-3 py-3 whitespace-nowrap text-xs">
															{formatLocalDateTime(batch.updated_at)}
														</TableCell>
														<TableCell className="px-3 py-3 text-right">
															<Button
																variant="outline"
																size="sm"
																onClick={() => void openBatchDetail(batch.id)}
															>
																详情
															</Button>
														</TableCell>
													</TableRow>
												))}
											</TableBody>
										</Table>
									</div>
									<div className="space-y-2 md:hidden">
										{batches.map((batch) => (
											<div key={batch.id} className="rounded-lg border p-3">
												<div className="flex items-center gap-2">
													<p className="truncate whitespace-nowrap font-medium text-sm">
														{batch.trigger_reason}
													</p>
													<StatusBadge
														label={translationRunStatusLabel(batch.status)}
														tone={translationRunTone(batch.status)}
													/>
													<StatusBadge
														label={batch.business_outcome.label}
														tone={translationBusinessOutcomeTone(
															batch.business_outcome.code,
														)}
													/>
												</div>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													{translationWorkerSlotLabel(batch.worker_slot)} · 请求{" "}
													{formatCount(batch.request_count)} · work items{" "}
													{formatCount(batch.item_count)}
												</p>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap text-xs">
													预算 {formatCount(batch.estimated_input_tokens)}{" "}
													tokens · 更新 {formatLocalDateTime(batch.updated_at)}
												</p>
												<p className="text-muted-foreground mt-1 text-xs">
													{translationBatchResultSummaryText(
														batch.result_summary,
													)}
												</p>
												<p className="text-muted-foreground mt-1 truncate whitespace-nowrap font-mono text-[11px]">
													{batch.id}
												</p>
												<div className="mt-2 flex justify-end">
													<Button
														variant="outline"
														size="sm"
														onClick={() => void openBatchDetail(batch.id)}
													>
														详情
													</Button>
												</div>
											</div>
										))}
									</div>
								</>
							)}

							<div className="flex items-center justify-between">
								<p className="text-muted-foreground text-xs">
									第 {batchPage}/{batchTotalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										variant="outline"
										size="sm"
										disabled={batchPage <= 1 || batchLoadPhase !== "idle"}
										onClick={() =>
											setBatchPage((prev) => Math.max(1, prev - 1))
										}
									>
										上一页
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={
											batchPage >= batchTotalPages || batchLoadPhase !== "idle"
										}
										onClick={() =>
											setBatchPage((prev) =>
												Math.min(batchTotalPages, prev + 1),
											)
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				</TabsContent>
			</Tabs>

			<Sheet
				open={Boolean(drawer)}
				onOpenChange={(open) => {
					if (!open) {
						setDrawer(null);
					}
				}}
			>
				<SheetContent
					side="right"
					showCloseButton={false}
					className="w-full gap-0 overflow-y-auto p-0 sm:max-w-3xl"
				>
					<SheetHeader className="gap-3 border-b px-5 py-4 text-left">
						<div className="flex items-start justify-between gap-3">
							<div className="min-w-0 space-y-2">
								<SheetTitle className="text-lg">
									{drawer?.kind === "batch"
										? "翻译批次详情"
										: drawer?.kind === "worker"
											? "工作者详情"
											: "翻译请求详情"}
								</SheetTitle>
								<SheetDescription>
									{drawer?.kind === "batch"
										? "查看批次 item、工作者槽位、错误原因与关联 LLM 调用。"
										: drawer?.kind === "worker"
											? "查看当前槽位状态、负载与关联批次。"
											: "查看单条请求结果与 fan-out 归属。"}
								</SheetDescription>
							</div>
							<Button variant="outline" onClick={() => setDrawer(null)}>
								关闭
							</Button>
						</div>
					</SheetHeader>
					<div className="space-y-4 px-5 py-4">
						{drawerLoading ? (
							<LoadingMessage>正在加载详情...</LoadingMessage>
						) : drawer?.kind === "request" && requestDetail ? (
							<>
								<div className="grid gap-2 md:grid-cols-2">
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">请求状态</p>
										<p className="mt-1 font-medium">
											{translationRunStatusLabel(requestDetail.request.status)}
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											来源类型{" "}
											{translationRequestOriginLabel(
												requestDetail.request.request_origin,
											)}
										</p>
									</div>
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">
											来源 / 请求人
										</p>
										<p className="mt-1 font-medium">
											{requestDetail.request.source}
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											requested_by {requestDetail.request.requested_by ?? "-"} ·
											scope #{requestDetail.request.scope_user_id}
										</p>
									</div>
								</div>
								<div className="rounded-lg border p-3">
									<div className="flex flex-wrap items-center gap-2">
										<p className="font-medium text-sm">
											{requestDetail.result.kind} ·{" "}
											{requestDetail.result.variant}
										</p>
										<StatusBadge
											label={
												translationResultBadge(
													requestDetail.result.status,
													requestDetail.request.status,
												).label
											}
											tone={
												translationResultBadge(
													requestDetail.result.status,
													requestDetail.request.status,
												).tone
											}
										/>
									</div>
									<p className="text-muted-foreground mt-1 text-xs">
										entity {requestDetail.result.entity_id} · producer_ref{" "}
										{requestDetail.result.producer_ref}
									</p>
									{requestDetail.result.title_zh ||
									requestDetail.result.summary_md ||
									requestDetail.result.body_md ? (
										<div className="text-muted-foreground mt-2 space-y-1 text-xs">
											{requestDetail.result.title_zh ? (
												<p>标题：{requestDetail.result.title_zh}</p>
											) : null}
											{requestDetail.result.summary_md ? (
												<p>摘要：{requestDetail.result.summary_md}</p>
											) : null}
											{requestDetail.result.body_md ? (
												<p>正文：{requestDetail.result.body_md}</p>
											) : null}
										</div>
									) : null}
									<div className="mt-2 flex flex-wrap items-center gap-2">
										{requestDetail.result.batch_id ? (
											<Button
												variant="outline"
												size="sm"
												onClick={() =>
													void openBatchDetail(requestDetail.result.batch_id!)
												}
											>
												查看批次
											</Button>
										) : null}
										{translationErrorSummaryText(requestDetail.result) ? (
											<div className="space-y-1 text-xs">
												<span className="text-destructive block">
													{translationErrorSummaryText(requestDetail.result)}
												</span>
												{requestDetail.result.error_code ? (
													<span className="text-muted-foreground font-mono block text-[11px]">
														{requestDetail.result.error_code}
													</span>
												) : null}
												{translationErrorDetailText(requestDetail.result) ? (
													<span className="text-muted-foreground block">
														{translationErrorDetailText(requestDetail.result)}
													</span>
												) : null}
											</div>
										) : null}
									</div>
								</div>
							</>
						) : drawer?.kind === "worker" && selectedWorker ? (
							<>
								<div className="grid gap-2 md:grid-cols-2">
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">工作者</p>
										<p className="mt-1 font-medium">
											{translationWorkerSlotLabel(selectedWorker.worker_slot)} ·{" "}
											{translationWorkerKindLabel(selectedWorker.worker_kind)}
										</p>
										<p className="text-muted-foreground mt-1 font-mono text-[11px]">
											{selectedWorker.worker_id}
										</p>
									</div>
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">当前状态</p>
										<div className="mt-1">
											<StatusBadge
												label={translationRunStatusLabel(selectedWorker.status)}
												tone={translationRunTone(selectedWorker.status)}
											/>
										</div>
										<p className="text-muted-foreground mt-1 text-xs">
											最近更新 {formatLocalDateTime(selectedWorker.updated_at)}
										</p>
									</div>
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">当前负载</p>
										<p className="mt-1 font-medium">
											{formatCount(selectedWorker.request_count)} requests ·{" "}
											{formatCount(selectedWorker.work_item_count)} items
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											trigger {selectedWorker.trigger_reason ?? "-"}
										</p>
									</div>
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">当前批次</p>
										<p className="mt-1 font-medium">
											{selectedWorker.current_batch_id ?? "-"}
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											槽位{" "}
											{translationWorkerSlotLabel(selectedWorker.worker_slot)}
										</p>
									</div>
								</div>

								<div className="rounded-lg border p-3">
									<div className="flex flex-wrap items-center justify-between gap-2">
										<div>
											<p className="text-muted-foreground text-xs">
												错误与跳转
											</p>
											<p className="mt-1 text-sm">
												{translationErrorSummaryText(selectedWorker) ??
													"当前没有错误信息。"}
											</p>
											{selectedWorker.error_code ? (
												<p className="text-muted-foreground mt-1 font-mono text-[11px]">
													{selectedWorker.error_code}
												</p>
											) : null}
											{translationErrorDetailText(selectedWorker) ? (
												<p className="text-muted-foreground mt-1 text-xs">
													{translationErrorDetailText(selectedWorker)}
												</p>
											) : null}
										</div>
										{selectedWorker.current_batch_id ? (
											<Button
												variant="outline"
												size="sm"
												onClick={() =>
													void openBatchDetail(selectedWorker.current_batch_id!)
												}
											>
												查看当前批次
											</Button>
										) : null}
									</div>
								</div>
							</>
						) : drawer?.kind === "batch" && batchDetail ? (
							<>
								<div className="grid gap-2 md:grid-cols-2">
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">批次状态</p>
										<div className="mt-1 flex flex-wrap items-center gap-2">
											<StatusBadge
												label={translationRunStatusLabel(
													batchDetail.batch.status,
												)}
												tone={translationRunTone(batchDetail.batch.status)}
											/>
											<StatusBadge
												label={batchDetail.batch.business_outcome.label}
												tone={translationBusinessOutcomeTone(
													batchDetail.batch.business_outcome.code,
												)}
											/>
										</div>
										<p className="text-muted-foreground mt-1 text-xs">
											{batchDetail.batch.trigger_reason}
										</p>
										<p className="text-muted-foreground mt-1 text-xs leading-5">
											{batchDetail.batch.business_outcome.message}
										</p>
									</div>
									<div className="rounded-lg border p-3">
										<p className="text-muted-foreground text-xs">
											槽位 / 请求数
										</p>
										<p className="mt-1 font-medium">
											{translationWorkerSlotLabel(
												batchDetail.batch.worker_slot,
											)}{" "}
											· {formatCount(batchDetail.batch.request_count)} requests
										</p>
										<p className="text-muted-foreground mt-1 text-xs">
											items {formatCount(batchDetail.batch.item_count)} ·{" "}
											{formatCount(batchDetail.batch.estimated_input_tokens)}{" "}
											tokens
										</p>
										<p className="text-muted-foreground mt-1 text-xs leading-5">
											{translationBatchResultSummaryText(
												batchDetail.batch.result_summary,
											)}
										</p>
									</div>
								</div>
								<div className="space-y-2">
									<p className="text-muted-foreground text-xs">批次条目</p>
									{batchDetail.items.map((item) => (
										<div
											key={`${item.work_item_id ?? "batch-item"}:${item.entity_id}:${item.kind}:${item.variant}`}
											className="rounded-lg border p-3"
										>
											<div className="flex flex-wrap items-center gap-2">
												<p className="font-medium text-sm">
													{item.kind} · {item.variant}
												</p>
												<StatusBadge
													label={
														translationResultBadge(
															item.status,
															batchDetail.batch.status,
														).label
													}
													tone={
														translationResultBadge(
															item.status,
															batchDetail.batch.status,
														).tone
													}
												/>
											</div>
											<p className="text-muted-foreground mt-1 text-xs">
												entity {item.entity_id} · fan-out batch{" "}
												{item.batch_id ?? "-"}
											</p>
											{translationErrorSummaryText(item) ? (
												<p className="text-destructive mt-1 text-xs">
													{translationErrorSummaryText(item)}
												</p>
											) : null}
											{item.error_code ? (
												<p className="text-muted-foreground mt-1 font-mono text-[11px]">
													{item.error_code}
												</p>
											) : null}
											{translationErrorDetailText(item) ? (
												<p className="text-muted-foreground mt-1 text-xs">
													{translationErrorDetailText(item)}
												</p>
											) : null}
										</div>
									))}
								</div>
								<div className="space-y-2 border-t pt-4">
									<div className="flex items-center justify-between gap-2">
										<p className="text-muted-foreground text-xs">
											关联 LLM 调用
										</p>
										<span className="text-muted-foreground text-xs">
											{formatCount(batchDetail.llm_calls.length)} 条
										</span>
									</div>
									{batchDetail.llm_calls.length === 0 ? (
										<p className="text-muted-foreground text-sm">
											暂无关联 LLM 调用。
										</p>
									) : (
										batchDetail.llm_calls.map((call) => (
											<div key={call.id} className="rounded-lg border p-3">
												<div className="flex flex-wrap items-center justify-between gap-2">
													<div>
														<p className="font-medium text-sm">{call.source}</p>
														<p className="text-muted-foreground mt-1 font-mono text-[11px]">
															{call.id}
														</p>
														<p className="text-muted-foreground mt-1 text-xs">
															等待 {formatDurationMs(call.scheduler_wait_ms)} ·
															耗时 {formatDurationMs(call.duration_ms)}
														</p>
													</div>
													<Button
														variant="outline"
														size="sm"
														onClick={() => onOpenLlmCallDetail(call.id)}
													>
														打开 LLM 详情
													</Button>
												</div>
											</div>
										))
									)}
								</div>
							</>
						) : (
							<p className="text-muted-foreground text-sm">未找到详情数据。</p>
						)}
					</div>
				</SheetContent>
			</Sheet>

			<Dialog
				open={settingsDialogOpen}
				onOpenChange={(open) => {
					setSettingsDialogOpen(open);
					if (!open) {
						setSettingsSaveError(null);
					}
				}}
			>
				<DialogContent className="max-w-md">
					<DialogHeader>
						<DialogTitle>配置翻译 worker 数量</DialogTitle>
						<DialogDescription>
							保存后立即生效；缩容不会打断当前批次，超出的 worker
							会在当前工作完成后自然退场。
						</DialogDescription>
					</DialogHeader>

					<div className="space-y-4">
						<div className="space-y-2">
							<Label htmlFor="translation-general-worker-concurrency">
								通用 worker 数量
							</Label>
							<Input
								id="translation-general-worker-concurrency"
								type="number"
								min={1}
								step={1}
								inputMode="numeric"
								value={generalWorkerInput}
								onChange={(event) => setGeneralWorkerInput(event.target.value)}
							/>
						</div>
						<div className="space-y-2">
							<Label htmlFor="translation-dedicated-worker-concurrency">
								用户专用 worker 数量
							</Label>
							<Input
								id="translation-dedicated-worker-concurrency"
								type="number"
								min={1}
								step={1}
								inputMode="numeric"
								value={dedicatedWorkerInput}
								onChange={(event) =>
									setDedicatedWorkerInput(event.target.value)
								}
							/>
						</div>
						{settingsSaveError ? (
							<p className="text-destructive text-sm">{settingsSaveError}</p>
						) : (
							<p className="text-muted-foreground text-xs">
								工作者板会按通用 worker 在前、用户专用 worker
								在后的顺序连续编号。
							</p>
						)}
					</div>

					<DialogFooter>
						<Button
							type="button"
							variant="outline"
							onClick={() => setSettingsDialogOpen(false)}
							disabled={settingsSaving}
						>
							取消
						</Button>
						<Button
							type="button"
							onClick={() => void saveSettings()}
							disabled={settingsSaving}
						>
							{settingsSaving ? "保存中…" : "保存设置"}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>
		</>
	);
}

export function JobManagement({
	currentUserId,
	routeState: controlledRouteState,
	onNavigateRoute,
}: JobManagementProps) {
	const isRouteControlled = controlledRouteState !== undefined;
	const [uncontrolledRouteState, setUncontrolledRouteState] =
		useState<AdminJobsRouteState>(() =>
			parseAdminJobsRoute(window.location.pathname, window.location.search),
		);
	const routeState = controlledRouteState ?? uncontrolledRouteState;
	const [overview, setOverview] = useState<AdminJobsOverviewResponse | null>(
		null,
	);
	const [overviewLoading, setOverviewLoading] = useState(false);

	const [statusFilter, setStatusFilter] = useState<RealtimeStatusFilter>("all");
	const [tasks, setTasks] = useState<AdminRealtimeTaskItem[]>([]);
	const [taskTotal, setTaskTotal] = useState(0);
	const [taskPage, setTaskPage] = useState(1);
	const [tasksLoadPhase, setTasksLoadPhase] = useState<ListLoadPhase>("idle");
	const [taskActionBusyId, setTaskActionBusyId] = useState<string | null>(null);

	const [scheduledRunStatusFilter, setScheduledRunStatusFilter] =
		useState<RealtimeStatusFilter>("all");
	const [scheduledRuns, setScheduledRuns] = useState<AdminRealtimeTaskItem[]>(
		[],
	);
	const [scheduledRunTotal, setScheduledRunTotal] = useState(0);
	const [scheduledRunPage, setScheduledRunPage] = useState(1);
	const [scheduledRunsLoadPhase, setScheduledRunsLoadPhase] =
		useState<ListLoadPhase>("idle");

	const [llmStatus, setLlmStatus] =
		useState<AdminLlmSchedulerStatusResponse | null>(null);
	const [llmStatusLoading, setLlmStatusLoading] = useState(false);
	const [llmSettingsDialogOpen, setLlmSettingsDialogOpen] = useState(false);
	const [llmMaxConcurrencyInput, setLlmMaxConcurrencyInput] = useState("");
	const [llmModelContextLimitInput, setLlmModelContextLimitInput] =
		useState("");
	const [llmSettingsSaveError, setLlmSettingsSaveError] = useState<
		string | null
	>(null);
	const [llmSettingsSaving, setLlmSettingsSaving] = useState(false);
	const [llmStatusFilter, setLlmStatusFilter] =
		useState<LlmStatusFilter>("all");
	const [llmSourceFilter, setLlmSourceFilter] = useState("");
	const [llmRequestedByFilter, setLlmRequestedByFilter] = useState("");
	const [llmStartedFromFilter, setLlmStartedFromFilter] = useState("");
	const [llmStartedToFilter, setLlmStartedToFilter] = useState("");
	const [llmCalls, setLlmCalls] = useState<AdminLlmCallItem[]>([]);
	const [llmCallTotal, setLlmCallTotal] = useState(0);
	const [llmCallPage, setLlmCallPage] = useState(1);
	const [llmCallsLoadPhase, setLlmCallsLoadPhase] =
		useState<ListLoadPhase>("idle");
	const [llmDetail, setLlmDetail] = useState<AdminLlmCallDetailResponse | null>(
		null,
	);
	const [llmDetailLoading, setLlmDetailLoading] = useState(false);
	const [taskDrawerLlmDetail, setTaskDrawerLlmDetail] =
		useState<AdminLlmCallDetailResponse | null>(null);
	const [taskDrawerLlmLoading, setTaskDrawerLlmLoading] = useState(false);
	const [taskRelatedLlmCalls, setTaskRelatedLlmCalls] = useState<
		AdminLlmCallItem[]
	>([]);
	const [taskRelatedLlmLoading, setTaskRelatedLlmLoading] = useState(false);

	const [detailTask, setDetailTask] =
		useState<AdminRealtimeTaskDetailResponse | null>(null);
	const [detailLoading, setDetailLoading] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [streamStatus, setStreamStatus] = useState<StreamStatus>("connecting");
	const [refreshNonce, setRefreshNonce] = useState(0);

	const overviewLoadedOnceRef = useRef(false);
	const overviewInitialRequestInFlightRef = useRef(false);
	const overviewRequestIdRef = useRef(0);
	const tasksLoadedOnceRef = useRef(false);
	const tasksInitialRequestInFlightRef = useRef(false);
	const tasksRequestIdRef = useRef(0);
	const tasksRequestKeyRef = useRef<string | null>(null);
	const scheduledRunsLoadedOnceRef = useRef(false);
	const scheduledRunsInitialRequestInFlightRef = useRef(false);
	const scheduledRunsRequestIdRef = useRef(0);
	const llmStatusLoadedOnceRef = useRef(false);
	const llmStatusInitialRequestInFlightRef = useRef(false);
	const llmStatusRequestIdRef = useRef(0);
	const llmCallsLoadedOnceRef = useRef(false);
	const llmCallsInitialRequestInFlightRef = useRef(false);
	const llmCallsRequestIdRef = useRef(0);
	const detailTaskIdRef = useRef<string | null>(null);
	const llmDetailIdRef = useRef<string | null>(null);
	const activeTaskDrawerLlmCallIdRef = useRef<string | null>(null);
	const streamRefreshTimerRef = useRef<number | null>(null);
	const streamRefreshInFlightRef = useRef(false);
	const streamPendingFullRefreshRef = useRef(false);
	const streamPendingDetailTaskIdRef = useRef<string | null>(null);
	const streamPendingLlmRefreshRef = useRef(false);
	const streamPendingLlmDetailCallIdRef = useRef<string | null>(null);
	const streamPendingTranslationRefreshRef = useRef(false);

	const taskTotalPages = useMemo(
		() => Math.max(1, Math.ceil(taskTotal / TASK_PAGE_SIZE)),
		[taskTotal],
	);
	const scheduledRunTotalPages = useMemo(
		() => Math.max(1, Math.ceil(scheduledRunTotal / TASK_PAGE_SIZE)),
		[scheduledRunTotal],
	);
	const llmCallTotalPages = useMemo(
		() => Math.max(1, Math.ceil(llmCallTotal / TASK_PAGE_SIZE)),
		[llmCallTotal],
	);
	const taskEvents = useMemo(() => {
		if (!detailTask) return [];
		return [...detailTask.events]
			.sort(
				(a, b) =>
					a.created_at.localeCompare(b.created_at) || a.id.localeCompare(b.id),
			)
			.map((event) => ({
				event,
				presentation: formatEventPresentation(event),
			}));
	}, [detailTask]);
	const detailTaskTone = useMemo(
		() => (detailTask ? taskStatusTone(detailTask.task.status) : null),
		[detailTask],
	);
	const tab = routeState.primaryTab;
	const translationView = routeState.translationView;
	const taskDrawerRoute = routeState.taskDrawerRoute;
	const taskDrawerFromTab = routeState.drawerFromTab;
	const activeTaskDrawerTaskId = taskDrawerRoute?.taskId ?? null;
	const activeTaskDrawerLlmCallId = taskDrawerRoute?.llmCallId ?? null;
	const isTaskDrawerOpen = activeTaskDrawerTaskId !== null;
	const isTaskDrawerLlmRoute = activeTaskDrawerLlmCallId !== null;
	const activeTaskDetail =
		detailTask && detailTask.task.id === activeTaskDrawerTaskId
			? detailTask
			: null;
	const detailBusinessOutcome =
		activeTaskDetail?.diagnostics?.business_outcome ?? null;
	const detailEventMeta = activeTaskDetail?.event_meta ?? null;
	const tasksInitialLoading = tasksLoadPhase === "initial";
	const tasksRefreshing = tasksLoadPhase === "refreshing";
	const tasksActionsDisabled = detailLoading || tasksLoadPhase !== "idle";
	const scheduledRunsInitialLoading = scheduledRunsLoadPhase === "initial";
	const scheduledRunsRefreshing = scheduledRunsLoadPhase === "refreshing";
	const scheduledRunActionsDisabled =
		detailLoading || scheduledRunsLoadPhase !== "idle";
	const llmCallsInitialLoading = llmCallsLoadPhase === "initial";
	const llmCallsRefreshing = llmCallsLoadPhase === "refreshing";
	const llmStatusRefreshing = llmStatusLoading && llmStatus !== null;
	const llmRefreshing = llmStatusRefreshing || llmCallsRefreshing;
	const llmCallActionsDisabled = llmDetailLoading || llmRefreshing;
	const isRefreshingData =
		overviewLoading ||
		tasksLoadPhase !== "idle" ||
		scheduledRunsLoadPhase !== "idle" ||
		llmStatusLoading ||
		llmCallsLoadPhase !== "idle";

	useEffect(() => {
		detailTaskIdRef.current = activeTaskDetail?.task.id ?? null;
	}, [activeTaskDetail]);

	useEffect(() => {
		activeTaskDrawerLlmCallIdRef.current = activeTaskDrawerLlmCallId;
		llmDetailIdRef.current = activeTaskDrawerLlmCallId ?? llmDetail?.id ?? null;
	}, [activeTaskDrawerLlmCallId, llmDetail]);

	useEffect(() => {
		activeTaskDrawerLlmCallIdRef.current = activeTaskDrawerLlmCallId;
	}, [activeTaskDrawerLlmCallId]);

	const navigateAdminJobsRoute = useCallback(
		(
			nextRoute: AdminJobsRouteState,
			options?: {
				replace?: boolean;
			},
		) => {
			if (onNavigateRoute) {
				onNavigateRoute(nextRoute, options);
				return;
			}
			const currentSearch = window.location.search;
			const nextUrl = buildAdminJobsRouteUrl(nextRoute, currentSearch);
			const currentUrl = `${normalizePathname(window.location.pathname)}${currentSearch}`;
			const allowPathSync =
				normalizePathname(window.location.pathname) === ADMIN_JOBS_BASE_PATH ||
				normalizePathname(window.location.pathname).startsWith(
					`${ADMIN_JOBS_BASE_PATH}/`,
				);
			if (allowPathSync && nextUrl !== currentUrl) {
				if (options?.replace) {
					window.history.replaceState({}, "", nextUrl);
				} else {
					window.history.pushState({}, "", nextUrl);
				}
			}
			setUncontrolledRouteState(nextRoute);
		},
		[onNavigateRoute],
	);

	useEffect(() => {
		if (isRouteControlled) return;
		const syncRouteFromWindow = (replace = false) => {
			const nextRoute = parseAdminJobsRoute(
				window.location.pathname,
				window.location.search,
			);
			const currentSearch = window.location.search;
			const canonicalUrl = buildAdminJobsRouteUrl(nextRoute, currentSearch);
			const currentUrl = `${normalizePathname(window.location.pathname)}${currentSearch}`;
			if (replace && canonicalUrl !== currentUrl) {
				window.history.replaceState({}, "", canonicalUrl);
			}
			setUncontrolledRouteState(nextRoute);
		};

		syncRouteFromWindow(true);

		const onPopState = () => {
			syncRouteFromWindow(true);
		};
		window.addEventListener("popstate", onPopState);
		return () => {
			window.removeEventListener("popstate", onPopState);
		};
	}, [isRouteControlled]);

	const loadOverview = useCallback(async (options?: LoadOptions) => {
		if (
			options?.background &&
			!overviewLoadedOnceRef.current &&
			overviewInitialRequestInFlightRef.current
		) {
			return;
		}
		const requestId = overviewRequestIdRef.current + 1;
		overviewRequestIdRef.current = requestId;
		overviewInitialRequestInFlightRef.current = !overviewLoadedOnceRef.current;
		setOverviewLoading(true);
		try {
			const res = await apiGetAdminJobsOverview();
			if (requestId !== overviewRequestIdRef.current) {
				return;
			}
			setOverview(res);
			overviewLoadedOnceRef.current = true;
		} catch (err) {
			if (requestId !== overviewRequestIdRef.current) {
				return;
			}
			throw err;
		} finally {
			if (requestId === overviewRequestIdRef.current) {
				setOverviewLoading(false);
				overviewInitialRequestInFlightRef.current = false;
			}
		}
	}, []);

	const loadRealtimeTasks = useCallback(
		async (options?: LoadOptions) => {
			const requestKey = `${statusFilter}:${taskPage}`;
			if (
				shouldReuseInitialListRequest(
					tasksLoadedOnceRef.current,
					tasksInitialRequestInFlightRef.current,
					tasksRequestKeyRef.current,
					requestKey,
				)
			) {
				return;
			}
			const requestId = tasksRequestIdRef.current + 1;
			tasksRequestIdRef.current = requestId;
			tasksRequestKeyRef.current = requestKey;
			tasksInitialRequestInFlightRef.current = !tasksLoadedOnceRef.current;
			setTasksLoadPhase(
				resolveListLoadPhase(tasksLoadedOnceRef.current, options),
			);
			try {
				const params = new URLSearchParams();
				params.set("status", statusFilter);
				params.set("task_group", "realtime");
				params.set("page", String(taskPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				const res = await apiGetAdminRealtimeTasks(params);
				if (requestId !== tasksRequestIdRef.current) {
					return;
				}
				const realtimeOnlyItems = res.items.filter(
					(task) => !SCHEDULED_TASK_TYPES.has(task.task_type),
				);
				// Fallback for older backend versions that ignore task_group.
				const realtimeTotal =
					realtimeOnlyItems.length === res.items.length
						? res.total
						: realtimeOnlyItems.length;
				setTasks(realtimeOnlyItems);
				setTaskTotal(realtimeTotal);
				tasksLoadedOnceRef.current = true;
			} catch (err) {
				if (requestId !== tasksRequestIdRef.current) {
					return;
				}
				throw err;
			} finally {
				if (requestId === tasksRequestIdRef.current) {
					setTasksLoadPhase("idle");
					tasksInitialRequestInFlightRef.current = false;
				}
			}
		},
		[statusFilter, taskPage],
	);

	const loadScheduledRuns = useCallback(
		async (options?: LoadOptions) => {
			if (
				options?.background &&
				!scheduledRunsLoadedOnceRef.current &&
				scheduledRunsInitialRequestInFlightRef.current
			) {
				return;
			}
			const requestId = scheduledRunsRequestIdRef.current + 1;
			scheduledRunsRequestIdRef.current = requestId;
			scheduledRunsInitialRequestInFlightRef.current =
				!scheduledRunsLoadedOnceRef.current;
			setScheduledRunsLoadPhase(
				resolveListLoadPhase(scheduledRunsLoadedOnceRef.current, options),
			);
			try {
				const params = new URLSearchParams();
				params.set("status", scheduledRunStatusFilter);
				params.set("task_group", "scheduled");
				params.set("page", String(scheduledRunPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				const res = await apiGetAdminRealtimeTasks(params);
				if (requestId !== scheduledRunsRequestIdRef.current) {
					return;
				}
				const scheduledItems = res.items.filter((task) =>
					SCHEDULED_TASK_TYPES.has(task.task_type),
				);
				const scheduledTotal =
					scheduledItems.length === res.items.length
						? res.total
						: scheduledItems.length;
				setScheduledRuns(scheduledItems);
				setScheduledRunTotal(scheduledTotal);
				scheduledRunsLoadedOnceRef.current = true;
			} catch (err) {
				if (requestId !== scheduledRunsRequestIdRef.current) {
					return;
				}
				throw err;
			} finally {
				if (requestId === scheduledRunsRequestIdRef.current) {
					setScheduledRunsLoadPhase("idle");
					scheduledRunsInitialRequestInFlightRef.current = false;
				}
			}
		},
		[scheduledRunPage, scheduledRunStatusFilter],
	);

	const loadLlmSchedulerStatus = useCallback(async (options?: LoadOptions) => {
		if (
			options?.background &&
			!llmStatusLoadedOnceRef.current &&
			llmStatusInitialRequestInFlightRef.current
		) {
			return;
		}
		const requestId = llmStatusRequestIdRef.current + 1;
		llmStatusRequestIdRef.current = requestId;
		llmStatusInitialRequestInFlightRef.current =
			!llmStatusLoadedOnceRef.current;
		setLlmStatusLoading(true);
		try {
			const res = await apiGetAdminLlmSchedulerStatus();
			if (requestId !== llmStatusRequestIdRef.current) {
				return;
			}
			setLlmStatus(res);
			llmStatusLoadedOnceRef.current = true;
		} catch (err) {
			if (requestId !== llmStatusRequestIdRef.current) {
				return;
			}
			throw err;
		} finally {
			if (requestId === llmStatusRequestIdRef.current) {
				setLlmStatusLoading(false);
				llmStatusInitialRequestInFlightRef.current = false;
			}
		}
	}, []);

	const openLlmSettingsDialog = useCallback(() => {
		setLlmSettingsSaveError(null);
		setLlmMaxConcurrencyInput(String(llmStatus?.max_concurrency ?? ""));
		setLlmModelContextLimitInput(
			typeof llmStatus?.ai_model_context_limit === "number"
				? String(llmStatus.ai_model_context_limit)
				: "",
		);
		setLlmSettingsDialogOpen(true);
	}, [llmStatus]);

	const saveLlmSettings = useCallback(async () => {
		const maxConcurrency = parsePositiveIntegerInput(llmMaxConcurrencyInput);
		if (!maxConcurrency) {
			setLlmSettingsSaveError("并发上限必须是大于 0 的整数。");
			return;
		}
		const normalizedModelContextLimit = llmModelContextLimitInput.trim();
		const aiModelContextLimit =
			normalizedModelContextLimit.length === 0
				? null
				: parsePositiveIntegerInput(normalizedModelContextLimit);
		if (normalizedModelContextLimit.length > 0 && !aiModelContextLimit) {
			setLlmSettingsSaveError(
				"LLM 输入长度上限必须留空，或填写大于 0 的整数。",
			);
			return;
		}

		setLlmSettingsSaving(true);
		setLlmSettingsSaveError(null);
		try {
			const nextStatus = await apiPatchAdminLlmRuntimeConfig({
				max_concurrency: maxConcurrency,
				ai_model_context_limit: aiModelContextLimit,
			});
			setLlmStatus(nextStatus);
			setLlmSettingsDialogOpen(false);
		} catch (err) {
			setLlmSettingsSaveError(normalizeErrorMessage(err));
		} finally {
			setLlmSettingsSaving(false);
		}
	}, [llmMaxConcurrencyInput, llmModelContextLimitInput]);

	const loadLlmCalls = useCallback(
		async (options?: LoadOptions) => {
			if (
				options?.background &&
				!llmCallsLoadedOnceRef.current &&
				llmCallsInitialRequestInFlightRef.current
			) {
				return;
			}
			const requestId = llmCallsRequestIdRef.current + 1;
			llmCallsRequestIdRef.current = requestId;
			llmCallsInitialRequestInFlightRef.current =
				!llmCallsLoadedOnceRef.current;
			setLlmCallsLoadPhase(
				resolveListLoadPhase(llmCallsLoadedOnceRef.current, options),
			);
			try {
				const params = new URLSearchParams();
				params.set("status", llmStatusFilter);
				params.set("sort", "status_grouped");
				params.set("page", String(llmCallPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				if (llmSourceFilter.trim()) {
					params.set("source", llmSourceFilter.trim());
				}
				const requestedBy = llmRequestedByFilter.trim();
				if (requestedBy) {
					params.set("requested_by", requestedBy);
				}
				const startedFromUtc = localInputToUtc(llmStartedFromFilter);
				if (startedFromUtc) {
					params.set("started_from", startedFromUtc);
				}
				const startedToUtc = localInputToUtc(llmStartedToFilter);
				if (startedToUtc) {
					params.set("started_to", startedToUtc);
				}
				const res = await apiGetAdminLlmCalls(params);
				if (requestId !== llmCallsRequestIdRef.current) {
					return;
				}
				setLlmCalls(sortLlmCallsForDisplay(res.items));
				setLlmCallTotal(res.total);
				llmCallsLoadedOnceRef.current = true;
			} catch (err) {
				if (requestId !== llmCallsRequestIdRef.current) {
					return;
				}
				throw err;
			} finally {
				if (requestId === llmCallsRequestIdRef.current) {
					setLlmCallsLoadPhase("idle");
					llmCallsInitialRequestInFlightRef.current = false;
				}
			}
		},
		[
			llmStatusFilter,
			llmCallPage,
			llmSourceFilter,
			llmRequestedByFilter,
			llmStartedFromFilter,
			llmStartedToFilter,
		],
	);

	const loadAll = useCallback(
		async (options?: LoadOptions) => {
			setError(null);
			try {
				await Promise.all([
					loadOverview({ background: true }),
					loadRealtimeTasks(options),
					loadScheduledRuns(options),
					loadLlmSchedulerStatus(options),
					loadLlmCalls(options),
				]);
			} catch (err) {
				setError(normalizeErrorMessage(err));
			}
		},
		[
			loadOverview,
			loadRealtimeTasks,
			loadScheduledRuns,
			loadLlmSchedulerStatus,
			loadLlmCalls,
		],
	);

	const refreshTaskDetail = useCallback(async (taskId: string) => {
		const detail = await apiGetAdminRealtimeTaskDetail(taskId);
		setDetailTask(detail);
	}, []);

	const refreshLlmDetail = useCallback(async (callId: string) => {
		const detail = await apiGetAdminLlmCallDetail(callId);
		if (activeTaskDrawerLlmCallIdRef.current === callId) {
			setTaskDrawerLlmDetail(detail);
			return;
		}
		setLlmDetail(detail);
	}, []);

	const loadTaskRelatedLlmCalls = useCallback(async (taskId: string) => {
		const params = new URLSearchParams();
		params.set("status", "all");
		params.set("sort", "created_desc");
		params.set("page", "1");
		params.set("page_size", "50");
		params.set("parent_task_id", taskId);
		const res = await apiGetAdminLlmCalls(params);
		setTaskRelatedLlmCalls(res.items);
	}, []);

	const loadTaskDrawerLlmDetail = useCallback(async (callId: string) => {
		const detail = await apiGetAdminLlmCallDetail(callId);
		setTaskDrawerLlmDetail(detail);
	}, []);

	const drainStreamRefreshQueue = useCallback(async () => {
		if (streamRefreshInFlightRef.current) {
			return;
		}
		streamRefreshInFlightRef.current = true;
		try {
			const needFullRefresh = streamPendingFullRefreshRef.current;
			const needLlmRefresh = streamPendingLlmRefreshRef.current;
			const needTranslationRefresh = streamPendingTranslationRefreshRef.current;
			const pendingDetailTaskId = streamPendingDetailTaskIdRef.current;
			const pendingLlmDetailCallId = streamPendingLlmDetailCallIdRef.current;
			streamPendingFullRefreshRef.current = false;
			streamPendingLlmRefreshRef.current = false;
			streamPendingTranslationRefreshRef.current = false;
			streamPendingDetailTaskIdRef.current = null;
			streamPendingLlmDetailCallIdRef.current = null;

			if (needFullRefresh) {
				await Promise.all([
					loadOverview({ background: true }),
					loadRealtimeTasks({ background: true }),
					loadScheduledRuns({ background: true }),
					loadLlmSchedulerStatus({ background: true }),
					loadLlmCalls({ background: true }),
				]);
				const activeDetailTaskId = detailTaskIdRef.current;
				if (activeDetailTaskId) {
					await refreshTaskDetail(activeDetailTaskId);
				}
				const activeLlmDetailId = llmDetailIdRef.current;
				if (activeLlmDetailId) {
					await refreshLlmDetail(activeLlmDetailId);
				}
				if (needTranslationRefresh) {
					setRefreshNonce((prev) => prev + 1);
				}
				return;
			}

			if (needLlmRefresh) {
				await Promise.all([
					loadLlmSchedulerStatus({ background: true }),
					loadLlmCalls({ background: true }),
				]);
			}

			if (needTranslationRefresh) {
				setRefreshNonce((prev) => prev + 1);
			}

			if (pendingDetailTaskId) {
				await refreshTaskDetail(pendingDetailTaskId);
			}

			if (pendingLlmDetailCallId) {
				await refreshLlmDetail(pendingLlmDetailCallId);
			}
		} catch (err) {
			setError(normalizeErrorMessage(err));
		} finally {
			streamRefreshInFlightRef.current = false;
			if (
				streamPendingFullRefreshRef.current ||
				streamPendingLlmRefreshRef.current ||
				streamPendingTranslationRefreshRef.current ||
				streamPendingDetailTaskIdRef.current ||
				streamPendingLlmDetailCallIdRef.current
			) {
				void drainStreamRefreshQueue();
			}
		}
	}, [
		loadOverview,
		loadRealtimeTasks,
		loadScheduledRuns,
		loadLlmSchedulerStatus,
		loadLlmCalls,
		refreshTaskDetail,
		refreshLlmDetail,
	]);

	const scheduleStreamRefresh = useCallback(
		(
			mode: "all" | "detail" | "llm" | "llm_detail" | "translation",
			id?: string,
		) => {
			if (mode === "all") {
				streamPendingFullRefreshRef.current = true;
				streamPendingDetailTaskIdRef.current = null;
				streamPendingLlmRefreshRef.current = false;
				streamPendingLlmDetailCallIdRef.current = null;
			} else if (mode === "detail" && id) {
				streamPendingDetailTaskIdRef.current = id;
			} else if (mode === "llm") {
				streamPendingLlmRefreshRef.current = true;
			} else if (mode === "llm_detail" && id) {
				streamPendingLlmRefreshRef.current = true;
				streamPendingLlmDetailCallIdRef.current = id;
			} else if (mode === "translation") {
				streamPendingTranslationRefreshRef.current = true;
			}

			if (streamRefreshTimerRef.current !== null) {
				return;
			}
			streamRefreshTimerRef.current = window.setTimeout(() => {
				streamRefreshTimerRef.current = null;
				void drainStreamRefreshQueue();
			}, STREAM_REFRESH_DELAY_MS);
		},
		[drainStreamRefreshQueue],
	);

	useEffect(() => {
		setError(null);
		void loadOverview().catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadOverview]);

	useEffect(() => {
		setError(null);
		const options = tasksLoadedOnceRef.current
			? { background: true }
			: undefined;
		void loadRealtimeTasks(options).catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadRealtimeTasks]);

	useEffect(() => {
		setError(null);
		const options = scheduledRunsLoadedOnceRef.current
			? { background: true }
			: undefined;
		void loadScheduledRuns(options).catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadScheduledRuns]);

	useEffect(() => {
		setError(null);
		const llmStatusOptions = llmStatusLoadedOnceRef.current
			? { background: true }
			: undefined;
		const llmCallOptions = llmCallsLoadedOnceRef.current
			? { background: true }
			: undefined;
		void Promise.all([
			loadLlmSchedulerStatus(llmStatusOptions),
			loadLlmCalls(llmCallOptions),
		]).catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadLlmSchedulerStatus, loadLlmCalls]);

	useEffect(() => {
		if (!activeTaskDrawerTaskId) {
			setDetailTask(null);
			setDetailLoading(false);
			return;
		}
		if (detailTask?.task.id === activeTaskDrawerTaskId) {
			return;
		}
		let canceled = false;
		setDetailLoading(true);
		setError(null);
		void refreshTaskDetail(activeTaskDrawerTaskId)
			.catch((err) => {
				if (!canceled) {
					setError(normalizeErrorMessage(err));
				}
			})
			.finally(() => {
				if (!canceled) {
					setDetailLoading(false);
				}
			});
		return () => {
			canceled = true;
		};
	}, [activeTaskDrawerTaskId, detailTask?.task.id, refreshTaskDetail]);

	useEffect(() => {
		if (!activeTaskDrawerTaskId) {
			setTaskRelatedLlmCalls([]);
			setTaskRelatedLlmLoading(false);
			return;
		}
		let canceled = false;
		setTaskRelatedLlmLoading(true);
		void loadTaskRelatedLlmCalls(activeTaskDrawerTaskId)
			.catch((err) => {
				if (!canceled) {
					setError(normalizeErrorMessage(err));
				}
			})
			.finally(() => {
				if (!canceled) {
					setTaskRelatedLlmLoading(false);
				}
			});
		return () => {
			canceled = true;
		};
	}, [activeTaskDrawerTaskId, loadTaskRelatedLlmCalls]);

	useEffect(() => {
		if (!activeTaskDrawerLlmCallId) {
			setTaskDrawerLlmDetail(null);
			setTaskDrawerLlmLoading(false);
			return;
		}
		if (taskDrawerLlmDetail?.id === activeTaskDrawerLlmCallId) {
			return;
		}
		let canceled = false;
		setTaskDrawerLlmLoading(true);
		setError(null);
		void loadTaskDrawerLlmDetail(activeTaskDrawerLlmCallId)
			.catch((err) => {
				if (!canceled) {
					setError(normalizeErrorMessage(err));
				}
			})
			.finally(() => {
				if (!canceled) {
					setTaskDrawerLlmLoading(false);
				}
			});
		return () => {
			canceled = true;
		};
	}, [
		activeTaskDrawerLlmCallId,
		loadTaskDrawerLlmDetail,
		taskDrawerLlmDetail?.id,
	]);

	useEffect(() => {
		return () => {
			if (streamRefreshTimerRef.current !== null) {
				window.clearTimeout(streamRefreshTimerRef.current);
				streamRefreshTimerRef.current = null;
			}
		};
	}, []);

	useEffect(() => {
		if (
			typeof window === "undefined" ||
			typeof window.EventSource === "undefined"
		) {
			setStreamStatus("reconnecting");
			return;
		}

		let disposed = false;
		let reconnectTimer: number | null = null;
		let source: EventSource | null = null;

		const closeSource = () => {
			if (source) {
				source.close();
				source = null;
			}
		};

		const connect = () => {
			if (disposed) return;
			setStreamStatus((prev) =>
				prev === "connected" || prev === "reconnecting"
					? "reconnecting"
					: "connecting",
			);
			const nextSource = apiOpenAdminJobsEventsStream();
			source = nextSource;

			const onJobEvent = (evt: Event) => {
				const message = evt as MessageEvent<string>;
				let parsed: AdminJobsStreamEvent | null = null;
				try {
					parsed = JSON.parse(message.data) as AdminJobsStreamEvent;
				} catch {
					parsed = null;
				}
				if (!parsed) {
					scheduleStreamRefresh("all");
					return;
				}

				if (parsed.event_type === "task.progress") {
					const activeDetailTaskId = detailTaskIdRef.current;
					if (activeDetailTaskId && activeDetailTaskId === parsed.task_id) {
						scheduleStreamRefresh("detail", activeDetailTaskId);
					}
					return;
				}

				scheduleStreamRefresh("all");
			};
			const onLlmCallEvent = (evt: Event) => {
				const message = evt as MessageEvent<string>;
				let parsed: AdminLlmCallStreamEvent | null = null;
				try {
					parsed = JSON.parse(message.data) as AdminLlmCallStreamEvent;
				} catch {
					parsed = null;
				}
				if (!parsed) {
					scheduleStreamRefresh("llm");
					return;
				}

				const activeLlmDetailId = llmDetailIdRef.current;
				if (activeLlmDetailId && activeLlmDetailId === parsed.call_id) {
					scheduleStreamRefresh("llm_detail", activeLlmDetailId);
					return;
				}

				scheduleStreamRefresh("llm");
			};
			const onLlmSchedulerEvent = () => {
				scheduleStreamRefresh("llm");
			};
			const onTranslationEvent = (evt: Event) => {
				const message = evt as MessageEvent<string>;
				let parsed: AdminTranslationStreamEvent | null = null;
				try {
					parsed = JSON.parse(message.data) as AdminTranslationStreamEvent;
				} catch {
					parsed = null;
				}
				if (!parsed) {
					scheduleStreamRefresh("translation");
					return;
				}

				scheduleStreamRefresh("translation");
			};

			nextSource.addEventListener("job.event", onJobEvent as EventListener);
			nextSource.addEventListener("llm.call", onLlmCallEvent as EventListener);
			nextSource.addEventListener(
				"llm.scheduler",
				onLlmSchedulerEvent as EventListener,
			);
			nextSource.addEventListener(
				"translation.event",
				onTranslationEvent as EventListener,
			);
			nextSource.onopen = () => {
				if (disposed) return;
				setStreamStatus("connected");
			};
			nextSource.onerror = () => {
				nextSource.removeEventListener(
					"job.event",
					onJobEvent as EventListener,
				);
				nextSource.removeEventListener(
					"llm.call",
					onLlmCallEvent as EventListener,
				);
				nextSource.removeEventListener(
					"llm.scheduler",
					onLlmSchedulerEvent as EventListener,
				);
				nextSource.removeEventListener(
					"translation.event",
					onTranslationEvent as EventListener,
				);
				closeSource();
				if (disposed) return;
				setStreamStatus("reconnecting");
				if (reconnectTimer !== null) {
					window.clearTimeout(reconnectTimer);
				}
				reconnectTimer = window.setTimeout(() => {
					reconnectTimer = null;
					connect();
				}, STREAM_RECONNECT_DELAY_MS);
			};
		};

		connect();

		return () => {
			disposed = true;
			if (reconnectTimer !== null) {
				window.clearTimeout(reconnectTimer);
				reconnectTimer = null;
			}
			closeSource();
		};
	}, [scheduleStreamRefresh]);

	const onOpenTaskDetail = useCallback(
		(taskId: string) => {
			setError(null);
			setLlmDetail(null);
			navigateAdminJobsRoute({
				primaryTab: tab,
				translationView,
				taskDrawerRoute: { taskId, llmCallId: null },
				drawerFromTab: tab,
			});
		},
		[navigateAdminJobsRoute, tab, translationView],
	);

	const onOpenLlmCallDetail = useCallback(async (callId: string) => {
		setLlmDetailLoading(true);
		setError(null);
		try {
			const detail = await apiGetAdminLlmCallDetail(callId);
			setLlmDetail(detail);
		} catch (err) {
			setError(normalizeErrorMessage(err));
		} finally {
			setLlmDetailLoading(false);
		}
	}, []);

	const onOpenTaskLlmDetail = useCallback(
		(callId: string) => {
			if (!activeTaskDrawerTaskId) return;
			setError(null);
			navigateAdminJobsRoute({
				primaryTab: tab,
				translationView,
				taskDrawerRoute: {
					taskId: activeTaskDrawerTaskId,
					llmCallId: callId,
				},
				drawerFromTab: taskDrawerFromTab,
			});
		},
		[
			activeTaskDrawerTaskId,
			navigateAdminJobsRoute,
			tab,
			taskDrawerFromTab,
			translationView,
		],
	);

	const onCloseTaskDrawer = useCallback(() => {
		navigateAdminJobsRoute({
			primaryTab: taskDrawerFromTab ?? "realtime",
			translationView,
			taskDrawerRoute: null,
			drawerFromTab: null,
		});
	}, [navigateAdminJobsRoute, taskDrawerFromTab, translationView]);

	const onBackToTaskDetail = useCallback(() => {
		if (!activeTaskDrawerTaskId) return;
		navigateAdminJobsRoute({
			primaryTab: tab,
			translationView,
			taskDrawerRoute: {
				taskId: activeTaskDrawerTaskId,
				llmCallId: null,
			},
			drawerFromTab: taskDrawerFromTab,
		});
	}, [
		activeTaskDrawerTaskId,
		navigateAdminJobsRoute,
		tab,
		taskDrawerFromTab,
		translationView,
	]);

	const onOpenParentTaskFromLlm = useCallback(
		(taskId: string | null) => {
			if (!taskId) return;
			setLlmDetail(null);
			onOpenTaskDetail(taskId);
		},
		[onOpenTaskDetail],
	);

	const onRetryTask = useCallback(
		async (taskId: string) => {
			setTaskActionBusyId(taskId);
			setError(null);
			try {
				await apiRetryAdminRealtimeTask(taskId);
				await loadAll({ background: true });
			} catch (err) {
				setError(normalizeErrorMessage(err));
			} finally {
				setTaskActionBusyId(null);
			}
		},
		[loadAll],
	);

	const onCancelTask = useCallback(
		async (taskId: string) => {
			setTaskActionBusyId(taskId);
			setError(null);
			try {
				await apiCancelAdminRealtimeTask(taskId);
				await loadAll({ background: true });
			} catch (err) {
				setError(normalizeErrorMessage(err));
			} finally {
				setTaskActionBusyId(null);
			}
		},
		[loadAll],
	);

	const streamLabel =
		streamStatus === "connected"
			? "SSE 已连接"
			: streamStatus === "reconnecting"
				? "SSE 重连中..."
				: "SSE 连接中...";
	const streamTone = streamStatusTone(streamStatus);
	const activeDetailTone = detailTaskTone ?? taskStatusTone("");
	const activeTaskDrawerLlmTone = taskDrawerLlmDetail
		? taskStatusTone(taskDrawerLlmDetail.status)
		: null;
	const openLlmDetailTone = llmDetail ? taskStatusTone(llmDetail.status) : null;
	const cancelRequestedBadgeClass =
		"border-amber-300 bg-amber-100 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100";

	return (
		<div className="space-y-4">
			<Card>
				<CardHeader>
					<CardTitle>任务总览</CardTitle>
					<CardDescription>
						展示实时异步任务与定时任务运行概览。
					</CardDescription>
				</CardHeader>
				<CardContent className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">队列中</p>
						<p className="mt-1 text-xl font-semibold">
							{formatCount(overview?.queued)}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">运行中</p>
						<p className="mt-1 text-xl font-semibold">
							{formatCount(overview?.running)}
						</p>
					</div>
					<div className="bg-card/70 rounded-lg border p-3">
						<p className="text-muted-foreground text-xs">近24h 成功 / 失败</p>
						<p className="mt-1 text-xl font-semibold">
							{formatCount(overview?.succeeded_24h)} /{" "}
							{formatCount(overview?.failed_24h)}
						</p>
					</div>
				</CardContent>
			</Card>

			<Tabs
				value={tab}
				onValueChange={(nextValue) =>
					navigateAdminJobsRoute({
						primaryTab: nextValue as AdminJobsPrimaryTab,
						translationView,
						taskDrawerRoute: null,
						drawerFromTab: null,
					})
				}
				className="space-y-4"
			>
				<div className="flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
					<TabsList className="grid w-full grid-cols-4 sm:inline-flex sm:w-auto">
						<TabsTrigger value="realtime" className="font-mono text-xs">
							实时异步任务
						</TabsTrigger>
						<TabsTrigger value="scheduled" className="font-mono text-xs">
							定时任务
						</TabsTrigger>
						<TabsTrigger value="llm" className="font-mono text-xs">
							LLM调度
						</TabsTrigger>
						<TabsTrigger value="translations" className="font-mono text-xs">
							翻译调度
						</TabsTrigger>
					</TabsList>
					<div className="flex flex-wrap items-center gap-2">
						<Button
							variant="secondary"
							size="sm"
							disabled={isRefreshingData}
							onClick={() => {
								setRefreshNonce((prev) => prev + 1);
								void loadAll({ background: true });
							}}
						>
							刷新
						</Button>
						<StatusBadge label={streamLabel} tone={streamTone} />
					</div>
				</div>

				{error ? <p className="text-destructive text-sm">{error}</p> : null}

				<TabsContent value="realtime">
					<Card>
						<CardHeader>
							<div className="flex items-center gap-2">
								<CardTitle>实时异步任务</CardTitle>
								<Tooltip>
									<TooltipTrigger asChild>
										<Button
											type="button"
											variant="ghost"
											size="icon"
											className="text-muted-foreground size-7 rounded-full"
										>
											<CircleHelp className="size-4" />
											<span className="sr-only">实时异步任务说明</span>
										</Button>
									</TooltipTrigger>
									<TooltipContent side="right" sideOffset={6}>
										监控系统内部任务，并支持重试与取消。
									</TooltipContent>
								</Tooltip>
							</div>
							<CardDescription>
								按状态查看实时任务队列，并保留当前用户上下文与详情入口。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
								<FilterSelect
									value={statusFilter}
									onValueChange={(nextValue) => {
										setTaskPage(1);
										setStatusFilter(nextValue);
									}}
									options={REALTIME_STATUS_FILTER_OPTIONS}
									placeholder="状态筛选"
									ariaLabel="实时异步任务状态筛选"
									className="w-full sm:w-[220px]"
								/>
								<span className="text-muted-foreground text-xs">
									共 {formatCount(taskTotal)} 个任务 · 当前用户 #{currentUserId}
								</span>
							</div>

							<div className="space-y-2">
								{tasksRefreshing ? (
									<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
										<span className="size-2 rounded-full bg-amber-500/80" />
										任务列表更新中...
									</p>
								) : null}
								{tasksInitialLoading ? (
									<LoadingMessage>正在加载任务...</LoadingMessage>
								) : tasks.length === 0 ? (
									<p className="text-muted-foreground text-sm">暂无任务。</p>
								) : (
									tasks.map((task) => {
										const busy = taskActionBusyId === task.id;
										const tone = taskStatusTone(task.status);
										return (
											<div
												key={task.id}
												className={`bg-card/70 flex flex-col gap-3 rounded-lg border border-l-4 p-3 transition-colors duration-200 hover:bg-card/90 lg:flex-row lg:items-center lg:justify-between ${tone.cardAccentClass}`}
											>
												<div className="min-w-0">
													<div className="flex flex-wrap items-center gap-2">
														<p className="font-medium text-sm">
															{taskTypeLabel(task.task_type)}
														</p>
														<StatusBadge
															label={taskStatusLabel(task.status)}
															tone={tone}
														/>
														{task.cancel_requested ? (
															<FlagBadge
																label="已请求取消"
																className={cancelRequestedBadgeClass}
															/>
														) : null}
													</div>
													<p className="text-muted-foreground mt-1 text-xs">
														类型：
														<span className="font-mono">{task.task_type}</span>
													</p>
													<p className="text-muted-foreground mt-1 truncate font-mono text-[11px]">
														ID: {task.id}
													</p>
													<div className="mt-1 flex flex-wrap gap-1.5 text-xs">
														<span className="bg-muted/60 rounded px-2 py-0.5">
															创建 {formatLocalHm(task.created_at)}
														</span>
														<span className="bg-muted/60 rounded px-2 py-0.5">
															开始 {formatLocalHm(task.started_at)}
														</span>
														<span className="bg-muted/60 rounded px-2 py-0.5">
															完成 {formatLocalHm(task.finished_at)}
														</span>
													</div>
													{task.error_message ? (
														<p className="text-destructive mt-1 text-xs font-medium">
															失败原因：{task.error_message}
														</p>
													) : null}
												</div>
												<div className="flex flex-wrap gap-2">
													<Button
														variant="outline"
														disabled={tasksActionsDisabled}
														onClick={() => void onOpenTaskDetail(task.id)}
													>
														详情
													</Button>
													<Button
														variant="outline"
														disabled={
															tasksActionsDisabled ||
															busy ||
															task.status === "queued" ||
															task.status === "running"
														}
														onClick={() => void onRetryTask(task.id)}
													>
														重试
													</Button>
													<Button
														variant="destructive"
														disabled={
															tasksActionsDisabled ||
															busy ||
															task.status === "succeeded" ||
															task.status === "failed" ||
															task.status === "canceled"
														}
														onClick={() => void onCancelTask(task.id)}
													>
														取消
													</Button>
												</div>
											</div>
										);
									})
								)}
							</div>

							<div className="flex items-center justify-between">
								<p className="text-muted-foreground text-xs">
									第 {taskPage}/{taskTotalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										variant="outline"
										size="sm"
										disabled={taskPage <= 1 || tasksLoadPhase !== "idle"}
										onClick={() => setTaskPage((prev) => Math.max(1, prev - 1))}
									>
										上一页
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={
											taskPage >= taskTotalPages || tasksLoadPhase !== "idle"
										}
										onClick={() =>
											setTaskPage((prev) => Math.min(taskTotalPages, prev + 1))
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				</TabsContent>

				<TabsContent value="scheduled">
					<Card>
						<CardHeader>
							<CardTitle>定时任务</CardTitle>
							<CardDescription>
								查看 `brief.daily_slot` 运行记录，并保留重试/取消与详情联动。
							</CardDescription>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:justify-between">
								<FilterSelect
									value={scheduledRunStatusFilter}
									onValueChange={(nextValue) => {
										setScheduledRunPage(1);
										setScheduledRunStatusFilter(nextValue);
									}}
									options={REALTIME_STATUS_FILTER_OPTIONS}
									placeholder="状态筛选"
									ariaLabel="定时任务状态筛选"
									className="w-full sm:w-[220px]"
								/>
								<p className="text-muted-foreground text-xs">
									共 {formatCount(scheduledRunTotal)} 条
								</p>
							</div>

							<div className="space-y-2">
								{scheduledRunsRefreshing ? (
									<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
										<span className="size-2 rounded-full bg-amber-500/80" />
										运行记录更新中...
									</p>
								) : null}
								{scheduledRunsInitialLoading ? (
									<LoadingMessage>正在加载运行记录...</LoadingMessage>
								) : scheduledRuns.length === 0 ? (
									<p className="text-muted-foreground text-sm">
										暂无运行记录。
									</p>
								) : (
									scheduledRuns.map((task) => {
										const busy = taskActionBusyId === task.id;
										const tone = taskStatusTone(task.status);
										return (
											<div
												key={task.id}
												className={`bg-card/70 flex flex-col gap-3 rounded-lg border border-l-4 p-3 transition-colors duration-200 hover:bg-card/90 lg:flex-row lg:items-center lg:justify-between ${tone.cardAccentClass}`}
											>
												<div className="min-w-0">
													<div className="flex flex-wrap items-center gap-2">
														<p className="font-medium text-sm">
															{taskTypeLabel(task.task_type)}
														</p>
														<StatusBadge
															label={taskStatusLabel(task.status)}
															tone={tone}
														/>
														{task.cancel_requested ? (
															<FlagBadge
																label="已请求取消"
																className={cancelRequestedBadgeClass}
															/>
														) : null}
													</div>
													<p className="text-muted-foreground mt-1 text-xs">
														类型：
														<span className="font-mono">{task.task_type}</span>
													</p>
													<p className="text-muted-foreground mt-1 truncate font-mono text-[11px]">
														ID: {task.id}
													</p>
													<div className="mt-1 flex flex-wrap gap-1.5 text-xs">
														<span className="bg-muted/60 rounded px-2 py-0.5">
															创建 {formatLocalHm(task.created_at)}
														</span>
														<span className="bg-muted/60 rounded px-2 py-0.5">
															完成 {formatLocalHm(task.finished_at)}
														</span>
													</div>
													{task.error_message ? (
														<p className="text-destructive mt-1 text-xs font-medium">
															失败原因：{task.error_message}
														</p>
													) : null}
												</div>
												<div className="flex flex-wrap gap-2">
													<Button
														variant="outline"
														disabled={scheduledRunActionsDisabled}
														onClick={() => void onOpenTaskDetail(task.id)}
													>
														详情
													</Button>
													<Button
														variant="outline"
														disabled={
															scheduledRunActionsDisabled ||
															busy ||
															task.status === "queued" ||
															task.status === "running"
														}
														onClick={() => void onRetryTask(task.id)}
													>
														重试
													</Button>
													<Button
														variant="destructive"
														disabled={
															scheduledRunActionsDisabled ||
															busy ||
															task.status === "succeeded" ||
															task.status === "failed" ||
															task.status === "canceled"
														}
														onClick={() => void onCancelTask(task.id)}
													>
														取消
													</Button>
												</div>
											</div>
										);
									})
								)}
							</div>

							<div className="flex items-center justify-between">
								<p className="text-muted-foreground text-xs">
									第 {scheduledRunPage}/{scheduledRunTotalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										variant="outline"
										size="sm"
										disabled={
											scheduledRunPage <= 1 || scheduledRunsLoadPhase !== "idle"
										}
										onClick={() =>
											setScheduledRunPage((prev) => Math.max(1, prev - 1))
										}
									>
										上一页
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={
											scheduledRunPage >= scheduledRunTotalPages ||
											scheduledRunsLoadPhase !== "idle"
										}
										onClick={() =>
											setScheduledRunPage((prev) =>
												Math.min(scheduledRunTotalPages, prev + 1),
											)
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				</TabsContent>

				<TabsContent value="llm">
					<Card>
						<CardHeader className="flex flex-row items-start justify-between gap-4">
							<div className="space-y-1.5">
								<CardTitle>LLM 调度</CardTitle>
								<CardDescription>
									查看调度状态与调用级日志，支持按状态/来源/用户/时间筛选。
								</CardDescription>
							</div>
							<Button
								type="button"
								variant="outline"
								size="icon"
								aria-label="配置 LLM 运行参数"
								onClick={openLlmSettingsDialog}
								disabled={!llmStatus || llmSettingsSaving}
							>
								<Settings2 />
							</Button>
						</CardHeader>
						<CardContent className="space-y-4">
							<div className="max-w-sm">
								<div className="bg-card/70 rounded-lg border p-3">
									<p className="text-muted-foreground text-xs">
										近24h 调用 / 失败
									</p>
									<p className="mt-1 text-sm font-semibold">
										{formatCount(llmStatus?.calls_24h)} /{" "}
										{formatCount(llmStatus?.failed_24h)}
									</p>
									<p className="text-muted-foreground mt-1 text-xs">
										平均等待{" "}
										{formatDurationMs(llmStatus?.avg_wait_ms_24h ?? null)}
									</p>
									<p className="text-muted-foreground mt-1 text-xs">
										并发上限 {formatCount(llmStatus?.max_concurrency)} · 可用{" "}
										{formatCount(llmStatus?.available_slots)} · 输入{" "}
										{formatCount(llmStatus?.effective_model_input_limit)} tokens
									</p>
								</div>
							</div>

							<div className="grid gap-2 lg:grid-cols-[minmax(0,220px)_minmax(0,1fr)_minmax(0,1fr)_minmax(0,1.35fr)]">
								<FilterSelect
									value={llmStatusFilter}
									onValueChange={(nextValue) => {
										setLlmCallPage(1);
										setLlmStatusFilter(nextValue);
									}}
									options={LLM_STATUS_FILTER_OPTIONS}
									placeholder="状态筛选"
									ariaLabel="LLM 调用状态筛选"
								/>
								<Input
									value={llmSourceFilter}
									onChange={(event) => {
										setLlmCallPage(1);
										setLlmSourceFilter(event.target.value);
									}}
									placeholder="来源（source）"
									aria-label="LLM 调用来源筛选"
								/>
								<Input
									value={llmRequestedByFilter}
									onChange={(event) => {
										setLlmCallPage(1);
										setLlmRequestedByFilter(event.target.value);
									}}
									placeholder="用户 NanoID（requested_by）"
									aria-label="LLM 调用用户筛选"
								/>
								<div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
									<Input
										type="datetime-local"
										value={llmStartedFromFilter}
										onChange={(event) => {
											setLlmCallPage(1);
											setLlmStartedFromFilter(event.target.value);
										}}
										aria-label="LLM 开始时间下限"
										className="text-xs"
									/>
									<Input
										type="datetime-local"
										value={llmStartedToFilter}
										onChange={(event) => {
											setLlmCallPage(1);
											setLlmStartedToFilter(event.target.value);
										}}
										aria-label="LLM 开始时间上限"
										className="text-xs"
									/>
								</div>
							</div>

							<p className="text-muted-foreground text-xs">
								共 {formatCount(llmCallTotal)} 条调用
							</p>

							<div className="space-y-2">
								{llmRefreshing ? (
									<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
										<span className="size-2 rounded-full bg-amber-500/80" />
										LLM 调度更新中...
									</p>
								) : null}
								{llmCallsInitialLoading ? (
									<LoadingMessage>正在加载调用记录...</LoadingMessage>
								) : llmCalls.length === 0 ? (
									<p className="text-muted-foreground text-sm">
										暂无调用记录。
									</p>
								) : (
									llmCalls.map((call) => {
										const tone = taskStatusTone(call.status);
										return (
											<div
												key={call.id}
												className={`bg-card/70 flex flex-col gap-3 rounded-lg border border-l-4 p-3 transition-colors duration-200 hover:bg-card/90 lg:flex-row lg:items-center lg:justify-between ${tone.cardAccentClass}`}
											>
												<div className="min-w-0">
													<div className="flex flex-wrap items-center gap-2">
														<p className="font-medium text-sm">{call.source}</p>
														<StatusBadge
															label={taskStatusLabel(call.status)}
															tone={tone}
														/>
													</div>
													<p className="text-muted-foreground mt-1 text-xs">
														模型：
														<span className="font-mono">{call.model}</span>
													</p>
													<p className="text-muted-foreground mt-1 text-xs">
														用户：{call.requested_by ?? "-"} · 重试次数：
														{formatCount(call.attempt_count)}
													</p>
													<p className="text-muted-foreground mt-1 text-xs">
														等待 {formatDurationMs(call.scheduler_wait_ms)} ·
														首字 {formatDurationMs(call.first_token_wait_ms)} ·
														耗时 {formatDurationMs(call.duration_ms)}
													</p>
													<p className="text-muted-foreground mt-1 text-xs">
														Token 输入/输出/缓存：
														{formatCount(call.input_tokens)} /{" "}
														{formatCount(call.output_tokens)} /{" "}
														{formatCount(call.cached_input_tokens)}
													</p>
													<p className="text-muted-foreground mt-1 truncate font-mono text-[11px]">
														ID: {call.id}
													</p>
												</div>
												<div className="flex flex-wrap gap-2">
													<Button
														variant="outline"
														disabled={llmCallActionsDisabled}
														onClick={() => void onOpenLlmCallDetail(call.id)}
													>
														详情
													</Button>
												</div>
											</div>
										);
									})
								)}
							</div>

							<div className="flex items-center justify-between">
								<p className="text-muted-foreground text-xs">
									第 {llmCallPage}/{llmCallTotalPages} 页
								</p>
								<div className="flex gap-2">
									<Button
										variant="outline"
										size="sm"
										disabled={llmCallPage <= 1 || llmCallsLoadPhase !== "idle"}
										onClick={() =>
											setLlmCallPage((prev) => Math.max(1, prev - 1))
										}
									>
										上一页
									</Button>
									<Button
										variant="outline"
										size="sm"
										disabled={
											llmCallPage >= llmCallTotalPages ||
											llmCallsLoadPhase !== "idle"
										}
										onClick={() =>
											setLlmCallPage((prev) =>
												Math.min(llmCallTotalPages, prev + 1),
											)
										}
									>
										下一页
									</Button>
								</div>
							</div>
						</CardContent>
					</Card>
				</TabsContent>

				<TabsContent value="translations">
					<TranslationSchedulerSection
						viewTab={translationView}
						onViewTabChange={(nextValue) =>
							navigateAdminJobsRoute({
								primaryTab: "translations",
								translationView: nextValue,
								taskDrawerRoute: null,
								drawerFromTab: null,
							})
						}
						refreshNonce={refreshNonce}
						onOpenLlmCallDetail={(callId) => void onOpenLlmCallDetail(callId)}
					/>
				</TabsContent>
			</Tabs>

			<Dialog
				open={llmSettingsDialogOpen}
				onOpenChange={(open) => {
					setLlmSettingsDialogOpen(open);
					if (!open) {
						setLlmSettingsSaveError(null);
					}
				}}
			>
				<DialogContent className="max-w-md">
					<DialogHeader>
						<DialogTitle>配置 LLM 运行参数</DialogTitle>
						<DialogDescription>
							保存后立即生效；并发上限控制同时 in-flight
							调用数，输入长度上限为空时会自动跟随当前模型能力，后续翻译分块会据此计算。
						</DialogDescription>
					</DialogHeader>

					<div className="space-y-4">
						<div className="space-y-2">
							<Label htmlFor="llm-max-concurrency">最大并发数</Label>
							<Input
								id="llm-max-concurrency"
								type="number"
								min={1}
								step={1}
								inputMode="numeric"
								value={llmMaxConcurrencyInput}
								onChange={(event) =>
									setLlmMaxConcurrencyInput(event.target.value)
								}
							/>
						</div>
						<div className="space-y-2">
							<Label htmlFor="llm-model-context-limit">
								LLM 输入长度上限（tokens）
							</Label>
							<Input
								id="llm-model-context-limit"
								type="number"
								min={1}
								step={1}
								inputMode="numeric"
								placeholder="留空则自动跟随模型能力"
								value={llmModelContextLimitInput}
								onChange={(event) =>
									setLlmModelContextLimitInput(event.target.value)
								}
							/>
							<p className="text-muted-foreground text-xs">
								留空时按当前模型目录自动推导；当前生效{" "}
								{formatCount(llmStatus?.effective_model_input_limit)} tokens（
								{formatModelInputLimitSource(
									llmStatus?.effective_model_input_limit_source,
								)}
								）。
							</p>
						</div>
						{llmSettingsSaveError ? (
							<p className="text-destructive text-sm">{llmSettingsSaveError}</p>
						) : (
							<p className="text-muted-foreground text-xs">
								当前页面会保留正在执行的调用，不会因为缩容而中断已有任务；只有模型目录不准确时，才需要手动填写输入长度上限。
							</p>
						)}
					</div>

					<DialogFooter>
						<Button
							type="button"
							variant="outline"
							onClick={() => setLlmSettingsDialogOpen(false)}
							disabled={llmSettingsSaving}
						>
							取消
						</Button>
						<Button
							type="button"
							onClick={() => void saveLlmSettings()}
							disabled={llmSettingsSaving}
						>
							{llmSettingsSaving ? "保存中…" : "保存设置"}
						</Button>
					</DialogFooter>
				</DialogContent>
			</Dialog>

			<Sheet
				open={isTaskDrawerOpen}
				onOpenChange={(open) => {
					if (!open) {
						onCloseTaskDrawer();
					}
				}}
			>
				<SheetContent
					side="right"
					showCloseButton={false}
					className="w-full gap-0 overflow-y-auto p-0 sm:max-w-4xl"
				>
					<SheetHeader className="gap-3 border-b px-5 py-4 text-left">
						<div className="flex items-start justify-between gap-3">
							<div className="min-w-0 space-y-2">
								<SheetTitle className="text-lg">
									{isTaskDrawerLlmRoute
										? "任务详情 · LLM 调用详情"
										: "任务详情"}
								</SheetTitle>
								<SheetDescription>
									{isTaskDrawerLlmRoute
										? "保留任务路由上下文，查看单次 LLM 调用详情与原始对话。"
										: "查看任务概览、类型详情、关联 LLM 调用与执行时间线。"}
								</SheetDescription>
								{activeTaskDetail ? (
									<>
										<div className="flex flex-wrap items-center gap-2">
											<p className="text-muted-foreground text-sm">
												{taskTypeLabel(activeTaskDetail.task.task_type)}
											</p>
											<StatusBadge
												label={taskStatusLabel(activeTaskDetail.task.status)}
												tone={activeDetailTone}
											/>
											{activeTaskDetail.task.cancel_requested ? (
												<FlagBadge
													label="已请求取消"
													className={cancelRequestedBadgeClass}
												/>
											) : null}
										</div>
										<p className="text-muted-foreground truncate font-mono text-xs">
											{activeTaskDetail.task.id}
										</p>
									</>
								) : activeTaskDrawerTaskId ? (
									<p className="text-muted-foreground truncate font-mono text-xs">
										{activeTaskDrawerTaskId}
									</p>
								) : null}
							</div>
							<Button variant="outline" onClick={onCloseTaskDrawer}>
								关闭
							</Button>
						</div>
					</SheetHeader>
					<div className="space-y-4 px-5 py-4">
						{activeTaskDetail ? (
							isTaskDrawerLlmRoute ? (
								<>
									<div className="rounded-lg border p-3">
										<div className="flex flex-wrap items-center justify-between gap-2">
											<div className="min-w-0">
												<p className="text-muted-foreground text-xs">
													LLM 调用
												</p>
												<p className="mt-1 truncate font-mono text-sm">
													{activeTaskDrawerLlmCallId}
												</p>
												{taskDrawerLlmDetail && activeTaskDrawerLlmTone ? (
													<div className="mt-2">
														<StatusBadge
															label={taskStatusLabel(
																taskDrawerLlmDetail.status,
															)}
															tone={activeTaskDrawerLlmTone}
														/>
													</div>
												) : null}
											</div>
											<Button
												variant="outline"
												size="sm"
												onClick={onBackToTaskDetail}
											>
												返回任务详情
											</Button>
										</div>
									</div>
									{taskDrawerLlmLoading && !taskDrawerLlmDetail ? (
										<LoadingMessage>正在加载 LLM 调用详情...</LoadingMessage>
									) : taskDrawerLlmDetail ? (
										<>
											<p className="text-muted-foreground text-sm">
												来源：
												<span className="font-mono">
													{taskDrawerLlmDetail.source}
												</span>
											</p>
											<LlmCallDetailSection
												detail={taskDrawerLlmDetail}
												onOpenParentTask={onOpenParentTaskFromLlm}
											/>
										</>
									) : (
										<p className="text-muted-foreground text-sm">
											未找到该 LLM 调用详情。
										</p>
									)}
								</>
							) : (
								<>
									<div className="grid gap-2 md:grid-cols-2">
										<div className="rounded-lg border p-3">
											<p className="text-muted-foreground text-xs">任务状态</p>
											<p className="mt-1 font-medium">
												{taskStatusLabel(activeTaskDetail.task.status)}
											</p>
										</div>
										<div className="rounded-lg border p-3">
											<p className="text-muted-foreground text-xs">触发来源</p>
											<p className="mt-1 font-medium">
												{sourceLabel(activeTaskDetail.task.source)}
											</p>
										</div>
										<div className="rounded-lg border p-3">
											<p className="text-muted-foreground text-xs">
												创建 / 开始 / 完成
											</p>
											<p className="mt-1 font-medium">
												{formatLocalDateTime(activeTaskDetail.task.created_at)}
											</p>
											<p className="text-muted-foreground mt-1 text-xs">
												开始{" "}
												{formatLocalDateTime(activeTaskDetail.task.started_at)}{" "}
												· 完成{" "}
												{formatLocalDateTime(activeTaskDetail.task.finished_at)}
											</p>
										</div>
									</div>

									<div className="border-t pt-4">
										<TaskTypeDetailSection
											detail={activeTaskDetail}
											relatedLlmCalls={taskRelatedLlmCalls}
											relatedLlmCallsLoading={taskRelatedLlmLoading}
											onOpenLlmCallDetail={onOpenTaskLlmDetail}
										/>
									</div>

									{activeTaskDetail.task.status === "succeeded" &&
									detailBusinessOutcome &&
									detailBusinessOutcome.code !== "ok" ? (
										<div
											className={`rounded-lg border p-3 ${businessOutcomeBannerClass(
												detailBusinessOutcome.code,
											)}`}
										>
											<p className="text-xs font-semibold">
												业务结果：{detailBusinessOutcome.label}
											</p>
											<p className="mt-1 text-xs opacity-90">
												{detailBusinessOutcome.message}
											</p>
										</div>
									) : null}

									{activeTaskDetail.task.error_message ? (
										<p className="text-destructive text-sm">
											失败原因：{activeTaskDetail.task.error_message}
										</p>
									) : null}

									<div className="border-t pt-4">
										<p className="text-muted-foreground text-xs">执行时间线</p>
										{detailEventMeta?.truncated ? (
											<p className="mt-1 text-xs text-amber-700 dark:text-amber-200">
												仅展示最近 {detailEventMeta.limit} 条事件（已加载{" "}
												{detailEventMeta.returned}/{detailEventMeta.total}）。
											</p>
										) : detailEventMeta ? (
											<p className="text-muted-foreground mt-1 text-xs">
												事件总数 {detailEventMeta.total}，当前已加载{" "}
												{detailEventMeta.returned} 条。
											</p>
										) : null}
									</div>
									<div className="max-h-[52vh] space-y-2 overflow-auto pr-1">
										{taskEvents.length === 0 ? (
											<p className="text-muted-foreground text-sm">
												暂无事件日志。
											</p>
										) : (
											taskEvents
												.slice()
												.reverse()
												.map(({ event, presentation }) => (
													<div
														key={event.id}
														className={`rounded-lg border p-3 ${eventLevelClass(presentation.level)}`}
													>
														<p className="font-medium text-sm">
															{presentation.title}
														</p>
														<p className="text-muted-foreground mt-1 text-xs">
															{presentation.description}
														</p>
														<p className="text-muted-foreground mt-1 font-mono text-[11px]">
															{event.event_type} ·{" "}
															{formatLocalDateTime(event.created_at)}
														</p>
														<details className="mt-2">
															<summary className="text-muted-foreground cursor-pointer text-xs">
																查看原始事件
															</summary>
															<pre className="text-muted-foreground mt-1 overflow-x-auto whitespace-pre-wrap break-all text-[11px]">
																{event.payload_json}
															</pre>
														</details>
													</div>
												))
										)}
									</div>
								</>
							)
						) : detailLoading ? (
							<LoadingMessage>正在加载任务详情...</LoadingMessage>
						) : (
							<div className="space-y-3">
								<p className="text-muted-foreground text-sm">
									任务详情加载失败，请刷新后重试。
								</p>
								<Button variant="outline" onClick={onCloseTaskDrawer}>
									关闭
								</Button>
							</div>
						)}
					</div>
				</SheetContent>
			</Sheet>

			<Sheet
				open={Boolean(llmDetail)}
				onOpenChange={(open) => {
					if (!open) {
						setLlmDetail(null);
					}
				}}
			>
				<SheetContent
					side="right"
					showCloseButton={false}
					className="w-full gap-0 overflow-y-auto p-0 sm:max-w-3xl"
				>
					<SheetHeader className="gap-3 border-b px-5 py-4 text-left">
						<div className="flex items-start justify-between gap-3">
							<div className="min-w-0 space-y-2">
								<SheetTitle className="text-lg">LLM 调用详情</SheetTitle>
								<SheetDescription>
									查看单次 LLM 调用的 prompt、response、错误与等待耗时。
								</SheetDescription>
								{llmDetail ? (
									<>
										<div className="flex flex-wrap items-center gap-2">
											<p className="text-muted-foreground text-sm">
												来源：
												<span className="font-mono">{llmDetail.source}</span>
											</p>
											{openLlmDetailTone ? (
												<StatusBadge
													label={taskStatusLabel(llmDetail.status)}
													tone={openLlmDetailTone}
												/>
											) : null}
										</div>
										<p className="text-muted-foreground truncate font-mono text-xs">
											{llmDetail.id}
										</p>
									</>
								) : null}
							</div>
							<Button variant="outline" onClick={() => setLlmDetail(null)}>
								关闭
							</Button>
						</div>
					</SheetHeader>
					<div className="px-5 py-4">
						{llmDetail ? (
							<LlmCallDetailSection
								detail={llmDetail}
								onOpenParentTask={onOpenParentTaskFromLlm}
							/>
						) : null}
					</div>
				</SheetContent>
			</Sheet>
		</div>
	);
}
