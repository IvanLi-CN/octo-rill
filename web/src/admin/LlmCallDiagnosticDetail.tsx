import { Copy } from "lucide-react";
import { useState } from "react";
import type { SyntheticEvent } from "react";

import {
	type AdminLlmCallDetailResponse,
	apiAuditAdminLlmDiagnosticAccess,
} from "@/api";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";

function statusLabel(status: string) {
	switch (status) {
		case "succeeded":
			return "成功";
		case "failed":
			return "失败";
		case "running":
			return "处理中";
		default:
			return status;
	}
}

function outputContractStatus(detail: AdminLlmCallDetailResponse) {
	if (detail.output_contract_status) return detail.output_contract_status;
	if (detail.finish_reason === "length") return "failed / length";
	if (detail.status !== "succeeded") return "not_run";
	return detail.response_text ? "passed / inspected" : "failed / unavailable";
}

export function LlmCallDiagnosticDetail({
	detail,
}: {
	detail: AdminLlmCallDetailResponse;
}) {
	const [copied, setCopied] = useState(false);
	const failed =
		detail.status === "failed" ||
		outputContractStatus(detail).startsWith("failed");
	const copyResponse = async () => {
		if (!detail.response_text) return;
		try {
			await navigator.clipboard.writeText(detail.response_text);
			await apiAuditAdminLlmDiagnosticAccess(detail.id, "copy");
			setCopied(true);
			window.setTimeout(() => setCopied(false), 1600);
		} catch {
			setCopied(false);
		}
	};
	const auditReveal = async (event: SyntheticEvent<HTMLDetailsElement>) => {
		if (!event.currentTarget.open) return;
		try {
			await apiAuditAdminLlmDiagnosticAccess(detail.id, "reveal");
		} catch {
			// Diagnostic rendering remains useful when audit persistence is unavailable.
		}
	};

	return (
		<div className="space-y-5">
			<div className="space-y-1">
				<p className="font-semibold text-base">{detail.model}</p>
				<div className="flex flex-wrap items-center gap-2">
					<Badge
						variant={detail.status === "failed" ? "destructive" : "secondary"}
					>
						{statusLabel(detail.status)}
					</Badge>
					<span className="text-muted-foreground text-xs">{detail.source}</span>
				</div>
			</div>
			<div className="text-muted-foreground grid gap-2 border-y py-3 text-sm sm:grid-cols-2">
				<p>开始：{detail.started_at ?? "未开始"}</p>
				<p>完成：{detail.finished_at ?? "未完成"}</p>
				<p>调用尝试：{detail.attempt_count}</p>
				<p>Provider HTTP：{detail.provider_http_status ?? "未返回"}</p>
				<p>Finish reason：{detail.finish_reason ?? "未返回"}</p>
				<p>请求标识：{detail.provider_request_id ?? "未返回"}</p>
			</div>
			<div className="grid gap-2 border-b pb-3 text-sm sm:grid-cols-2">
				<p>
					Provider 状态：
					{detail.provider_status ??
						(detail.status === "succeeded" ? "succeeded" : detail.status)}
				</p>
				<p>输出契约：{outputContractStatus(detail)}</p>
				<p>处理阶段：{detail.processing_stage ?? "未记录"}</p>
				<p>重试处置：{detail.retry_disposition ?? "未记录"}</p>
				<p>关联角色：{detail.relation_role ?? "未记录"}</p>
				<p>证据状态：{detail.evidence_availability ?? "可用"}</p>
			</div>
			{detail.error_text ? (
				<p className="border border-red-500/35 bg-red-500/5 p-3 text-destructive text-sm">
					{detail.error_text}
				</p>
			) : null}
			<details open={failed} onToggle={auditReveal} className="border-y py-3">
				<summary className="cursor-pointer font-semibold text-sm">
					模型响应
				</summary>
				<div className="mt-3 flex items-start justify-between gap-3">
					<code className="max-h-72 min-w-0 flex-1 overflow-auto whitespace-pre-wrap break-words text-xs">
						{detail.response_text ?? "未捕获响应内容"}
					</code>
					<Button
						type="button"
						variant="outline"
						size="sm"
						onClick={copyResponse}
						disabled={!detail.response_text}
						aria-label="复制模型响应"
					>
						<Copy className="mr-1 size-3.5" />
						{copied ? "已复制" : "复制"}
					</Button>
				</div>
			</details>
			<details onToggle={auditReveal} className="border-b pb-3">
				<summary className="cursor-pointer font-semibold text-sm">
					Prompt（敏感内容已脱敏）
				</summary>
				<pre className="mt-3 max-h-56 overflow-auto whitespace-pre-wrap break-words text-xs">
					{detail.prompt_text}
				</pre>
			</details>
			<section className="space-y-3">
				<h3 className="font-semibold text-sm">调用事件</h3>
				<div className="divide-y border-y">
					{detail.attempt_history.length === 0 ? (
						<p className="text-muted-foreground py-3 text-xs">暂无尝试记录。</p>
					) : (
						detail.attempt_history.map((attempt, index) => (
							<div key={`${attempt.created_at}:${index}`} className="py-3">
								<div className="flex flex-wrap items-center gap-2">
									<Badge
										variant={
											attempt.status === "failed" ? "destructive" : "secondary"
										}
									>
										{statusLabel(attempt.status)}
									</Badge>
									<span className="text-sm">
										{attempt.model ?? detail.model}
									</span>
								</div>
								<p className="text-muted-foreground mt-1 text-xs">
									{attempt.event_type} · {attempt.created_at}
								</p>
							</div>
						))
					)}
				</div>
			</section>
		</div>
	);
}
