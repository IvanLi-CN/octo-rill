import { ChevronDown } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import { TaskTypeDetailSection } from "@/admin/TaskTypeDetailSection";
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
	ApiError,
	apiCancelAdminRealtimeTask,
	apiGetAdminLlmCallDetail,
	apiGetAdminLlmCalls,
	apiGetAdminLlmSchedulerStatus,
	apiGetAdminJobsOverview,
	apiGetAdminRealtimeTaskDetail,
	apiGetAdminRealtimeTasks,
	apiOpenAdminJobsEventsStream,
	apiRetryAdminRealtimeTask,
} from "@/api";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";

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

function taskTypeLabel(taskType: string) {
	switch (taskType) {
		case "brief.daily_slot":
			return "定时日报";
		case "sync.subscriptions":
			return "订阅同步";
		case "brief.generate":
			return "日报生成";
		case "sync.all":
			return "全量同步";
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
	return typeof value === "string" ? value : null;
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
			const userId = readNumber(payload, "user_id");
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
			const userId = readNumber(payload, "user_id");
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
			const userId = readNumber(payload, "user_id");
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
const ADMIN_JOBS_BASE_PATH = "/admin/jobs";
const TASK_DRAWER_ROUTE_PATTERN =
	/^\/admin\/jobs\/tasks\/([^/]+?)(?:\/llm\/([^/]+))?$/;

type StreamStatus = "connecting" | "connected" | "reconnecting";
type TaskDrawerRoute = {
	taskId: string;
	llmCallId: string | null;
};

function normalizePathname(pathname: string) {
	return pathname.replace(/\/+$/, "") || "/";
}

function buildTaskDrawerPath(taskId: string, llmCallId?: string | null) {
	const base = `${ADMIN_JOBS_BASE_PATH}/tasks/${encodeURIComponent(taskId)}`;
	if (!llmCallId) return base;
	return `${base}/llm/${encodeURIComponent(llmCallId)}`;
}

function parseTaskDrawerRoute(pathname: string): TaskDrawerRoute | null {
	const normalized = normalizePathname(pathname);
	const matched = normalized.match(TASK_DRAWER_ROUTE_PATTERN);
	if (!matched) return null;
	try {
		return {
			taskId: decodeURIComponent(matched[1] ?? ""),
			llmCallId: matched[2] ? decodeURIComponent(matched[2]) : null,
		};
	} catch {
		return null;
	}
}

type JobManagementProps = {
	currentUserId: number;
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

export function JobManagement({ currentUserId }: JobManagementProps) {
	const [tab, setTab] = useState<"realtime" | "scheduled" | "llm">("realtime");
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
	const [taskDrawerRoute, setTaskDrawerRoute] =
		useState<TaskDrawerRoute | null>(() =>
			parseTaskDrawerRoute(window.location.pathname),
		);
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

	const overviewLoadedOnceRef = useRef(false);
	const overviewInitialRequestInFlightRef = useRef(false);
	const overviewRequestIdRef = useRef(0);
	const tasksLoadedOnceRef = useRef(false);
	const tasksInitialRequestInFlightRef = useRef(false);
	const tasksRequestIdRef = useRef(0);
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
	const streamRefreshTimerRef = useRef<number | null>(null);
	const streamRefreshInFlightRef = useRef(false);
	const streamPendingFullRefreshRef = useRef(false);
	const streamPendingDetailTaskIdRef = useRef<string | null>(null);
	const streamPendingLlmRefreshRef = useRef(false);
	const streamPendingLlmDetailCallIdRef = useRef<string | null>(null);

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
			.sort((a, b) => a.id - b.id)
			.map((event) => ({
				event,
				presentation: formatEventPresentation(event),
			}));
	}, [detailTask]);
	const detailTaskTone = useMemo(
		() => (detailTask ? taskStatusTone(detailTask.task.status) : null),
		[detailTask],
	);
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
	const scheduledRunsInitialLoading = scheduledRunsLoadPhase === "initial";
	const scheduledRunsRefreshing = scheduledRunsLoadPhase === "refreshing";
	const llmCallsInitialLoading = llmCallsLoadPhase === "initial";
	const llmCallsRefreshing = llmCallsLoadPhase === "refreshing";
	const llmStatusRefreshing = llmStatusLoading && llmStatus !== null;
	const llmRefreshing = llmStatusRefreshing || llmCallsRefreshing;
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
		llmDetailIdRef.current = activeTaskDrawerLlmCallId ?? llmDetail?.id ?? null;
	}, [activeTaskDrawerLlmCallId, llmDetail]);

	const navigateTaskDrawerRoute = useCallback(
		(
			nextRoute: TaskDrawerRoute | null,
			options?: {
				replace?: boolean;
			},
		) => {
			const nextPath = nextRoute
				? buildTaskDrawerPath(nextRoute.taskId, nextRoute.llmCallId)
				: ADMIN_JOBS_BASE_PATH;
			const currentPath = normalizePathname(window.location.pathname);
			const allowPathSync =
				currentPath === ADMIN_JOBS_BASE_PATH ||
				currentPath.startsWith(`${ADMIN_JOBS_BASE_PATH}/`);
			if (allowPathSync && normalizePathname(nextPath) !== currentPath) {
				if (options?.replace) {
					window.history.replaceState({}, "", nextPath);
				} else {
					window.history.pushState({}, "", nextPath);
				}
			}
			setTaskDrawerRoute(nextRoute);
		},
		[],
	);

	useEffect(() => {
		const onPopState = () => {
			setTaskDrawerRoute(parseTaskDrawerRoute(window.location.pathname));
		};
		window.addEventListener("popstate", onPopState);
		return () => {
			window.removeEventListener("popstate", onPopState);
		};
	}, []);

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
			if (
				options?.background &&
				!tasksLoadedOnceRef.current &&
				tasksInitialRequestInFlightRef.current
			) {
				return;
			}
			const requestId = tasksRequestIdRef.current + 1;
			tasksRequestIdRef.current = requestId;
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
				params.set("page", String(llmCallPage));
				params.set("page_size", String(TASK_PAGE_SIZE));
				if (llmSourceFilter.trim()) {
					params.set("source", llmSourceFilter.trim());
				}
				const requestedBy = Number(llmRequestedByFilter.trim());
				if (!Number.isNaN(requestedBy) && llmRequestedByFilter.trim() !== "") {
					params.set("requested_by", String(requestedBy));
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
				setLlmCalls(res.items);
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

	const refreshLlmDetail = useCallback(
		async (callId: string) => {
			const detail = await apiGetAdminLlmCallDetail(callId);
			if (activeTaskDrawerLlmCallId === callId) {
				setTaskDrawerLlmDetail(detail);
				return;
			}
			setLlmDetail(detail);
		},
		[activeTaskDrawerLlmCallId],
	);

	const loadTaskRelatedLlmCalls = useCallback(async (taskId: string) => {
		const params = new URLSearchParams();
		params.set("status", "all");
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
			const pendingDetailTaskId = streamPendingDetailTaskIdRef.current;
			const pendingLlmDetailCallId = streamPendingLlmDetailCallIdRef.current;
			streamPendingFullRefreshRef.current = false;
			streamPendingLlmRefreshRef.current = false;
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
				return;
			}

			if (needLlmRefresh) {
				await Promise.all([
					loadLlmSchedulerStatus({ background: true }),
					loadLlmCalls({ background: true }),
				]);
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
		(mode: "all" | "detail" | "llm" | "llm_detail", id?: string) => {
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
		void loadRealtimeTasks().catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadRealtimeTasks]);

	useEffect(() => {
		setError(null);
		void loadScheduledRuns().catch((err) => {
			setError(normalizeErrorMessage(err));
		});
	}, [loadScheduledRuns]);

	useEffect(() => {
		setError(null);
		void Promise.all([loadLlmSchedulerStatus(), loadLlmCalls()]).catch(
			(err) => {
				setError(normalizeErrorMessage(err));
			},
		);
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

			nextSource.addEventListener("job.event", onJobEvent as EventListener);
			nextSource.addEventListener("llm.call", onLlmCallEvent as EventListener);
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
			navigateTaskDrawerRoute({ taskId, llmCallId: null });
		},
		[navigateTaskDrawerRoute],
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
			navigateTaskDrawerRoute({
				taskId: activeTaskDrawerTaskId,
				llmCallId: callId,
			});
		},
		[activeTaskDrawerTaskId, navigateTaskDrawerRoute],
	);

	const onCloseTaskDrawer = useCallback(() => {
		navigateTaskDrawerRoute(null);
	}, [navigateTaskDrawerRoute]);

	const onBackToTaskDetail = useCallback(() => {
		if (!activeTaskDrawerTaskId) return;
		navigateTaskDrawerRoute({
			taskId: activeTaskDrawerTaskId,
			llmCallId: null,
		});
	}, [activeTaskDrawerTaskId, navigateTaskDrawerRoute]);

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

			<div className="flex flex-wrap items-center gap-2">
				<Button
					variant={tab === "realtime" ? "default" : "outline"}
					size="sm"
					className="font-mono text-xs"
					onClick={() => setTab("realtime")}
				>
					实时异步任务
				</Button>
				<Button
					variant={tab === "scheduled" ? "default" : "outline"}
					size="sm"
					className="font-mono text-xs"
					onClick={() => setTab("scheduled")}
				>
					定时任务
				</Button>
				<Button
					variant={tab === "llm" ? "default" : "outline"}
					size="sm"
					className="font-mono text-xs"
					onClick={() => setTab("llm")}
				>
					LLM调度
				</Button>
				<Button
					variant="secondary"
					size="sm"
					disabled={isRefreshingData}
					onClick={() => void loadAll({ background: true })}
				>
					刷新
				</Button>
				<span
					className={`inline-flex items-center gap-1.5 rounded-full border px-2 py-1 text-xs font-medium ${
						streamStatus === "connected"
							? "border-emerald-300 bg-emerald-100/80 text-emerald-900 dark:border-emerald-500/60 dark:bg-emerald-500/20 dark:text-emerald-100"
							: streamStatus === "reconnecting"
								? "border-amber-300 bg-amber-100/80 text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100"
								: "border-sky-300 bg-sky-100/80 text-sky-900 dark:border-sky-500/60 dark:bg-sky-500/20 dark:text-sky-100"
					}`}
				>
					<span
						className={`size-1.5 rounded-full ${
							streamStatus === "connected"
								? "bg-emerald-500"
								: streamStatus === "reconnecting"
									? "bg-amber-500"
									: "bg-sky-500"
						}`}
					/>
					SSE
					{streamStatus === "connected"
						? " 已连接"
						: streamStatus === "reconnecting"
							? " 重连中..."
							: " 连接中..."}
				</span>
			</div>

			{error ? <p className="text-destructive text-sm">{error}</p> : null}

			{tab === "realtime" ? (
				<Card>
					<CardHeader>
						<div className="flex items-center gap-2">
							<CardTitle>实时异步任务</CardTitle>
							<div className="group relative">
								<button
									type="button"
									aria-label="实时异步任务说明"
									className="text-muted-foreground hover:text-foreground inline-flex size-5 items-center justify-center rounded-full border text-xs transition-colors"
								>
									?
								</button>
								<div
									role="tooltip"
									className="bg-foreground text-background pointer-events-none absolute top-full left-0 z-20 mt-2 w-60 rounded-md px-2 py-1.5 text-xs leading-relaxed opacity-0 shadow-sm transition-opacity duration-150 group-hover:opacity-100 group-focus-within:opacity-100"
								>
									监控系统内部任务，并支持重试与取消。
								</div>
							</div>
						</div>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="flex flex-wrap items-center gap-2">
							<div className="relative w-full max-w-xs">
								<select
									value={statusFilter}
									onChange={(e) => {
										setTaskPage(1);
										setStatusFilter(e.target.value as RealtimeStatusFilter);
									}}
									className="bg-background h-9 w-full appearance-none rounded-md border pl-3 pr-10 text-sm outline-none"
								>
									<option value="all">状态：全部</option>
									<option value="queued">状态：排队</option>
									<option value="running">状态：运行中</option>
									<option value="failed">状态：失败</option>
									<option value="succeeded">状态：成功</option>
									<option value="canceled">状态：取消</option>
								</select>
								<ChevronDown className="text-muted-foreground pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2" />
							</div>
							<span className="text-muted-foreground text-xs">
								共 {formatCount(taskTotal)} 个任务 · 当前用户 #{currentUserId}
							</span>
						</div>

						<div className="space-y-2">
							{tasksInitialLoading ? (
								<p className="text-muted-foreground inline-flex items-center gap-2 text-sm">
									<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
									正在加载任务...
								</p>
							) : (
								<>
									{tasksRefreshing ? (
										<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
											<span className="size-2 rounded-full bg-amber-500/80" />
											任务列表更新中...
										</p>
									) : null}
									{tasks.length === 0 ? (
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
															<span
																className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tone.badgeClass}`}
															>
																<span
																	className={`mr-1.5 size-1.5 rounded-full ${tone.dotClass}`}
																/>
																{taskStatusLabel(task.status)}
															</span>
															{task.cancel_requested ? (
																<span className="rounded-full border border-amber-300 bg-amber-100 px-2 py-0.5 text-[11px] font-medium text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100">
																	已请求取消
																</span>
															) : null}
														</div>
														<p className="text-muted-foreground mt-1 text-xs">
															类型：
															<span className="font-mono">
																{task.task_type}
															</span>
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
															disabled={detailLoading}
															onClick={() => void onOpenTaskDetail(task.id)}
														>
															详情
														</Button>
														<Button
															variant="outline"
															disabled={
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
								</>
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
			) : null}

			{tab === "scheduled" ? (
				<Card>
					<CardHeader>
						<CardTitle>定时任务</CardTitle>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="space-y-3">
							<div className="flex flex-wrap items-center justify-between gap-2">
								<h4 className="font-medium text-sm">运行记录</h4>
								<div className="relative w-full max-w-xs">
									<select
										value={scheduledRunStatusFilter}
										onChange={(e) => {
											setScheduledRunPage(1);
											setScheduledRunStatusFilter(
												e.target.value as RealtimeStatusFilter,
											);
										}}
										className="bg-background h-9 w-full appearance-none rounded-md border pl-3 pr-10 text-sm outline-none"
									>
										<option value="all">状态：全部</option>
										<option value="queued">状态：排队</option>
										<option value="running">状态：运行中</option>
										<option value="failed">状态：失败</option>
										<option value="succeeded">状态：成功</option>
										<option value="canceled">状态：取消</option>
									</select>
									<ChevronDown className="text-muted-foreground pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2" />
								</div>
							</div>
							<p className="text-muted-foreground text-xs">
								共 {formatCount(scheduledRunTotal)} 条
							</p>
							<div className="space-y-2">
								{scheduledRunsInitialLoading ? (
									<p className="text-muted-foreground inline-flex items-center gap-2 text-sm">
										<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
										正在加载运行记录...
									</p>
								) : (
									<>
										{scheduledRunsRefreshing ? (
											<p className="text-muted-foreground inline-flex items-center gap-2 text-xs">
												<span className="size-2 rounded-full bg-amber-500/80" />
												运行记录更新中...
											</p>
										) : null}
										{scheduledRuns.length === 0 ? (
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
																<span
																	className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tone.badgeClass}`}
																>
																	<span
																		className={`mr-1.5 size-1.5 rounded-full ${tone.dotClass}`}
																	/>
																	{taskStatusLabel(task.status)}
																</span>
																{task.cancel_requested ? (
																	<span className="rounded-full border border-amber-300 bg-amber-100 px-2 py-0.5 text-[11px] font-medium text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100">
																		已请求取消
																	</span>
																) : null}
															</div>
															<p className="text-muted-foreground mt-1 text-xs">
																类型：
																<span className="font-mono">
																	{task.task_type}
																</span>
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
																disabled={detailLoading}
																onClick={() => void onOpenTaskDetail(task.id)}
															>
																详情
															</Button>
															<Button
																variant="outline"
																disabled={
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
									</>
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
						</div>
					</CardContent>
				</Card>
			) : null}

			{tab === "llm" ? (
				<Card>
					<CardHeader>
						<CardTitle>LLM 调度</CardTitle>
						<CardDescription>
							查看调度状态与调用级日志，支持按状态/来源/用户/时间筛选。
						</CardDescription>
					</CardHeader>
					<CardContent className="space-y-4">
						<div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
							<div className="bg-card/70 rounded-lg border p-3">
								<p className="text-muted-foreground text-xs">调度器状态</p>
								<p className="mt-1 text-sm font-semibold">
									{llmStatus?.scheduler_enabled ? "已启用" : "未启用"}
								</p>
								<p className="text-muted-foreground mt-1 text-xs">
									节流{" "}
									{formatDurationMs(llmStatus?.request_interval_ms ?? null)}
								</p>
							</div>
							<div className="bg-card/70 rounded-lg border p-3">
								<p className="text-muted-foreground text-xs">等待 / 进行中</p>
								<p className="mt-1 text-sm font-semibold">
									{formatCount(llmStatus?.waiting_calls)} /{" "}
									{formatCount(llmStatus?.in_flight_calls)}
								</p>
								<p className="text-muted-foreground mt-1 text-xs">
									下一个发放槽位{" "}
									{formatDurationMs(llmStatus?.next_slot_in_ms ?? null)}
								</p>
							</div>
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
							</div>
						</div>

						<div className="grid gap-2 lg:grid-cols-4">
							<div className="relative w-full">
								<select
									value={llmStatusFilter}
									onChange={(e) => {
										setLlmCallPage(1);
										setLlmStatusFilter(e.target.value as LlmStatusFilter);
									}}
									className="bg-background h-9 w-full appearance-none rounded-md border pl-3 pr-10 text-sm outline-none"
								>
									<option value="all">状态：全部</option>
									<option value="queued">状态：排队</option>
									<option value="running">状态：运行中</option>
									<option value="failed">状态：失败</option>
									<option value="succeeded">状态：成功</option>
								</select>
								<ChevronDown className="text-muted-foreground pointer-events-none absolute top-1/2 right-3 size-4 -translate-y-1/2" />
							</div>
							<input
								value={llmSourceFilter}
								onChange={(e) => {
									setLlmCallPage(1);
									setLlmSourceFilter(e.target.value);
								}}
								placeholder="来源（source）"
								className="bg-background h-9 rounded-md border px-3 text-sm outline-none"
							/>
							<input
								value={llmRequestedByFilter}
								onChange={(e) => {
									setLlmCallPage(1);
									setLlmRequestedByFilter(e.target.value);
								}}
								placeholder="用户ID（requested_by）"
								className="bg-background h-9 rounded-md border px-3 text-sm outline-none"
							/>
							<div className="grid grid-cols-2 gap-2">
								<input
									type="datetime-local"
									value={llmStartedFromFilter}
									onChange={(e) => {
										setLlmCallPage(1);
										setLlmStartedFromFilter(e.target.value);
									}}
									className="bg-background h-9 rounded-md border px-2 text-xs outline-none"
								/>
								<input
									type="datetime-local"
									value={llmStartedToFilter}
									onChange={(e) => {
										setLlmCallPage(1);
										setLlmStartedToFilter(e.target.value);
									}}
									className="bg-background h-9 rounded-md border px-2 text-xs outline-none"
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
								<p className="text-muted-foreground inline-flex items-center gap-2 text-sm">
									<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
									正在加载调用记录...
								</p>
							) : llmCalls.length === 0 ? (
								<p className="text-muted-foreground text-sm">暂无调用记录。</p>
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
													<span
														className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${tone.badgeClass}`}
													>
														<span
															className={`mr-1.5 size-1.5 rounded-full ${tone.dotClass}`}
														/>
														{taskStatusLabel(call.status)}
													</span>
												</div>
												<p className="text-muted-foreground mt-1 text-xs">
													模型：<span className="font-mono">{call.model}</span>
												</p>
												<p className="text-muted-foreground mt-1 text-xs">
													用户：{call.requested_by ?? "-"} · 重试次数：
													{formatCount(call.attempt_count)}
												</p>
												<p className="text-muted-foreground mt-1 text-xs">
													等待 {formatDurationMs(call.scheduler_wait_ms)} · 首字{" "}
													{formatDurationMs(call.first_token_wait_ms)} · 耗时{" "}
													{formatDurationMs(call.duration_ms)}
												</p>
												<p className="text-muted-foreground mt-1 text-xs">
													Token 输入/输出/缓存：{formatCount(call.input_tokens)}{" "}
													/ {formatCount(call.output_tokens)} /{" "}
													{formatCount(call.cached_input_tokens)}
												</p>
												<p className="text-muted-foreground mt-1 truncate font-mono text-[11px]">
													ID: {call.id}
												</p>
											</div>
											<div className="flex flex-wrap gap-2">
												<Button
													variant="outline"
													disabled={llmDetailLoading}
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
			) : null}

			{isTaskDrawerOpen ? (
				<div className="fixed inset-0 z-40 flex">
					<button
						type="button"
						className="flex-1 bg-black/35"
						aria-label="关闭任务详情"
						onClick={onCloseTaskDrawer}
					/>
					<div className="bg-card relative h-full w-full max-w-4xl border-l p-5 shadow-2xl">
						{activeTaskDetail ? (
							<>
								<div className="flex items-start justify-between gap-3">
									<div className="min-w-0">
										<h3 className="text-lg font-semibold tracking-tight">
											{isTaskDrawerLlmRoute
												? "任务详情 · LLM 调用详情"
												: "任务详情"}
										</h3>
										<div className="mt-1 flex flex-wrap items-center gap-2">
											<p className="text-muted-foreground text-sm">
												{taskTypeLabel(activeTaskDetail.task.task_type)}
											</p>
											<span
												className={`inline-flex items-center rounded-full border px-2 py-0.5 text-[11px] font-semibold tracking-wide ${detailTaskTone?.badgeClass ?? ""}`}
											>
												<span
													className={`mr-1.5 size-1.5 rounded-full ${detailTaskTone?.dotClass ?? "bg-muted-foreground"}`}
												/>
												{taskStatusLabel(activeTaskDetail.task.status)}
											</span>
											{activeTaskDetail.task.cancel_requested ? (
												<span className="rounded-full border border-amber-300 bg-amber-100 px-2 py-0.5 text-[11px] font-medium text-amber-900 dark:border-amber-500/60 dark:bg-amber-500/20 dark:text-amber-100">
													已请求取消
												</span>
											) : null}
										</div>
										<p className="text-muted-foreground mt-1 text-xs">
											类型：
											<span className="font-mono">
												{activeTaskDetail.task.task_type}
											</span>
										</p>
										<p className="text-muted-foreground mt-1 truncate font-mono text-xs">
											{activeTaskDetail.task.id}
										</p>
									</div>
									<Button variant="outline" onClick={onCloseTaskDrawer}>
										关闭
									</Button>
								</div>

								{isTaskDrawerLlmRoute ? (
									<>
										<div className="mt-4 border-t pt-4">
											<div className="flex flex-wrap items-center justify-between gap-2">
												<div className="min-w-0">
													<p className="text-muted-foreground text-xs">
														LLM 调用
													</p>
													<p className="mt-1 truncate font-mono text-sm">
														{activeTaskDrawerLlmCallId}
													</p>
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
											<p className="text-muted-foreground mt-4 inline-flex items-center gap-2 text-sm">
												<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
												正在加载 LLM 调用详情...
											</p>
										) : taskDrawerLlmDetail ? (
											<>
												<p className="text-muted-foreground mt-3 text-sm">
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
											<p className="text-muted-foreground mt-4 text-sm">
												未找到该 LLM 调用详情。
											</p>
										)}
									</>
								) : (
									<>
										<div className="mt-4 grid gap-2 md:grid-cols-2">
											<div className="rounded-lg border p-3">
												<p className="text-muted-foreground text-xs">
													任务状态
												</p>
												<p className="mt-1 font-medium">
													{taskStatusLabel(activeTaskDetail.task.status)}
												</p>
											</div>
											<div className="rounded-lg border p-3">
												<p className="text-muted-foreground text-xs">
													触发来源
												</p>
												<p className="mt-1 font-medium">
													{sourceLabel(activeTaskDetail.task.source)}
												</p>
											</div>
											<div className="rounded-lg border p-3">
												<p className="text-muted-foreground text-xs">
													创建 / 开始 / 完成
												</p>
												<p className="mt-1 font-medium">
													{formatLocalDateTime(
														activeTaskDetail.task.created_at,
													)}
												</p>
												<p className="text-muted-foreground mt-1 text-xs">
													开始{" "}
													{formatLocalDateTime(
														activeTaskDetail.task.started_at,
													)}{" "}
													· 完成{" "}
													{formatLocalDateTime(
														activeTaskDetail.task.finished_at,
													)}
												</p>
											</div>
										</div>

										<div className="mt-4 border-t pt-4">
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
												className={`mt-3 rounded-lg border p-3 ${businessOutcomeBannerClass(
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
											<p className="text-destructive mt-3 text-sm">
												失败原因：{activeTaskDetail.task.error_message}
											</p>
										) : null}

										<div className="mt-4 border-t pt-4">
											<p className="text-muted-foreground text-xs">
												执行时间线
											</p>
											{detailEventMeta?.truncated ? (
												<p className="text-amber-700 mt-1 text-xs dark:text-amber-200">
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
										<div className="mt-2 max-h-[52vh] space-y-2 overflow-auto pr-1">
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
								)}
							</>
						) : detailLoading ? (
							<p className="text-muted-foreground mt-4 inline-flex items-center gap-2 text-sm">
								<span className="size-3 animate-spin rounded-full border-2 border-muted-foreground/35 border-t-muted-foreground" />
								正在加载任务详情...
							</p>
						) : (
							<div className="mt-4 space-y-3">
								<p className="text-muted-foreground text-sm">
									任务详情加载失败，请刷新后重试。
								</p>
								<Button variant="outline" onClick={onCloseTaskDrawer}>
									关闭
								</Button>
							</div>
						)}
					</div>
				</div>
			) : null}

			{llmDetail ? (
				<div className="fixed inset-0 z-40 flex">
					<button
						type="button"
						className="flex-1 bg-black/35"
						aria-label="关闭 LLM 调用详情"
						onClick={() => setLlmDetail(null)}
					/>
					<div className="bg-card relative h-full w-full max-w-3xl border-l p-5 shadow-2xl">
						<div className="flex items-start justify-between gap-3">
							<div className="min-w-0">
								<h3 className="text-lg font-semibold tracking-tight">
									LLM 调用详情
								</h3>
								<p className="text-muted-foreground mt-1 text-sm">
									来源：<span className="font-mono">{llmDetail.source}</span>
								</p>
								<p className="text-muted-foreground mt-1 truncate font-mono text-xs">
									{llmDetail.id}
								</p>
							</div>
							<Button variant="outline" onClick={() => setLlmDetail(null)}>
								关闭
							</Button>
						</div>
						<LlmCallDetailSection
							detail={llmDetail}
							onOpenParentTask={onOpenParentTaskFromLlm}
						/>
					</div>
				</div>
			) : null}
		</div>
	);
}
