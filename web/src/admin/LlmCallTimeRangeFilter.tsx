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
	from: string;
	to: string;
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

function TimeRangeFields({
	fromLabel,
	toLabel,
	fromValue,
	toValue,
	onFromChange,
	onToChange,
}: {
	fromLabel: string;
	toLabel: string;
	fromValue: string;
	toValue: string;
	onFromChange: (value: string) => void;
	onToChange: (value: string) => void;
}) {
	const fromInputId = useId();
	const toInputId = useId();

	return (
		<div className="grid gap-3 sm:grid-cols-2">
			<div className="min-w-0 space-y-1.5">
				<Label htmlFor={fromInputId} className="text-muted-foreground text-xs">
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
	);
}

export function LlmCallTimeRangeFilter({
	label,
	value,
	onValueChange,
	exclusiveUpperBound = false,
}: {
	label: string;
	value: LlmCallTimeRangeValue;
	onValueChange: (value: LlmCallTimeRangeValue) => void;
	exclusiveUpperBound?: boolean;
}) {
	const summary = formatRangeSummary(value.from, value.to, {
		exclusiveUpperBound,
	});
	const fromLabel = `${label}后`;
	const toLabel = exclusiveUpperBound ? `${label}前（不含）` : `${label}前`;

	return (
		<Popover>
			<PopoverTrigger asChild>
				<Button
					type="button"
					variant="outline"
					aria-label={`LLM ${label}范围`}
					className="h-auto min-h-11 w-full justify-start px-3 py-2 text-left font-normal"
				>
					<CalendarRange className="text-muted-foreground size-4" />
					<span className="shrink-0 text-sm">{label}</span>
					<span className="text-muted-foreground min-w-0 truncate text-sm">
						{summary || "不限"}
					</span>
					<ChevronDown className="text-muted-foreground ml-auto size-4" />
				</Button>
			</PopoverTrigger>
			<PopoverContent
				align="start"
				collisionPadding={16}
				className="w-[min(calc(100vw-2rem),34rem)] rounded-md p-3"
			>
				<div className="space-y-3">
					<p className="text-sm font-medium">{label}范围</p>
					<TimeRangeFields
						fromLabel={fromLabel}
						toLabel={toLabel}
						fromValue={value.from}
						toValue={value.to}
						onFromChange={(from) => onValueChange({ ...value, from })}
						onToChange={(to) => onValueChange({ ...value, to })}
					/>
				</div>
			</PopoverContent>
		</Popover>
	);
}
