import type { ReactNode } from "react";

/**
 * Per-task-type rendering for plan-task run records.
 *
 * The job center keeps one generic card so every type is readable, and lets a
 * type opt into a richer view when its `result_json` carries structure worth
 * showing. Registering a renderer is the only way a type gets one: an unknown
 * type falls back to the generic card rather than rendering nothing, which is
 * what happened when the frontend kept its own literal list of types.
 */
export type PlanTaskRunSummary = {
	scanned?: number;
	admitted?: number;
	cursor?: number;
	complete?: boolean;
	quota_scanned?: number;
	quota_requeued?: number;
};

export type PlanTaskRunRenderer = (result: PlanTaskRunSummary) => ReactNode;

/**
 * Parse a task's `result_json` into the shape renderers consume.
 *
 * Returns `null` when the payload is missing or not an object, so a renderer
 * never has to defend against arbitrary input.
 */
export function parsePlanTaskResult(
	raw: string | null | undefined,
): PlanTaskRunSummary | null {
	if (!raw) return null;
	let parsed: unknown;
	try {
		parsed = JSON.parse(raw);
	} catch {
		return null;
	}
	if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
		return null;
	}
	const record = parsed as Record<string, unknown>;
	const num = (value: unknown) =>
		typeof value === "number" && Number.isFinite(value) ? value : undefined;
	return {
		scanned: num(record.scanned),
		admitted: num(record.admitted),
		cursor: num(record.cursor),
		complete:
			typeof record.complete === "boolean" ? record.complete : undefined,
		quota_scanned: num(record.quota_scanned),
		quota_requeued: num(record.quota_requeued),
	};
}

const POLISH_BACKFILL_RENDERER: PlanTaskRunRenderer = (result) => {
	const rows: Array<[string, string]> = [];
	if (result.scanned !== undefined)
		rows.push(["扫描", `${result.scanned} 个 Release`]);
	if (result.admitted !== undefined)
		rows.push(["准入", `${result.admitted} 个变体`]);
	if (result.cursor !== undefined) {
		rows.push([
			"游标",
			result.cursor === 0 ? "已归零（整轮完成）" : String(result.cursor),
		]);
	}
	if (result.complete !== undefined) {
		rows.push(["状态", result.complete ? "本轮回卷完成" : "本轮继续推进"]);
	}
	if (result.quota_requeued)
		rows.push(["额度恢复", `${result.quota_requeued} 条重新入队`]);
	if (rows.length === 0) return null;
	return (
		<dl className="mt-2 grid grid-cols-2 gap-x-4 gap-y-1 text-xs sm:grid-cols-3">
			{rows.map(([label, value]) => (
				<div key={label} className="flex items-baseline gap-1.5">
					<dt className="text-muted-foreground">{label}</dt>
					<dd className="font-mono">{value}</dd>
				</div>
			))}
		</dl>
	);
};

/**
 * Renderer registry keyed by task type.
 *
 * Add an entry only when the generic card genuinely cannot express the result;
 * a wider registry costs a reader more than it saves.
 */
export const PLAN_TASK_RUN_RENDERERS: Record<string, PlanTaskRunRenderer> = {
	"polish.backfill": POLISH_BACKFILL_RENDERER,
};
