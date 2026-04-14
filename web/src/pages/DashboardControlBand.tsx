import { Check, ChevronDown } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

import { FeedPageLaneSelector } from "@/feed/FeedPageLaneSelector";
import { FEED_LANE_OPTIONS } from "@/feed/laneOptions";
import type { FeedLane } from "@/feed/types";
import { cn } from "@/lib/utils";
import { TabsList, TabsTrigger } from "@/components/ui/tabs";

export type DashboardTab =
	| "all"
	| "releases"
	| "stars"
	| "followers"
	| "briefs"
	| "inbox";

export const DASHBOARD_TAB_OPTIONS: Array<{
	value: DashboardTab;
	mobileLabel: string;
	desktopLabel: string;
}> = [
	{ value: "all", mobileLabel: "全部", desktopLabel: "全部" },
	{ value: "releases", mobileLabel: "发布", desktopLabel: "Releases" },
	{ value: "stars", mobileLabel: "加星", desktopLabel: "被加星" },
	{ value: "followers", mobileLabel: "关注", desktopLabel: "被关注" },
	{ value: "briefs", mobileLabel: "日报", desktopLabel: "日报" },
	{ value: "inbox", mobileLabel: "收件箱", desktopLabel: "Inbox" },
];

function resolveLaneOption(lane: FeedLane) {
	return (
		FEED_LANE_OPTIONS.find((option) => option.lane === lane) ??
		FEED_LANE_OPTIONS[0]
	);
}

export function DashboardTabsList(props: { className?: string }) {
	return (
		<TabsList
			className={cn(
				"h-auto shrink-0 flex-nowrap rounded-lg bg-muted/60 p-1",
				props.className,
			)}
		>
			{DASHBOARD_TAB_OPTIONS.map((option) => (
				<TabsTrigger
					key={option.value}
					value={option.value}
					className="font-mono text-xs"
				>
					<span className="sm:hidden">{option.mobileLabel}</span>
					<span className="hidden sm:inline">{option.desktopLabel}</span>
				</TabsTrigger>
			))}
		</TabsList>
	);
}

function DashboardMobileTabStrip(props: {
	tab: DashboardTab;
	onSelectTab: (tab: DashboardTab) => void;
	distributed?: boolean;
	className?: string;
}) {
	const { tab, onSelectTab, distributed = false, className } = props;

	return (
		<div
			role="tablist"
			aria-label="Dashboard 主导航"
			className={cn(
				distributed
					? "grid min-w-0 flex-1 grid-cols-6 gap-1 rounded-full border border-border/45 bg-muted/60 p-1 shadow-sm"
					: "inline-flex shrink-0 items-center gap-0.5 rounded-full border border-border/45 bg-muted/60 p-1 shadow-sm",
				className,
			)}
		>
			{DASHBOARD_TAB_OPTIONS.map((option) => {
				const active = option.value === tab;
				return (
					<button
						key={option.value}
						type="button"
						role="tab"
						aria-selected={active}
						data-state={active ? "active" : "inactive"}
						className={cn(
							distributed
								? "inline-flex h-8 min-w-0 items-center justify-center rounded-full px-0 font-mono text-[11px] whitespace-nowrap transition-all"
								: "inline-flex h-7 items-center justify-center rounded-full px-3 font-mono text-xs whitespace-nowrap transition-all",
							active
								? "bg-background text-foreground shadow-sm"
								: "text-foreground/55 hover:text-foreground",
						)}
						onClick={() => onSelectTab(option.value)}
					>
						<span className="truncate">{option.mobileLabel}</span>
					</button>
				);
			})}
		</div>
	);
}

