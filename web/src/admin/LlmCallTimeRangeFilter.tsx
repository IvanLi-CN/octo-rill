import {
	CalendarRange,
	ChevronDown,
	ChevronLeft,
	ChevronRight,
} from "lucide-react";
import { useEffect, useId, useMemo, useState, type ReactNode } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";
import {
	Sheet,
	SheetContent,
	SheetHeader,
	SheetTitle,
	SheetTrigger,
} from "@/components/ui/sheet";
import {
	Select,
	SelectContent,
	SelectGroup,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { cn } from "@/lib/utils";

export type LlmCallTimeRangeValue = {
	from: string;
	to: string;
};

const WEEKDAYS = ["一", "二", "三", "四", "五", "六", "日"];

function pad2(value: number) {
	return value.toString().padStart(2, "0");
}

function formatDateTimeInput(value: string) {
	if (!value) return "";
	const [date = "", time = ""] = value.split("T");
	return `${date.replaceAll("-", "/")} ${time}`.trim();
}

function parseDateTimeInput(value: string) {
	const match = value
		.trim()
		.match(/^(\d{4})[/-](\d{2})[/-](\d{2})[ T](\d{2}):(\d{2})$/);
	if (!match) return null;

	const [, year, month, day, hour, minute] = match;
	const parsed = new Date(
		Number(year),
		Number(month) - 1,
		Number(day),
		Number(hour),
		Number(minute),
	);
	if (
		parsed.getFullYear() !== Number(year) ||
		parsed.getMonth() !== Number(month) - 1 ||
		parsed.getDate() !== Number(day) ||
		parsed.getHours() !== Number(hour) ||
		parsed.getMinutes() !== Number(minute)
	) {
		return null;
	}

	return `${year}-${month}-${day}T${hour}:${minute}`;
}

function parseLocalDateTime(value: string) {
	const match = value.match(/^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})$/);
	if (!match) return null;
	const [, year, month, day, hour, minute] = match;
	const date = new Date(
		Number(year),
		Number(month) - 1,
		Number(day),
		Number(hour),
		Number(minute),
	);
	if (Number.isNaN(date.getTime())) return null;
	return { date, hour: Number(hour), minute: Number(minute) };
}

