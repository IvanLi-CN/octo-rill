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

function getNextThemePreference(
	themePreference: ThemePreference,
): ThemePreference {
	const currentIndex = THEME_OPTIONS.findIndex(
		(option) => option.value === themePreference,
	);
	const nextIndex =
		currentIndex < 0 ? 0 : (currentIndex + 1) % THEME_OPTIONS.length;
	return THEME_OPTIONS[nextIndex]?.value ?? "system";
}

export function ThemeToggle(props: { className?: string; compact?: boolean }) {
	const { className, compact = false } = props;
	const { themePreference, setThemePreference } = useTheme();
	const activeOption = resolveThemeOption(themePreference);

	if (compact) {
		const ActiveIcon = activeOption.icon;
		const nextPreference = getNextThemePreference(themePreference);
		const nextOption = resolveThemeOption(nextPreference);

		return (
			<button
				type="button"
				className={cn(
					"inline-flex size-9 items-center justify-center rounded-full border border-border/70 bg-background/85 text-foreground shadow-sm backdrop-blur transition-colors motion-safe:transition-[width,height,padding,transform,background-color,border-color] motion-safe:duration-200 motion-safe:ease-out hover:bg-accent/70 focus-visible:border-ring focus-visible:ring-ring/50 focus-visible:ring-[3px] focus-visible:outline-1 focus-visible:outline-ring supports-[backdrop-filter]:bg-background/75",
					className,
				)}
				data-theme-toggle
				data-theme-preference={themePreference}
				data-theme-toggle-compact="true"
				aria-label={`主题模式：${activeOption.label}（点击切换到${nextOption.label}）`}
				title={`当前：${activeOption.label} · 点击切换到${nextOption.label}`}
				onClick={() => setThemePreference(nextPreference)}
			>
				<ActiveIcon className="size-4" />
				<span className="sr-only">{`当前主题模式：${activeOption.label}`}</span>
			</button>
		);
	}

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
				const isActive = activeOption.value === option.value;

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