function DashboardMobileLaneMenu(props: {
	value: FeedLane;
	onValueChange: (lane: FeedLane) => void;
	disabled?: boolean;
}) {
	const { value, onValueChange, disabled = false } = props;
	const menuId = useId();
	const wrapperRef = useRef<HTMLDivElement | null>(null);
	const [open, setOpen] = useState(false);
	const activeOption = useMemo(() => resolveLaneOption(value), [value]);
	const ActiveIcon = activeOption.icon;

	useEffect(() => {
		if (disabled) {
			setOpen(false);
		}
	}, [disabled]);

	useEffect(() => {
		if (!open || disabled) return;

		const handlePointerDown = (event: PointerEvent) => {
			const target = event.target;
			if (!(target instanceof Node)) return;
			if (!wrapperRef.current?.contains(target)) {
				setOpen(false);
			}
		};

		const handleKeyDown = (event: KeyboardEvent) => {
			if (event.key === "Escape") {
				setOpen(false);
			}
		};

		window.addEventListener("pointerdown", handlePointerDown);
		window.addEventListener("keydown", handleKeyDown);
		return () => {
			window.removeEventListener("pointerdown", handlePointerDown);
			window.removeEventListener("keydown", handleKeyDown);
		};
	}, [open, disabled]);

	return (
		<div
			ref={wrapperRef}
			className="relative shrink-0"
			data-dashboard-mobile-lane-menu
		>
			<button
				type="button"
				aria-label={
					disabled
						? `当前阅读模式：${activeOption.label}（当前分类不可切换）`
						: `当前阅读模式：${activeOption.label}`
				}
				aria-controls={menuId}
				aria-expanded={open}
				aria-haspopup="menu"
				disabled={disabled}
				data-disabled={disabled ? "true" : "false"}
				data-dashboard-mobile-lane-menu-trigger
				onClick={() => {
					if (disabled) return;
					setOpen((current) => !current);
				}}
				className={cn(
					"inline-flex size-8 items-center justify-center rounded-full border border-border/45 bg-muted/60 text-foreground shadow-sm transition",
					disabled
						? "cursor-not-allowed text-foreground/35 opacity-100"
						: "hover:border-foreground/25 hover:text-foreground",
				)}
			>
				<ActiveIcon className="size-3.5" />
				<span className="sr-only">选择阅读模式</span>
				<ChevronDown className="sr-only" />
			</button>

			{open ? (
				<div
					id={menuId}
					role="menu"
					aria-label="选择阅读模式"
					data-dashboard-mobile-lane-menu-popover
					className="absolute top-full right-0 z-30 mt-2 min-w-32 rounded-2xl border border-border/70 bg-card/98 p-1.5 shadow-lg backdrop-blur"
				>
					{FEED_LANE_OPTIONS.map((option) => {
						const Icon = option.icon;
						const active = option.lane === value;
						return (
							<button
								key={option.lane}
								type="button"
								role="menuitemradio"
								aria-checked={active}
								data-feed-page-lane={option.lane}
								className={cn(
									"flex w-full items-center gap-2 rounded-xl px-2.5 py-2 text-sm transition-colors",
									active
										? "bg-muted text-foreground"
										: "text-foreground/75 hover:bg-muted/60 hover:text-foreground",
								)}
								onClick={() => {
									onValueChange(option.lane);
									setOpen(false);
								}}
							>
								<Icon className="size-3.5 shrink-0" />
								<span className="flex-1 text-left">{option.label}</span>
								{active ? <Check className="size-3.5 shrink-0" /> : null}
							</button>
						);
					})}
				</div>
			) : null}
		</div>
	);
}

export function DashboardMobileControlBand(props: {
	tab: DashboardTab;
	onSelectTab: (tab: DashboardTab) => void;
	showPageLaneSelector: boolean;
	pageLane: FeedLane;
	onSelectPageLane: (lane: FeedLane) => void;
	layout?: "inline" | "stacked";
	className?: string;
}) {
	const {
		tab,
		onSelectTab,
		showPageLaneSelector,
		pageLane,
		onSelectPageLane,
		layout = "inline",
		className,
	} = props;

	if (layout === "stacked") {
		return (
			<div
				data-dashboard-mobile-control-band="true"
				data-dashboard-mobile-control-band-layout="stacked"
				className={cn("flex w-full flex-col gap-2", className)}
			>
				<div
					data-dashboard-mobile-control-band-row="tabs"
					className="w-full px-1"
				>
					<div className="flex w-full items-center gap-2">
						<DashboardMobileTabStrip
							tab={tab}
							onSelectTab={onSelectTab}
							distributed
						/>
						<DashboardMobileLaneMenu
							value={pageLane}
							onValueChange={onSelectPageLane}
							disabled={!showPageLaneSelector}
						/>
					</div>
				</div>
			</div>
		);
	}

	return (
		<div
			data-dashboard-mobile-control-band="true"
			data-dashboard-mobile-control-band-layout="inline"
			data-dashboard-mobile-rail="true"
			className={cn(className)}
		>
			<div className="-mx-1 overflow-x-auto px-1 no-scrollbar">
				<div className="flex min-w-max items-center gap-2">
					<DashboardMobileTabStrip tab={tab} onSelectTab={onSelectTab} />
					<FeedPageLaneSelector
						value={pageLane}
						onValueChange={onSelectPageLane}
						className="shrink-0"
					/>
				</div>
			</div>
		</div>
	);
}
