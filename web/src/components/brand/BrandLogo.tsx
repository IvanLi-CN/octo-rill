import { cn } from "@/lib/utils";
import { withBaseAssetPath } from "@/lib/asset-path";

type BrandLogoProps = {
	variant?: "mark" | "wordmark";
	theme?: "auto" | "light" | "dark";
	className?: string;
	imgClassName?: string;
	alt?: string;
};

const MARK_SRC = withBaseAssetPath("brand/mark.svg");
const WORDMARK_LIGHT_SRC = withBaseAssetPath("brand/wordmark-light.svg");
const WORDMARK_DARK_SRC = withBaseAssetPath("brand/wordmark-dark.svg");

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
		<picture className={cn("inline-flex h-full items-center", className)}>
			<source media="(prefers-color-scheme: dark)" srcSet={WORDMARK_DARK_SRC} />
			<img
				alt={alt}
				className={cn("block h-full w-auto", imgClassName)}
				loading="eager"
				src={WORDMARK_LIGHT_SRC}
			/>
		</picture>
	);
}
