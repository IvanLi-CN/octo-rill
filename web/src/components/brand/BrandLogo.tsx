import { cn } from "@/lib/utils";
import { withBaseAssetPath } from "@/lib/asset-path";
import { useTheme } from "@/theme/ThemeProvider";

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
	const { resolvedTheme } = useTheme();

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

	const resolvedWordmarkTheme = theme === "auto" ? resolvedTheme : theme;

	return (
		<img
			alt={alt}
			className={cn("h-full w-auto", className, imgClassName)}
			loading="eager"
			src={
				resolvedWordmarkTheme === "dark"
					? WORDMARK_DARK_SRC
					: WORDMARK_LIGHT_SRC
			}
		/>
	);
}
