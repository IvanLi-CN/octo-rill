import { cn } from "@/lib/utils";

type BrandLogoProps = {
	variant?: "mark" | "wordmark";
	theme?: "auto" | "light" | "dark";
	className?: string;
	imgClassName?: string;
	alt?: string;
};

const MARK_SRC = "/brand/mark.svg";
const WORDMARK_LIGHT_SRC = "/brand/wordmark-light.svg";
const WORDMARK_DARK_SRC = "/brand/wordmark-dark.svg";

export function BrandLogo({
	variant = "wordmark",
	theme = "auto",
	className,
	imgClassName,
	alt = "OctoRill",
}: BrandLogoProps) {
	if (variant === "mark") {
		return (
			<img
				alt={alt}
				className={cn("h-full w-auto", className, imgClassName)}
				loading="eager"
				src={MARK_SRC}
			/>
		);
	}

	if (theme === "light") {
		return (
			<img
				alt={alt}
				className={cn("h-full w-auto", className, imgClassName)}
				loading="eager"
				src={WORDMARK_LIGHT_SRC}
			/>
		);
	}

	if (theme === "dark") {
		return (
			<img
				alt={alt}
				className={cn("h-full w-auto", className, imgClassName)}
				loading="eager"
				src={WORDMARK_DARK_SRC}
			/>
		);
	}

	return (
		<span
			aria-label={alt}
			className={cn("inline-flex h-full items-center", className)}
			role="img"
		>
			<img
				alt=""
				aria-hidden="true"
				className={cn("block h-full w-auto dark:hidden", imgClassName)}
				loading="eager"
				src={WORDMARK_LIGHT_SRC}
			/>
			<img
				alt=""
				aria-hidden="true"
				className={cn("hidden h-full w-auto dark:block", imgClassName)}
				loading="eager"
				src={WORDMARK_DARK_SRC}
			/>
		</span>
	);
}
