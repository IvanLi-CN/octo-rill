import { AlertCircle, RefreshCcw } from "lucide-react";
import type { ReactNode } from "react";

import { ErrorStatePanel } from "@/components/feedback/ErrorStatePanel";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type ListSurfaceShellProps = {
	state: string;
	refreshing?: boolean;
	className?: string;
	children: ReactNode;
};

export function ListSurfaceShell(props: ListSurfaceShellProps) {
	const { state, refreshing = false, className, children } = props;
	return (
		<div
			data-list-state={state}
			data-list-refreshing={refreshing ? "true" : "false"}
			className={cn("relative", className)}
		>
			{children}
		</div>
	);
}

type ListRefreshingNoticeProps = {
	label: string;
	className?: string;
};

export function ListRefreshingNotice(props: ListRefreshingNoticeProps) {
	const { label, className } = props;
	return (
		<p
			className={cn(
				"bg-background/92 text-muted-foreground pointer-events-none absolute right-0 top-0 z-10 inline-flex items-center gap-2 rounded-full border px-3 py-1 text-xs shadow-sm backdrop-blur-sm",
				className,
			)}
			data-list-refresh-notice="true"
		>
			<span className="size-2 rounded-full bg-amber-500/80" />
			{label}
		</p>
	);
}

type ListInlineErrorProps = {
	title: string;
	summary: string;
	actionLabel?: string;
	onAction?: () => void;
	loading?: boolean;
	className?: string;
};

export function ListInlineError(props: ListInlineErrorProps) {
	const {
		title,
		summary,
		actionLabel,
		onAction,
		loading = false,
		className,
	} = props;
	return (
		<div
			className={cn(
				"flex flex-wrap items-start gap-3 rounded-xl border border-destructive/20 bg-destructive/[0.04] px-3 py-2 text-sm",
				className,
			)}
			role="alert"
			data-list-inline-error="true"
		>
			<div className="flex min-w-0 flex-1 items-start gap-2">
				<AlertCircle className="mt-0.5 size-4 shrink-0 text-destructive" />
				<div className="min-w-0">
					<p className="font-medium text-foreground">{title}</p>
					<p className="text-muted-foreground break-words text-xs leading-5">
						{summary}
					</p>
				</div>
			</div>
			{actionLabel && onAction ? (
				<Button
					type="button"
					variant="outline"
					size="sm"
					className="font-mono text-xs"
					onClick={onAction}
					disabled={loading}
				>
					<RefreshCcw className={cn("size-4", loading && "animate-spin")} />
					{actionLabel}
				</Button>
			) : null}
		</div>
	);
}

type ListEmptyStateProps = {
	title: string;
	description: ReactNode;
	action?: ReactNode;
	className?: string;
};

export function ListEmptyState(props: ListEmptyStateProps) {
	const { title, description, action, className } = props;
	return (
		<div
			className={cn(
				"rounded-2xl border border-dashed border-border/70 bg-muted/20 px-4 py-8 text-center",
				className,
			)}
			data-list-empty-state="true"
		>
			<p className="font-medium text-foreground">{title}</p>
			<p className="text-muted-foreground mx-auto mt-2 max-w-2xl text-sm leading-6">
				{description}
			</p>
			{action ? <div className="mt-4 flex justify-center">{action}</div> : null}
		</div>
	);
}

type ListBlockingErrorStateProps = {
	title: string;
	summary: string;
	detail?: string | null;
	actionLabel?: string;
	onAction?: () => void;
	loading?: boolean;
	className?: string;
};

export function ListBlockingErrorState(props: ListBlockingErrorStateProps) {
	const { className, ...rest } = props;
	return (
		<div data-list-blocking-error="true">
			<ErrorStatePanel {...rest} className={cn("rounded-2xl", className)} />
		</div>
	);
}
