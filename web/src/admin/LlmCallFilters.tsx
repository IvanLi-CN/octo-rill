import { CalendarRange, ChevronDown, SlidersHorizontal, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import {
	formatLlmCallTimeRangeSummary,
	LlmCallTimeRangeFilter,
	LlmCallTimeRangeSelectionPanel,
	type LlmCallTimeRangeValue,
} from "@/admin/LlmCallTimeRangeFilter";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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
	SheetFooter,
	SheetHeader,
	SheetTitle,
	SheetTrigger,
} from "@/components/ui/sheet";
import { cn } from "@/lib/utils";

export type LlmStatusFilter =
	| "all"
	| "queued"
	| "running"
	| "failed"
	| "succeeded";

export const LLM_STATUS_FILTER_OPTIONS: Array<{
	value: LlmStatusFilter;
	label: string;
}> = [
	{ value: "all", label: "状态：全部" },
	{ value: "queued", label: "状态：排队" },
	{ value: "running", label: "状态：运行中" },
	{ value: "failed", label: "状态：失败" },
	{ value: "succeeded", label: "状态：成功" },
];

type LlmCallFiltersProps = {
	status: LlmStatusFilter;
	onStatusChange: (value: LlmStatusFilter) => void;
	model: string;
	onModelChange: (value: string) => void;
	source: string;
	onSourceChange: (value: string) => void;
	requestedBy: string;
	onRequestedByChange: (value: string) => void;
	started: LlmCallTimeRangeValue;
	onStartedChange: (value: LlmCallTimeRangeValue) => void;
	finished: LlmCallTimeRangeValue;
	onFinishedChange: (value: LlmCallTimeRangeValue) => void;
	hasFilters: boolean;
	onClear: () => void;
};

