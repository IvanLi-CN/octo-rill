import type * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { Slot } from "radix-ui";

import { cn } from "@/lib/utils";

const chipVariants = cva(
	"inline-flex h-6 w-fit shrink-0 items-center justify-center gap-1.5 overflow-hidden rounded-full border px-2.5 text-xs font-medium leading-4 whitespace-nowrap outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 [&>svg]:pointer-events-none [&>svg]:size-3.5",
	{
		variants: {
			variant: {
				neutral: "border-border/70 bg-card/80 text-foreground/80 shadow-sm",
				destructive: "border-destructive/30 bg-destructive/10 text-destructive",
			},
		},
		defaultVariants: {
			variant: "neutral",
		},
	},
);

function Chip({
	className,
	variant,
	asChild = false,
	...props
}: React.ComponentProps<"span"> &
	VariantProps<typeof chipVariants> & { asChild?: boolean }) {
	const Comp = asChild ? Slot.Root : "span";

	return (
		<Comp
			data-slot="chip"
			data-variant={variant}
			className={cn(chipVariants({ variant }), className)}
			{...props}
		/>
	);
}

export { Chip, chipVariants };
