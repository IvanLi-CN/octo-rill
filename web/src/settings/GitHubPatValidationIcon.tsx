import type { SVGProps } from "react";

import { cn } from "@/lib/utils";

export function GitHubPatValidationIcon({
	className,
	...props
}: SVGProps<SVGSVGElement>) {
	return (
		<svg
			aria-hidden="true"
			className={cn("pat-validation-icon size-[18px] shrink-0", className)}
			data-pat-validation-icon="true"
			fill="none"
			viewBox="0 0 24 24"
			{...props}
		>
			<path
				className="pat-validation-icon-corners"
				d="M8 3H5a2 2 0 0 0-2 2v3M16 3h3a2 2 0 0 1 2 2v3M3 16v3a2 2 0 0 0 2 2h3M21 16v3a2 2 0 0 1-2 2h-3"
			/>
			<path
				className="pat-validation-icon-beam"
				data-pat-validation-beam="true"
				d="M7 12h10"
			/>
		</svg>
	);
}