function FilterSelect<T extends string>({
	value,
	onValueChange,
	options,
	ariaLabel,
	className,
}: {
	value: T;
	onValueChange: (value: T) => void;
	options: Array<{ value: T; label: string }>;
	ariaLabel: string;
	className?: string;
}) {
	return (
		<Select value={value} onValueChange={(next) => onValueChange(next as T)}>
			<SelectTrigger className={cn("w-full", className)} aria-label={ariaLabel}>
				<SelectValue />
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

function MobileTimeRangeButton({
	label,
	value,
	exclusiveUpperBound,
	onClick,
}: {
	label: string;
	value: LlmCallTimeRangeValue;
	exclusiveUpperBound?: boolean;
	onClick: () => void;
}) {
	const summary = formatLlmCallTimeRangeSummary(value.from, value.to, {
		exclusiveUpperBound,
	});
	return (
		<Button
			type="button"
			variant="outline"
			className="h-11 w-full justify-start px-3 text-left font-normal"
			aria-label={`LLM ${label}范围`}
			onClick={onClick}
		>
			<CalendarRange className="text-muted-foreground size-4" />
			<span className="shrink-0 text-sm">{label}</span>
			<span className="text-muted-foreground min-w-0 truncate text-sm">
				{summary || "不限"}
			</span>
		</Button>
	);
}

export function LlmCallFilters({
	status,
	onStatusChange,
	model,
	onModelChange,
	source,
	onSourceChange,
	requestedBy,
	onRequestedByChange,
	started,
	onStartedChange,
	finished,
	onFinishedChange,
	hasFilters,
	onClear,
}: LlmCallFiltersProps) {
	const [mobileDrawerOpen, setMobileDrawerOpen] = useState(false);
	const [mobileTimePanel, setMobileTimePanel] = useState<
		"started" | "finished" | null
	>(null);
	const mobileTimePanelRef = useRef<HTMLElement>(null);

	useEffect(() => {
		if (mobileTimePanel === null) return;
		mobileTimePanelRef.current?.focus();
	}, [mobileTimePanel]);

	const activeFilterCount = [
		status !== "all",
		model.trim() !== "",
		source.trim() !== "",
		requestedBy.trim() !== "",
		started.from !== "" || started.to !== "",
		finished.from !== "" || finished.to !== "",
	].filter(Boolean).length;

	const openTimePanel = (panel: "started" | "finished") => {
		setMobileTimePanel(panel);
	};
	const mobileTimePanelLabel =
		mobileTimePanel === "started" ? "开始时间" : "结束时间";
	const mobileTimePanelValue =
		mobileTimePanel === "started" ? started : finished;
	const onMobileTimePanelChange =
		mobileTimePanel === "started" ? onStartedChange : onFinishedChange;

	const mobileFilterFields = (
		<>
			<FilterSelect
				value={status}
				onValueChange={onStatusChange}
				options={LLM_STATUS_FILTER_OPTIONS}
				ariaLabel="LLM 调用状态筛选"
				className="h-11"
			/>
			<Input
				value={model}
				onChange={(event) => onModelChange(event.target.value)}
				placeholder="模型（精确匹配）"
				aria-label="LLM 调用模型筛选"
				className="h-11"
			/>
			<Input
				value={source}
				onChange={(event) => onSourceChange(event.target.value)}
				placeholder="来源（source）"
				aria-label="LLM 调用来源筛选"
				className="h-11"
			/>
			<Input
				value={requestedBy}
				onChange={(event) => onRequestedByChange(event.target.value)}
				placeholder="用户 NanoID（requested_by）"
				aria-label="LLM 调用用户筛选"
				className="h-11"
			/>
			<MobileTimeRangeButton
				label="开始时间"
				value={started}
				onClick={() => openTimePanel("started")}
			/>
			<MobileTimeRangeButton
				label="结束时间"
				exclusiveUpperBound
				value={finished}
				onClick={() => openTimePanel("finished")}
			/>
		</>
	);

	const clearButton = (
		<Button
			type="button"
			variant="outline"
			disabled={!hasFilters}
			onClick={onClear}
		>
			<X className="size-4" />
			清除全部筛选
		</Button>
	);

	return (
		<>
			<div className="sm:hidden">
				<Sheet
					open={mobileDrawerOpen}
					onOpenChange={(open) => {
						if (!open) setMobileTimePanel(null);
						setMobileDrawerOpen(open);
					}}
				>
					<SheetTrigger asChild>
						<Button
							type="button"
							variant="outline"
							className="h-11 w-full justify-between px-3"
							aria-label="打开 LLM 调用筛选"
						>
							<span className="inline-flex items-center gap-2">
								<SlidersHorizontal className="size-4" />
								筛选调用记录
							</span>
							{activeFilterCount > 0 ? (
								<Badge variant="secondary">{activeFilterCount} 项</Badge>
							) : null}
						</Button>
					</SheetTrigger>
					<SheetContent
						side="right"
						aria-label="LLM 调用筛选"
						className="w-[min(22rem,calc(100vw-1rem))] max-w-none gap-0 p-0"
						onEscapeKeyDown={(event) => {
							if (mobileTimePanel === null) return;
							event.preventDefault();
							setMobileTimePanel(null);
						}}
					>
						<SheetHeader className="border-b px-5 py-4 text-left">
							<SheetTitle>筛选调用记录</SheetTitle>
							<SheetDescription className="sr-only">
								设置 LLM 调用状态、模型、来源、用户和时间范围
							</SheetDescription>
						</SheetHeader>
						<div
							inert={mobileTimePanel !== null}
							aria-hidden={mobileTimePanel !== null}
							className="min-h-0 flex-1 space-y-3 overflow-y-auto px-5 py-4"
						>
							{mobileFilterFields}
						</div>
						<SheetFooter
							inert={mobileTimePanel !== null}
							aria-hidden={mobileTimePanel !== null}
							className="border-t px-5 py-4"
						>
							{clearButton}
						</SheetFooter>
						{mobileTimePanel !== null ? (
							<>
								<button
									type="button"
									aria-label="关闭时间范围设置"
									className="absolute inset-0 z-10 cursor-default bg-black/45"
									onClick={() => setMobileTimePanel(null)}
								/>
								<section
									ref={mobileTimePanelRef}
									tabIndex={-1}
									aria-label={`LLM ${mobileTimePanelLabel}范围设置`}
									className="fixed inset-x-0 bottom-0 z-20 max-h-[calc(100dvh-1.5rem)] overflow-hidden rounded-t-lg border-t bg-background shadow-lg outline-none animate-in slide-in-from-bottom-4 duration-200"
								>
									<div className="flex h-12 items-center justify-between border-b px-3">
										<div className="flex items-center gap-2 text-sm font-medium">
											<CalendarRange className="text-muted-foreground size-4" />
											{mobileTimePanelLabel}范围
										</div>
										<Button
											type="button"
											variant="ghost"
											size="icon"
											aria-label="返回筛选条件"
											className="size-10"
											onClick={() => setMobileTimePanel(null)}
										>
											<ChevronDown className="size-4" />
										</Button>
									</div>
									<div
										data-testid="llm-time-range-drawer-scroll"
										className="max-h-[calc(100dvh-4.5rem)] overflow-y-auto px-3 pb-[max(0.75rem,env(safe-area-inset-bottom))] pt-3"
									>
										<LlmCallTimeRangeSelectionPanel
											fromLabel={`${mobileTimePanelLabel}后`}
											toLabel={
												mobileTimePanel === "finished"
													? "结束时间前（不含）"
													: "开始时间前"
											}
											value={mobileTimePanelValue}
											onValueChange={onMobileTimePanelChange}
											singleBoundary
										/>
									</div>
								</section>
							</>
						) : null}
					</SheetContent>
				</Sheet>
			</div>

			<div className="hidden gap-2 sm:grid sm:grid-cols-2 xl:grid-cols-4">
				<FilterSelect
					value={status}
					onValueChange={onStatusChange}
					options={LLM_STATUS_FILTER_OPTIONS}
					ariaLabel="LLM 调用状态筛选"
				/>
				<Input
					value={model}
					onChange={(event) => onModelChange(event.target.value)}
					placeholder="模型（精确匹配）"
					aria-label="LLM 调用模型筛选"
				/>
				<Input
					value={source}
					onChange={(event) => onSourceChange(event.target.value)}
					placeholder="来源（source）"
					aria-label="LLM 调用来源筛选"
				/>
				<Input
					value={requestedBy}
					onChange={(event) => onRequestedByChange(event.target.value)}
					placeholder="用户 NanoID（requested_by）"
					aria-label="LLM 调用用户筛选"
				/>
				<div className="flex min-w-0 gap-2 sm:col-span-2 xl:col-span-4">
					<div className="grid min-w-0 flex-1 gap-2 sm:grid-cols-2">
						<LlmCallTimeRangeFilter
							label="开始时间"
							value={started}
							onValueChange={onStartedChange}
						/>
						<LlmCallTimeRangeFilter
							label="结束时间"
							exclusiveUpperBound
							value={finished}
							onValueChange={onFinishedChange}
						/>
					</div>
					<Button
						type="button"
						variant="outline"
						size="icon"
						aria-label="清除 LLM 调用筛选"
						title="清除筛选"
						disabled={!hasFilters}
						onClick={onClear}
					>
						<X className="size-4" />
					</Button>
				</div>
			</div>
		</>
	);
}
