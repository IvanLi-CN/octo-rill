import { CalendarRange, ChevronDown } from "lucide-react";
import { useId } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";

export type LlmCallTimeRangeValue = {
	startedFrom: string;
	startedTo: string;
	finishedFrom: string;
	finishedBefore: string;
};

function formatDateTimeInput(value: string) {
	if (!value) return "";
	const [date = "", time = ""] = value.split("T");
	return `${date.replaceAll("-", "/")} ${time}`.trim();
}

function formatRangeSummary(
	fromValue: string,
	toValue: string,
	options?: { exclusiveUpperBound?: boolean },
) {
	const from = formatDateTimeInput(fromValue);
	const to = formatDateTimeInput(toValue);
	if (!from && !to) return "";
	if (!from) return options?.exclusiveUpperBound ? `${to} 前` : `截至 ${to}`;
	if (!to) return `${from} 起`;
	return `${from} 至 ${to}`;
}

function formatFilterSummary(value: LlmCallTimeRangeValue) {
	const started = formatRangeSummary(value.startedFrom, value.startedTo);
	const finished = formatRangeSummary(
		value.finishedFrom,
		value.finishedBefore,
		{
			exclusiveUpperBound: true,
		},
	);
	return (
		[started ? `开始：${started}` : "", finished ? `结束：${finished}` : ""]
			.filter(Boolean)
			.join(" · ") || "不限时间"
	);
}

function TimeRangeFields({
	legend,
	fromLabel,
	toLabel,
	fromValue,
	toValue,
	onFromChange,
	onToChange,
	bordered = false,
}: {
	legend: string;
	fromLabel: string;
	toLabel: string;
	fromValue: string;
	toValue: string;
	onFromChange: (value: string) => void;
	onToChange: (value: string) => void;
	bordered?: boolean;
}) {
	const fromInputId = useId();
	const toInputId = useId();

	return (
		<fieldset
			className={
				bordered ? "border-border space-y-2 border-t pt-3" : "space-y-2"
			}
		>
			<legend className="text-sm font-medium">{legend}</legend>
			<div className="grid gap-3 sm:grid-cols-2">
				<div className="min-w-0 space-y-1.5">
					<Label
						htmlFor={fromInputId}
						className="text-muted-foreground text-xs"
					>
						{fromLabel}
					</Label>
					<Input
						id={fromInputId}
						type="datetime-local"
						value={fromValue}
						onChange={(event) => onFromChange(event.target.value)}
						aria-label={`LLM ${fromLabel}`}
						className="text-xs"
					/>
				</div>
				<div className="min-w-0 space-y-1.5">
					<Label htmlFor={toInputId} className="text-muted-foreground text-xs">
						{toLabel}
					</Label>
					<Input
						id={toInputId}
						type="datetime-local"
						value={toValue}
						onChange={(event) => onToChange(event.target.value)}
						aria-label={`LLM ${toLabel}`}
						className="text-xs"
					/>
				</div>
			</div>
		</fieldset>
	);
}

export function LlmCallTimeRangeFilter({
	value,
	onValueChange,
}: {
	value: LlmCallTimeRangeValue;
	onValueChange: (value: LlmCallTimeRangeValue) => void;
}) {
	const summary = formatFilterSummary(value);

	return (
		<Popover>
			<PopoverTrigger asChild>
				<Button
					type="button"
					variant="outline"
					aria-label="LLM 调用时间范围"
					className="h-auto min-h-9 w-full justify-start px-3 py-2 text-left font-normal"
				>
					<CalendarRange className="text-muted-foreground size-4" />
					<span className="shrink-0 text-sm">调用时间</span>
					<span className="text-muted-foreground min-w-0 truncate text-sm">
						{summary}
					</span>
					<ChevronDown className="text-muted-foreground ml-auto size-4" />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				align="start"
				className="w-[min(calc(100vw-2rem),34rem)] rounded-md p-3"
			>
				<div className="space-y-3">
					<p className="text-sm font-medium">调用时间范围</p>
					<TimeRangeFields
						legend="开始时间"
						fromLabel="开始时间后"
						toLabel="开始时间前"
						fromValue={value.startedFrom}
						toValue={value.startedTo}
						onFromChange={(startedFrom) =>
							onValueChange({ ...value, startedFrom })
						}
						onToChange={(startedTo) => onValueChange({ ...value, startedTo })}
					/>
					<TimeRangeFields
						legend="结束时间"
						fromLabel="结束时间后"
						toLabel="结束时间前（不含）"
						fromValue={value.finishedFrom}
						toValue={value.finishedBefore}
						onFromChange={(finishedFrom) =>
							onValueChange({ ...value, finishedFrom })
						}
						onToChange={(finishedBefore) =>
							onValueChange({ ...value, finishedBefore })
						}
						bordered
					/>
				</div>
			</PopoverContent>
		</Popover>
	);
}