function formatLocalValue(date: Date, hour: number, minute: number) {
	return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}T${pad2(hour)}:${pad2(minute)}`;
}

function dateKey(date: Date) {
	return `${date.getFullYear()}-${pad2(date.getMonth() + 1)}-${pad2(date.getDate())}`;
}

function monthLabel(date: Date) {
	return new Intl.DateTimeFormat("zh-CN", {
		year: "numeric",
		month: "long",
	}).format(date);
}

function getMonthDays(month: Date) {
	const first = new Date(month.getFullYear(), month.getMonth(), 1);
	const offset = (first.getDay() + 6) % 7;
	return Array.from({ length: 42 }, (_, index) => {
		const day = new Date(
			month.getFullYear(),
			month.getMonth(),
			index - offset + 1,
		);
		return day;
	});
}

export function formatLlmCallTimeRangeSummary(
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

function DateTimeBoundaryInput({
	label,
	value,
	onValueChange,
	hideLabel = false,
	isActive = false,
	onFocus,
}: {
	label: string;
	value: string;
	onValueChange: (value: string) => void;
	hideLabel?: boolean;
	isActive?: boolean;
	onFocus?: () => void;
}) {
	const inputId = useId();
	const formattedValue = formatDateTimeInput(value);
	const [draft, setDraft] = useState(formattedValue);

	useEffect(() => {
		setDraft(formattedValue);
	}, [formattedValue]);

	function commitDraft(nextDraft: string) {
		const trimmed = nextDraft.trim();
		if (!trimmed) {
			onValueChange("");
			return true;
		}
		const parsed = parseDateTimeInput(trimmed);
		if (!parsed) return false;
		onValueChange(parsed);
		return true;
	}

	return (
		<div className="min-w-0">
			<Label
				htmlFor={inputId}
				className={hideLabel ? "sr-only" : "mb-1.5 block text-xs"}
			>
				{label}
			</Label>
			<Input
				id={inputId}
				type="text"
				value={draft}
				onFocus={onFocus}
				onChange={(event) => {
					const nextDraft = event.target.value;
					setDraft(nextDraft);
					commitDraft(nextDraft);
				}}
				onBlur={() => {
					if (!commitDraft(draft)) setDraft(formattedValue);
				}}
				onKeyDown={(event) => {
					if (event.key === "Enter") event.currentTarget.blur();
				}}
				placeholder="YYYY/MM/DD HH:mm"
				inputMode="numeric"
				autoComplete="off"
				aria-label={`LLM ${label}`}
				className={cn(
					"h-10 px-2 text-xs tabular-nums",
					isActive && "border-primary ring-1 ring-primary",
				)}
			/>
		</div>
	);
}

function BoundaryCalendar({
	label,
	value,
	onValueChange,
}: {
	label: string;
	value: string;
	onValueChange: (value: string) => void;
}) {
	const parts = parseLocalDateTime(value);
	const [viewMonth, setViewMonth] = useState(() => {
		const date = parts?.date ?? new Date();
		return new Date(date.getFullYear(), date.getMonth(), 1);
	});
	const days = useMemo(() => getMonthDays(viewMonth), [viewMonth]);

	useEffect(() => {
		const parsed = parseLocalDateTime(value);
		if (!parsed) return;
		setViewMonth(
			new Date(parsed.date.getFullYear(), parsed.date.getMonth(), 1),
		);
	}, [value]);

	const selectedKey = parts ? dateKey(parts.date) : "";

	return (
		<section className="min-w-0 rounded-md border border-border/80 bg-muted/20 p-2.5">
			<div className="mb-2 flex items-center justify-center gap-1">
				<Button
					type="button"
					variant="ghost"
					size="icon"
					aria-label={`${label}上个月`}
					className="size-7"
					onClick={() =>
						setViewMonth(
							(month) => new Date(month.getFullYear(), month.getMonth() - 1, 1),
						)
					}
				>
					<ChevronLeft data-icon="inline-start" />
				</Button>
				<span className="min-w-20 text-center text-sm font-medium">
					{monthLabel(viewMonth)}
				</span>
				<Button
					type="button"
					variant="ghost"
					size="icon"
					aria-label={`${label}下个月`}
					className="size-7"
					onClick={() =>
						setViewMonth(
							(month) => new Date(month.getFullYear(), month.getMonth() + 1, 1),
						)
					}
				>
					<ChevronRight data-icon="inline-start" />
				</Button>
			</div>
			<fieldset className="grid grid-cols-7 gap-0.5 text-center text-muted-foreground text-xs">
				<legend className="sr-only">LLM {label}日历</legend>
				{WEEKDAYS.map((weekday) => (
					<span key={weekday} className="py-0.5">
						{weekday}
					</span>
				))}
				{days.map((day) => {
					const isCurrentMonth = day.getMonth() === viewMonth.getMonth();
					const isSelected = dateKey(day) === selectedKey;
					return (
						<button
							key={day.toISOString()}
							type="button"
							aria-label={`${label} ${day.getFullYear()}年${day.getMonth() + 1}月${day.getDate()}日`}
							aria-pressed={isSelected}
							className={cn(
								"flex size-7 items-center justify-center rounded-md text-xs transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none",
								!isCurrentMonth && "text-muted-foreground/45",
								isSelected &&
									"bg-primary text-primary-foreground hover:bg-primary/90",
							)}
							onClick={() => {
								const hour = parts?.hour ?? 0;
								const minute = parts?.minute ?? 0;
								onValueChange(formatLocalValue(day, hour, minute));
							}}
						>
							{day.getDate()}
						</button>
					);
				})}
			</fieldset>
			<div className="mt-2 grid grid-cols-2 gap-1.5 border-border/80 border-t pt-2">
				<div className="min-w-0">
					<Label className="mb-1 block text-muted-foreground text-xs">
						小时
					</Label>
					<Select
						value={pad2(parts?.hour ?? 0)}
						onValueChange={(hour) => {
							const parsed = parts ?? {
								date: new Date(),
								hour: 0,
								minute: 0,
							};
							onValueChange(
								formatLocalValue(parsed.date, Number(hour), parsed.minute),
							);
						}}
					>
						<SelectTrigger
							aria-label={`LLM ${label}小时`}
							className="h-9 w-full text-xs"
						>
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							<SelectGroup>
								{Array.from({ length: 24 }, (_, hour) => (
									<SelectItem key={hour} value={pad2(hour)}>
										{pad2(hour)}
									</SelectItem>
								))}
							</SelectGroup>
						</SelectContent>
					</Select>
				</div>
				<div className="min-w-0">
					<Label className="mb-1 block text-muted-foreground text-xs">
						分钟
					</Label>
					<Select
						value={pad2(parts?.minute ?? 0)}
						onValueChange={(minute) => {
							const parsed = parts ?? {
								date: new Date(),
								hour: 0,
								minute: 0,
							};
							onValueChange(
								formatLocalValue(parsed.date, parsed.hour, Number(minute)),
							);
						}}
					>
						<SelectTrigger
							aria-label={`LLM ${label}分钟`}
							className="h-9 w-full text-xs"
						>
							<SelectValue />
						</SelectTrigger>
						<SelectContent>
							<SelectGroup>
								{Array.from({ length: 60 }, (_, minute) => (
									<SelectItem key={minute} value={pad2(minute)}>
										{pad2(minute)}
									</SelectItem>
								))}
							</SelectGroup>
						</SelectContent>
					</Select>
				</div>
			</div>
		</section>
	);
}

export function LlmCallTimeRangeSelectionPanel({
	fromLabel,
	toLabel,
	value,
	onValueChange,
	singleBoundary = false,
}: {
	fromLabel: string;
	toLabel: string;
	value: LlmCallTimeRangeValue;
	onValueChange: (value: LlmCallTimeRangeValue) => void;
	singleBoundary?: boolean;
}) {
	const [activeBoundary, setActiveBoundary] = useState<"from" | "to">("from");
	const activeLabel = activeBoundary === "from" ? fromLabel : toLabel;
	const activeValue = activeBoundary === "from" ? value.from : value.to;
	const updateActiveValue = (nextValue: string) => {
		if (activeBoundary === "from") {
			onValueChange({ ...value, from: nextValue });
			return;
		}
		onValueChange({ ...value, to: nextValue });
	};

	return (
		<div
			data-testid="llm-time-range-selection-panel"
			className="flex flex-col gap-3"
		>
			<div className="grid grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)] items-center gap-2">
				<DateTimeBoundaryInput
					label={fromLabel}
					value={value.from}
					onValueChange={(from) => onValueChange({ ...value, from })}
					hideLabel
					isActive={singleBoundary && activeBoundary === "from"}
					onFocus={() => setActiveBoundary("from")}
				/>
				<span aria-hidden="true" className="text-muted-foreground text-sm">
					~
				</span>
				<DateTimeBoundaryInput
					label={toLabel}
					value={value.to}
					onValueChange={(to) => onValueChange({ ...value, to })}
					hideLabel
					isActive={singleBoundary && activeBoundary === "to"}
					onFocus={() => setActiveBoundary("to")}
				/>
			</div>
			{singleBoundary ? (
				<BoundaryCalendar
					label={activeLabel}
					value={activeValue}
					onValueChange={updateActiveValue}
				/>
			) : (
				<div className="grid gap-3 sm:grid-cols-2">
					<BoundaryCalendar
						label={fromLabel}
						value={value.from}
						onValueChange={(from) => onValueChange({ ...value, from })}
					/>
					<BoundaryCalendar
						label={toLabel}
						value={value.to}
						onValueChange={(to) => onValueChange({ ...value, to })}
					/>
				</div>
			)}
		</div>
	);
}

export function LlmCallTimeRangeFilter({
	label,
	value,
	onValueChange,
	exclusiveUpperBound = false,
	open: controlledOpen,
	onOpenChange,
	trigger: customTrigger,
}: {
	label: string;
	value: LlmCallTimeRangeValue;
	onValueChange: (value: LlmCallTimeRangeValue) => void;
	exclusiveUpperBound?: boolean;
	open?: boolean;
	onOpenChange?: (open: boolean) => void;
	trigger?: ReactNode | null;
}) {
	const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
	const open = controlledOpen ?? uncontrolledOpen;
	const isMobile = useMediaQuery("(max-width: 639px)");
	const summary = formatLlmCallTimeRangeSummary(value.from, value.to, {
		exclusiveUpperBound,
	});
	const setOpen = (nextOpen: boolean) => {
		if (controlledOpen === undefined) setUncontrolledOpen(nextOpen);
		onOpenChange?.(nextOpen);
	};
	const fromLabel = `${label}后`;
	const toLabel = exclusiveUpperBound ? `${label}前（不含）` : `${label}前`;
	const defaultTrigger = (
		<Button
			type="button"
			variant="outline"
			aria-label={`LLM ${label}范围`}
			className="h-9 w-full justify-start px-3 text-left font-normal"
		>
			<CalendarRange className="text-muted-foreground size-4" />
			<span className="shrink-0 text-sm">{label}</span>
			<span className="text-muted-foreground min-w-0 truncate text-sm">
				{summary || "不限"}
			</span>
			<ChevronDown className="text-muted-foreground ml-auto size-4" />
		</Button>
	);
	const trigger = customTrigger === undefined ? defaultTrigger : customTrigger;
	const panel = (
		<LlmCallTimeRangeSelectionPanel
			fromLabel={fromLabel}
			toLabel={toLabel}
			value={value}
			onValueChange={onValueChange}
		/>
	);

	if (isMobile) {
		return (
			<Sheet open={open} onOpenChange={setOpen}>
				{trigger ? <SheetTrigger asChild>{trigger}</SheetTrigger> : null}
				<SheetContent
					side="bottom"
					aria-label={`LLM ${label}范围设置`}
					className="max-h-[calc(100dvh-1.5rem)] gap-0 rounded-t-lg p-0"
				>
					<SheetHeader className="sr-only">
						<SheetTitle>LLM {label}范围设置</SheetTitle>
					</SheetHeader>
					<div
						data-testid="llm-time-range-drawer-scroll"
						className="min-h-0 flex-1 overflow-y-auto px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] pt-12"
					>
						<LlmCallTimeRangeSelectionPanel
							fromLabel={fromLabel}
							toLabel={toLabel}
							value={value}
							onValueChange={onValueChange}
							singleBoundary
						/>
					</div>
				</SheetContent>
			</Sheet>
		);
	}

	return (
		<Popover open={open} onOpenChange={setOpen}>
			{trigger ? <PopoverTrigger asChild>{trigger}</PopoverTrigger> : null}
			<PopoverContent
				align="start"
				collisionPadding={24}
				aria-label={`LLM ${label}范围设置`}
				className="max-h-[var(--radix-popover-content-available-height)] w-[min(calc(100vw-2rem),40rem)] overflow-y-auto rounded-md p-3"
			>
				{panel}
			</PopoverContent>
		</Popover>
	);
}
