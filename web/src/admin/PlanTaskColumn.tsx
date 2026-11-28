import type { AdminPlanTaskType } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import { useState } from "react";

type PlanTaskColumnProps = {
	tasks: AdminPlanTaskType[];
	selectedTaskType: string | null;
	onSelect: (taskType: string) => void;
	inflightTaskTypes: Record<string, string>;
	onTrigger: (
		taskType: string,
		params: Record<string, number>,
	) => Promise<void>;
	triggerBusyTaskType: string | null;
};

function defaultsFor(task: AdminPlanTaskType): Record<string, number> {
	const params: Record<string, number> = {};
	for (const param of task.trigger_params) {
		if (param.default !== null) params[param.name] = param.default;
	}
	return params;
}

/**
 * Left column: every plan-task type, grouped, with a trigger affordance where
 * the server says one is allowed.
 *
 * The list is rendered from the server catalogue rather than a local literal:
 * a type added on the server appears here without a second edit.
 */
export function PlanTaskColumn({
	tasks,
	selectedTaskType,
	onSelect,
	inflightTaskTypes,
	onTrigger,
	triggerBusyTaskType,
}: PlanTaskColumnProps) {
	const groups = new Map<
		string,
		{ label: string; items: AdminPlanTaskType[] }
	>();
	for (const task of tasks) {
		const group = groups.get(task.group_key);
		if (group) group.items.push(task);
		else groups.set(task.group_key, { label: task.group_label, items: [task] });
	}

	return (
		<nav aria-label="计划任务类型" className="space-y-4">
			{[...groups.entries()].map(([key, group]) => (
				<div key={key} className="space-y-1.5">
					<p className="text-muted-foreground px-2 text-xs font-medium">
						{group.label}
					</p>
					<ul className="space-y-1">
						{group.items.map((task) => {
							const inflight = inflightTaskTypes[task.task_type];
							const selected = selectedTaskType === task.task_type;
							return (
								<li key={task.task_type}>
									<div
										className={`flex items-center gap-1 rounded-md pr-1 transition-colors ${
											selected ? "bg-accent" : "hover:bg-accent/60"
										}`}
									>
										<button
											type="button"
											onClick={() => onSelect(task.task_type)}
											aria-current={selected ? "true" : undefined}
											className="min-w-0 flex-1 rounded-md px-2 py-1.5 text-left"
										>
											<span className="block truncate text-sm">
												{task.label}
											</span>
											<span className="text-muted-foreground block truncate font-mono text-[11px]">
												{task.task_type}
											</span>
										</button>
										{task.triggerable ? (
											<TriggerControl
												task={task}
												inflightStatus={inflight}
												onTrigger={onTrigger}
												busy={triggerBusyTaskType === task.task_type}
											/>
										) : null}
									</div>
								</li>
							);
						})}
					</ul>
				</div>
			))}
		</nav>
	);
}

type TriggerControlProps = {
	task: AdminPlanTaskType;
	inflightStatus: string | undefined;
	onTrigger: (
		taskType: string,
		params: Record<string, number>,
	) => Promise<void>;
	busy: boolean;
};

function TriggerControl({
	task,
	inflightStatus,
	onTrigger,
	busy,
}: TriggerControlProps) {
	const [open, setOpen] = useState(false);
	const [values, setValues] = useState<Record<string, number>>(() =>
		defaultsFor(task),
	);

	if (inflightStatus) {
		return (
			<span className="text-muted-foreground shrink-0 px-1.5 text-[11px]">
				{inflightStatus === "running" ? "运行中" : "排队中"}
			</span>
		);
	}

	return (
		<Popover open={open} onOpenChange={setOpen}>
			<PopoverTrigger asChild>
				<Button
					type="button"
					size="sm"
					variant="outline"
					className="shrink-0"
					disabled={busy}
					aria-label={`触发 ${task.label}`}
				>
					触发
				</Button>
			</PopoverTrigger>
			<PopoverContent align="end" className="w-72 space-y-3">
				<p className="text-sm font-medium">{task.label}</p>
				{task.trigger_params.length === 0 ? (
					<p className="text-muted-foreground text-xs">
						该任务无参数，触发后按默认配置入队。
					</p>
				) : (
					<div className="space-y-2">
						{task.trigger_params.map((param) => (
							<div key={param.name} className="space-y-1">
								<Label
									htmlFor={`${task.task_type}-${param.name}`}
									className="text-xs"
								>
									{param.label}
								</Label>
								<Input
									id={`${task.task_type}-${param.name}`}
									type="number"
									min={param.min ?? undefined}
									max={param.max ?? undefined}
									value={values[param.name] ?? param.default ?? 0}
									onChange={(event) => {
										const next = Number(event.currentTarget.value);
										setValues((current) => ({
											...current,
											[param.name]: Number.isFinite(next) ? next : 0,
										}));
									}}
								/>
								{param.min !== null && param.max !== null ? (
									<p className="text-muted-foreground text-[11px]">
										允许范围 {param.min}–{param.max}
									</p>
								) : null}
							</div>
						))}
					</div>
				)}
				<Button
					type="button"
					className="w-full"
					disabled={busy}
					onClick={() => {
						void onTrigger(task.task_type, values).then(() => setOpen(false));
					}}
				>
					{busy ? "触发中…" : "确认触发"}
				</Button>
			</PopoverContent>
		</Popover>
	);
}
