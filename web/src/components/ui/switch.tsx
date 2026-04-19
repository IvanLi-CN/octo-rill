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
				"relative inline-flex h-7 w-12 shrink-0 rounded-full border p-0.5 shadow-sm outline-none transition-[background-color,border-color,box-shadow,opacity] duration-200 ease-out",
				"focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50",
				checked
					? "border-primary bg-primary text-primary-foreground hover:bg-primary/92"
					: "border-border/80 bg-muted/70 text-muted-foreground hover:bg-muted",
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
					"pointer-events-none inline-flex size-5 items-center justify-center rounded-full bg-background shadow-sm ring-1 ring-black/5 transition-transform duration-200 ease-out",
					checked ? "translate-x-6" : "translate-x-0",
				)}
			/>
		</button>
	);
}
