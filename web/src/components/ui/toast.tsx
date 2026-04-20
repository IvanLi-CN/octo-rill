import type * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { XIcon } from "lucide-react";
import { Toast as ToastPrimitive } from "radix-ui";

import { cn } from "@/lib/utils";

function ToastProvider({
	label = "通知",
	...props
}: React.ComponentProps<typeof ToastPrimitive.Provider>) {
	return <ToastPrimitive.Provider label={label} {...props} />;
}

const toastVariants = cva(
	"group pointer-events-auto relative flex w-full items-start gap-3 overflow-hidden rounded-[1.35rem] border p-4 shadow-lg backdrop-blur supports-[backdrop-filter]:bg-background/96 data-[state=closed]:animate-out data-[state=closed]:fade-out-0 data-[state=closed]:slide-out-to-right-full data-[state=open]:animate-in data-[state=open]:fade-in-0 data-[state=open]:slide-in-from-top-2 data-[swipe=end]:animate-out data-[swipe=end]:slide-out-to-right-full data-[swipe=move]:translate-x-(--radix-toast-swipe-move-x)",
	{
		variants: {
			variant: {
				default: "border-border/80 bg-background text-foreground",
				destructive:
					"border-destructive/22 bg-background/96 text-foreground shadow-[0_22px_48px_-30px_rgba(239,68,68,0.4)] ring-1 ring-destructive/8 dark:bg-destructive/15",
			},
		},
		defaultVariants: {
			variant: "default",
		},
	},
);

function Toast({
	className,
	variant,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Root> &
	VariantProps<typeof toastVariants>) {
	return (
		<ToastPrimitive.Root
			data-slot="toast"
			className={cn(toastVariants({ variant }), className)}
			{...props}
		/>
	);
}

function ToastTitle({
	className,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Title>) {
	return (
		<ToastPrimitive.Title
			data-slot="toast-title"
			className={cn("text-sm font-semibold tracking-tight", className)}
			{...props}
		/>
	);
}

function ToastDescription({
	className,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Description>) {
	return (
		<ToastPrimitive.Description
			data-slot="toast-description"
			className={cn("text-sm leading-6 text-muted-foreground", className)}
			{...props}
		/>
	);
}

function ToastAction({
	className,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Action>) {
	return (
		<ToastPrimitive.Action
			data-slot="toast-action"
			className={cn(
				"inline-flex h-8 shrink-0 items-center justify-center rounded-full border border-border/70 px-3 font-mono text-xs transition-colors hover:bg-accent hover:text-accent-foreground focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none group-data-[variant=destructive]:border-destructive/16 group-data-[variant=destructive]:bg-destructive/[0.035] group-data-[variant=destructive]:hover:bg-destructive/[0.08]",
				className,
			)}
			{...props}
		/>
	);
}

function ToastClose({
	className,
	children,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Close>) {
	return (
		<ToastPrimitive.Close
			data-slot="toast-close"
			className={cn(
				"text-muted-foreground absolute right-3 top-3 inline-flex size-8 items-center justify-center rounded-full border border-border/55 bg-background/82 shadow-sm transition-colors hover:bg-accent hover:text-foreground focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-none group-data-[variant=destructive]:border-destructive/16 group-data-[variant=destructive]:bg-background/92",
				className,
			)}
			toast-close=""
			{...props}
		>
			{children ?? <XIcon className="size-4" />}
		</ToastPrimitive.Close>
	);
}

function ToastViewport({
	className,
	...props
}: React.ComponentProps<typeof ToastPrimitive.Viewport>) {
	return (
		<ToastPrimitive.Viewport
			data-slot="toast-viewport"
			className={cn(
				"pointer-events-none fixed right-4 top-4 z-[60] flex max-h-[calc(100vh-2rem)] w-[min(calc(100vw-2rem),24rem)] flex-col gap-3 outline-none sm:right-6",
				className,
			)}
			{...props}
		/>
	);
}

export {
	Toast,
	ToastAction,
	ToastClose,
	ToastDescription,
	ToastProvider,
	ToastTitle,
	ToastViewport,
};
