import { AlertTriangle, RefreshCcw } from "lucide-react";
import type { ReactNode } from "react";

import { ErrorDetailDisclosure } from "@/components/feedback/ErrorDetailDisclosure";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

type ErrorStatePanelProps = {
	title: string;
	summary: string;
	detail?: string | null;
	actions?: ReactNode;
	actionLabel?: string;
	onAction?: () => void;
	disabled?: boolean;
	loading?: boolean;
	size?: "default" | "compact";
	className?: string;
};

export function ErrorStatePanel(props: ErrorStatePanelProps) {
	const {
		title,
		summary,
		detail,
		actions,
		actionLabel,
		onAction,
		disabled = false,
		loading = false,
		size = "default",
		className,
	} = props;
	const compact = size === "compact";

	return (
		<div
			role="alert"
			className={cn(
				"rounded-[1.4rem] border border-destructive/18 bg-destructive/[0.045] text-foreground shadow-[0_18px_42px_-32px_rgba(239,68,68,0.45)]",
				compact ? "p-4" : "p-5 sm:p-6",
				className,
			)}
		>
			<div
				className={cn("flex gap-3", compact ? "items-start" : "items-start")}
			>
				<div className="bg-destructive/10 text-destructive flex size-10 shrink-0 items-center justify-center rounded-full border border-destructive/14">
					<AlertTriangle className="size-4.5" />
				</div>
				<div className="min-w-0 flex-1 space-y-3">
					<div className="space-y-1.5">
						<p
							className={cn(
								"font-semibold tracking-tight",
								compact ? "text-sm" : "text-base",
							)}
						>
							{title}
						</p>
						<p
							className={cn(
								"text-muted-foreground leading-6",
								compact ? "text-sm" : "text-sm",
							)}
						>
							{summary}
						</p>
					</div>
					<ErrorDetailDisclosure detail={detail} summary={summary} />
					{actions ? (
						<div className="flex flex-wrap gap-2">{actions}</div>
					) : actionLabel && onAction ? (
						<div className="flex flex-wrap gap-2">
							<Button
								variant="outline"
								size="sm"
								className="font-mono text-xs"
								onClick={onAction}
								disabled={disabled || loading}
								aria-busy={loading ? "true" : undefined}
							>
								<RefreshCcw
									className={cn("size-4", loading && "animate-spin")}
								/>
								{actionLabel}
							</Button>
						</div>
					) : null}
				</div>
			</div>
		</div>
	);
}
