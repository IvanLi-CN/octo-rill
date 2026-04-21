import { Eye, EyeOff } from "lucide-react";
import type * as React from "react";
import { useEffect, useId, useMemo, useState } from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type GitHubPatInputProps = Omit<React.ComponentProps<typeof Input>, "type"> & {
	inputClassName?: string;
	toggleClassName?: string;
};

export function GitHubPatInput({
	className,
	inputClassName,
	toggleClassName,
	id,
	value,
	defaultValue,
	"aria-label": ariaLabel,
	autoComplete,
	...props
}: GitHubPatInputProps) {
	const [isVisible, setIsVisible] = useState(false);
	const generatedId = useId();
	const inputId = id ?? `github-pat-${generatedId}`;
	const toggleLabel = isVisible ? "隐藏 GitHub PAT" : "显示 GitHub PAT";
	const secretState = useMemo(
		() => (isVisible ? "true" : "false"),
		[isVisible],
	);
	const secretValue = String(value ?? defaultValue ?? "");

	useEffect(() => {
		if (!secretValue) {
			setIsVisible(false);
		}
	}, [secretValue]);

	return (
		<div
			className={cn("relative", className)}
			data-secret-visible={secretState}
			data-secret-input="github-pat"
			data-secret-mask-mode={isVisible ? "plain-text" : "native-password"}
		>
			<Input
				{...props}
				id={inputId}
				type={isVisible ? "text" : "password"}
				autoComplete={autoComplete ?? "new-password"}
				aria-label={ariaLabel}
				data-secret-visible={secretState}
				data-secret-input="github-pat"
				data-secret-mask-mode={isVisible ? "plain-text" : "native-password"}
				data-1p-ignore="true"
				data-op-ignore="true"
				data-form-type="other"
				className={cn("pr-11", inputClassName)}
			/>
			<Button
				type="button"
				variant="ghost"
				size="icon"
				className={cn(
					"text-muted-foreground absolute top-1/2 right-1 size-8 -translate-y-1/2",
					toggleClassName,
				)}
				aria-label={toggleLabel}
				aria-controls={inputId}
				aria-pressed={isVisible}
				onClick={() => setIsVisible((value) => !value)}
			>
				{isVisible ? <EyeOff /> : <Eye />}
			</Button>
		</div>
	);
}
