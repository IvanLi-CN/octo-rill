import { AlertTriangle } from "lucide-react";
import type { ReactNode } from "react";

import { ErrorDetailDisclosure } from "@/components/feedback/ErrorDetailDisclosure";
import {
	Popover,
	PopoverContent,
	PopoverTrigger,
} from "@/components/ui/popover";

export type ErrorBubbleProps = {
	open: boolean;
	onOpenChange: (open: boolean) => void;
	title?: string;
	summary: string;
	detail?: string | null;
	actions?: ReactNode;
	children: ReactNode;
	side?: "top" | "right" | "bottom" | "left";
	align?: "start" | "center" | "end";
};

export function ErrorBubble(props: ErrorBubbleProps) {
	const {
		open,
		onOpenChange,
		title = "操作失败",
		summary,
		detail,
		actions,
		children,
		side = "top",
		align = "center",
	} = props;

	return (
		<Popover open={open} onOpenChange={onOpenChange}>
			<PopoverTrigger asChild>{children}</PopoverTrigger>
			<PopoverContent
				side={side}
				align={align}
				className="border-destructive/18 bg-background/96 w-[min(calc(100vw-1.5rem),21rem)] rounded-[1.25rem] p-3.5 shadow-[0_24px_60px_-34px_rgba(239,68,68,0.42)]"
			>
				<div className="flex gap-3">
					<div className="bg-destructive/10 text-destructive flex size-9 shrink-0 items-center justify-center rounded-full border border-destructive/14">
						<AlertTriangle className="size-4" />
					</div>
					<div className="min-w-0 flex-1 space-y-2.5">
						<div className="space-y-1">
							<p className="text-sm font-semibold tracking-tight">{title}</p>
							<p className="text-muted-foreground text-sm leading-6">
								{summary}
							</p>
						</div>
						<ErrorDetailDisclosure detail={detail} summary={summary} />
						{actions ? (
							<div className="flex flex-wrap gap-2">{actions}</div>
						) : null}
					</div>
				</div>
			</PopoverContent>
		</Popover>
	);
}
