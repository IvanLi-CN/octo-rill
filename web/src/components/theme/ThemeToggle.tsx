import { LaptopMinimal, MoonStar, SunMedium } from "lucide-react";

import { cn } from "@/lib/utils";
import { useTheme } from "@/theme/ThemeProvider";
import type { ThemePreference } from "@/theme/theme";

type ThemeOption = {
	value: ThemePreference;
	label: string;
	icon: typeof SunMedium;
};

const THEME_OPTIONS: ThemeOption[] = [
	{ value: "light", label: "浅色", icon: SunMedium },
	{ value: "dark", label: "深色", icon: MoonStar },
	{ value: "system", label: "跟随系统", icon: LaptopMinimal },
];

function resolveThemeOption(themePreference: ThemePreference): ThemeOption {
	return (
		THEME_OPTIONS.find((option) => option.value === themePreference) ??
		THEME_OPTIONS[2]
	);
}

export function ThemeToggle({ className }: { className?: string }) {
	const { themePreference, setThemePreference } = useTheme();

	return (
		<fieldset
			className={cn(
				"inline-flex items-center rounded-full border border-border/70 bg-background/85 p-1 shadow-sm backdrop-blur supports-[backdrop-filter]:bg-background/75",
				className,
			)}
			data-theme-toggle
			data-theme-preference={themePreference}
		>
			<legend className="sr-only">主题模式</legend>
			{THEME_OPTIONS.map((option) => {
				const OptionIcon = option.icon;
				const isActive =
					resolveThemeOption(themePreference).value === option.value;

				return (
					<button
						key={option.value}
						type="button"
						aria-label={option.label}
						aria-pressed={isActive}
						className={cn(
							"inline-flex size-8 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-accent/70 hover:text-foreground focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-1 focus-visible:outline-ring",
							isActive
								? "bg-card text-foreground shadow-sm ring-1 ring-border/70"
								: null,
						)}
						data-theme-option={option.value}
						onClick={() => setThemePreference(option.value)}
						title={option.label}
					>
						<OptionIcon className="size-4" />
						<span className="sr-only">{option.label}</span>
					</button>
				);
			})}
		</fieldset>
	);
}
