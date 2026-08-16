import { CalendarRange, ChevronDown } from "lucide-react";
import { useId } from "react";

import type { LlmCallTimeField } from "@/admin/jobsRouteState";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";

export type LlmCallTimeRangeValue = {
	timeField: LlmCallTimeField;
	timeFrom: string;
	timeTo: string;
};

function formatDateTimeInput(value: string) {
	if (!value) return "";
	const [date = "", time = ""] = value.split("T");
	return `${date.replaceAll("-", "/")} ${time}`.trim();
}

function formatRangeSummary(value: LlmCallTimeRangeValue) {
	const from = formatDateTimeInput(value.timeFrom);
	const to = formatDateTimeInput(value.timeTo);
	if (!from && !to) return "不限时间";
	if (!from) {
		return value.timeField === "finished" ? `${to} 前（不含）` : `截至 ${to}`;
	}
	if (!to) return `${from} 起`;
	return value.timeField === "finished"
		? `${from} 至 ${to}（上限不含）`
		: `${from} 至 ${to}`;
}

export function LlmCallTimeRangeFilter({
	value,
	onValueChange,
}: {
	value: LlmCallTimeRangeValue;
	onValueChange: (value: LlmCallTimeRangeValue) => void;
}) {
	const fieldLabel = value.timeField === "finished" ? "结束时间" : "开始时间";
	const fromInputId = useId();
	const toInputId = useId();

	return (
		<Popover>
			<PopoverTrigger asChild>
				<Button
					type="button"
					variant="outline"
					aria-label={`LLM 调用时间范围：${fieldLabel}`}
					className="h-auto min-h-9 w-full justify-start px-3 py-2 text-left font-normal"
				>
					<CalendarRange className="text-muted-foreground size-4" />
					<span className="shrink-0 text-sm">{fieldLabel}</span>
					<span className="text-muted-foreground min-w-0 truncate text-sm">
						{formatRangeSummary(value)}
					</span>
					<ChevronDown className="text-muted-foreground ml-auto size-4" />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				align="start"
				className="w-[min(calc(100vw-2rem),34rem)] rounded-md p-3"
			>
				<fieldset className="space-y-3">
					<legend className="sr-only">LLM 调用时间范围</legend>
					<div className="flex items-center justify-between gap-3">
						<p className="text-sm font-medium">调用时间范围</p>
						<Select
							value={value.timeField}
							onValueChange={(timeField) =>
								onValueChange({
									...value,
									timeField: timeField as LlmCallTimeField,
								})
							}
						>
							<SelectTrigger
								size="sm"
								className="w-[7.5rem]"
								aria-label="LLM 时间口径"
							>
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="started">开始时间</SelectItem>
								<SelectItem value="finished">结束时间</SelectItem>
							</SelectContent>
						</Select>
					</div>
					<div className="grid gap-3 sm:grid-cols-2">
						<div className="min-w-0 space-y-1.5">
							<Label
								htmlFor={fromInputId}
								className="text-muted-foreground text-xs"
							>
								{fieldLabel}后
							</Label>
							<Input
								id={fromInputId}
								type="datetime-local"
								value={value.timeFrom}
								onChange={(event) =>
									onValueChange({ ...value, timeFrom: event.target.value })
								}
								aria-label={`LLM ${fieldLabel}下限`}
								className="text-xs"
							/>
						</div>
						<div className="min-w-0 space-y-1.5">
							<Label
								htmlFor={toInputId}
								className="text-muted-foreground text-xs"
							>
								{value.timeField === "finished"
									? "结束时间前（不含）"
									: "开始时间前"}
							</Label>
							<Input
								id={toInputId}
								type="datetime-local"
								value={value.timeTo}
								onChange={(event) =>
									onValueChange({ ...value, timeTo: event.target.value })
								}
								aria-label={
									value.timeField === "finished"
										? "LLM 结束时间上限（不含）"
										: "LLM 开始时间上限"
								}
								className="text-xs"
							/>
						</div>
					</div>
				</fieldset>
			</PopoverContent>
		</Popover>
	);
}
