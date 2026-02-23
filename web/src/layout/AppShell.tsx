import type * as React from "react";

import { cn } from "@/lib/utils";

type AppShellProps = {
	header?: React.ReactNode;
	footer?: React.ReactNode;
	children: React.ReactNode;
};

export function AppShell({ header, footer, children }: AppShellProps) {
	return (
		<div className="min-h-screen">
			{header ? (
				<header className="supports-[backdrop-filter]:bg-background/70 bg-background/90 sticky top-0 z-20 border-b backdrop-blur">
					<div className={cn("mx-auto max-w-6xl px-6 py-4")}>{header}</div>
				</header>
			) : null}

			<main
				className={cn("mx-auto max-w-6xl px-6 py-8", footer ? "pb-16" : null)}
			>
				{children}
			</main>
			{footer}
		</div>
	);
}
