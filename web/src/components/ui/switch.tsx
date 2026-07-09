import type { ButtonHTMLAttributes } from "react";

import { cn } from "@/lib/utils";

export type SwitchProps = Omit<
	ButtonHTMLAttributes<HTMLButtonElement>,
	"onChange"
> & {
	checked: boolean;
	onCheckedChange?: (checked: boolean) => void;
};

export function Switch(props: SwitchProps) {
	const {
		checked,
		onCheckedChange,
		className,
		disabled = false,
		type,
		...rest
	} = props;

	return (
		<button
			{...rest}
			type={type ?? "button"}
			role="switch"
			aria-checked={checked}
			data-state={checked ? "checked" : "unchecked"}
			disabled={disabled}
			className={cn(
				"relative inline-flex h-7 w-12 shrink-0 rounded-full border shadow-sm outline-none transition-[background-color,border-color,box-shadow,opacity] duration-200 ease-out",
				"focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
				checked
					? "border-emerald-500/60 bg-emerald-500/85 text-white hover:bg-emerald-500/78 dark:border-emerald-400/55 dark:bg-emerald-500/72 dark:hover:bg-emerald-500/68"
					: "border-border/80 bg-muted/75 text-muted-foreground hover:border-foreground/15 hover:bg-muted/90 dark:bg-input/65",
				disabled && "cursor-not-allowed opacity-55",
				className,
			)}
			onClick={() => {
				if (disabled) return;
				onCheckedChange?.(!checked);
			}}
		>
			<span
				aria-hidden="true"
				className={cn(
					"pointer-events-none absolute top-1/2 left-[3px] size-5 -translate-y-1/2 rounded-full bg-white shadow-[0_1px_3px_rgba(15,23,42,0.32)] ring-1 ring-black/8 transition-transform duration-200 ease-out",
					checked ? "translate-x-5" : "translate-x-0",
				)}
			/>
		</button>
	);
}
