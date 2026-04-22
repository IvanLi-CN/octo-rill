import { Eye, EyeOff } from "lucide-react";
import type * as React from "react";
import { useEffect, useId, useMemo, useRef, useState } from "react";

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
	style,
	onChange,
	onBeforeInput,
	onCopy,
	onCut,
	onDrop,
	onDragStart,
	onKeyDown,
	...props
}: GitHubPatInputProps) {
	const [isVisible, setIsVisible] = useState(false);
	const [localValue, setLocalValue] = useState(() =>
		String(defaultValue ?? ""),
	);
	const generatedId = useId();
	const inputId = id ?? `github-pat-${generatedId}`;
	const hiddenHintId = `${inputId}-hidden-hint`;
	const inputRef = useRef<HTMLInputElement | null>(null);
	const toggleLabel = isVisible ? "隐藏 GitHub PAT" : "显示 GitHub PAT";
	const isControlled = value !== undefined;
	const secretValue = isControlled ? String(value ?? "") : localValue;
	const secretState = useMemo(
		() => (isVisible ? "true" : "false"),
		[isVisible],
	);
	const inputStyle = useMemo<React.CSSProperties>(
		() => ({
			...style,
			...(isVisible
				? {}
				: ({
						WebkitTextSecurity: "disc",
					} as React.CSSProperties)),
		}),
		[isVisible, style],
	);

	useEffect(() => {
		if (!secretValue) {
			setIsVisible(false);
		}
	}, [secretValue]);

	const queueSelection = (
		nextSelectionStart: number,
		nextSelectionEnd = nextSelectionStart,
	) => {
		requestAnimationFrame(() => {
			const input = inputRef.current;
			if (!input || document.activeElement !== input) {
				return;
			}
			input.setSelectionRange(nextSelectionStart, nextSelectionEnd);
		});
	};

	const emitFallbackChange = (nextValue: string) => {
		if (!isControlled) {
			setLocalValue(nextValue);
		}
		onChange?.({
			target: { value: nextValue },
			currentTarget: { value: nextValue },
		} as React.ChangeEvent<HTMLInputElement>);
	};

	const applyFallbackMutation = (
		nextValue: string,
		nextSelectionStart: number,
		nextSelectionEnd = nextSelectionStart,
	) => {
		if (nextValue === secretValue) {
			return;
		}
		emitFallbackChange(nextValue);
		queueSelection(nextSelectionStart, nextSelectionEnd);
	};

	const performEditCommand = (
		commandName: "delete" | "insertText",
		selectionStart: number,
		selectionEnd: number,
		valueArgument?: string,
	) => {
		const input = inputRef.current;
		if (!input) {
			return false;
		}
		input.focus();
		input.setSelectionRange(selectionStart, selectionEnd);
		try {
			return document.execCommand(commandName, false, valueArgument);
		} catch {
			return false;
		}
	};

	const replaceSelection = (insertedText: string) => {
		const input = inputRef.current;
		if (!input) {
			return;
		}
		const selectionStart = input.selectionStart ?? secretValue.length;
		const selectionEnd = input.selectionEnd ?? selectionStart;
		const nextValue =
			secretValue.slice(0, selectionStart) +
			insertedText +
			secretValue.slice(selectionEnd);
		const nextCaret = selectionStart + insertedText.length;
		if (
			performEditCommand(
				"insertText",
				selectionStart,
				selectionEnd,
				insertedText,
			)
		) {
			return;
		}
		applyFallbackMutation(nextValue, nextCaret);
	};

	const deleteRange = (selectionStart: number, selectionEnd: number) => {
		if (selectionStart === selectionEnd) {
			return;
		}
		if (performEditCommand("delete", selectionStart, selectionEnd)) {
			return;
		}
		const nextValue =
			secretValue.slice(0, selectionStart) + secretValue.slice(selectionEnd);
		applyFallbackMutation(nextValue, selectionStart);
	};

	const isTokenBoundary = (character: string) => !/[A-Za-z0-9]/.test(character);

	const findBackwardWordBoundary = (input: string, cursor: number) => {
		let nextCursor = cursor;
		while (nextCursor > 0 && isTokenBoundary(input[nextCursor - 1] ?? "")) {
			nextCursor -= 1;
		}
		while (nextCursor > 0 && !isTokenBoundary(input[nextCursor - 1] ?? "")) {
			nextCursor -= 1;
		}
		return nextCursor;
	};

	const findForwardWordBoundary = (input: string, cursor: number) => {
		let nextCursor = cursor;
		while (
			nextCursor < input.length &&
			isTokenBoundary(input[nextCursor] ?? "")
		) {
			nextCursor += 1;
		}
		while (
			nextCursor < input.length &&
			!isTokenBoundary(input[nextCursor] ?? "")
		) {
			nextCursor += 1;
		}
		return nextCursor;
	};

	const deleteWord = (direction: "backward" | "forward") => {
		const input = inputRef.current;
		if (!input) {
			return;
		}
		const selectionStart = input.selectionStart ?? secretValue.length;
		const selectionEnd = input.selectionEnd ?? selectionStart;
		if (selectionStart !== selectionEnd) {
			deleteRange(selectionStart, selectionEnd);
			return;
		}
		const rangeStart =
			direction === "backward"
				? findBackwardWordBoundary(secretValue, selectionStart)
				: selectionStart;
		const rangeEnd =
			direction === "forward"
				? findForwardWordBoundary(secretValue, selectionStart)
				: selectionStart;
		if (rangeStart === rangeEnd) {
			return;
		}
		deleteRange(rangeStart, rangeEnd);
	};

	const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
		if (!isControlled) {
			setLocalValue(event.target.value);
		}
		onChange?.(event);
	};

	const assistiveLabel = ariaLabel ?? "GitHub PAT";

	return (
		<div
			className={cn("relative", className)}
			data-secret-visible={secretState}
			data-secret-input="github-pat"
			data-secret-mask-mode={isVisible ? "plain-text" : "visual-mask"}
		>
			{!isVisible ? (
				<span className="sr-only" id={hiddenHintId}>
					当前内容已隐藏，仍可直接编辑。使用“显示 GitHub
					PAT”按钮可临时查看明文。
				</span>
			) : null}
			<Input
				{...props}
				onBeforeInput={onBeforeInput}
				ref={inputRef}
				id={inputId}
				type="text"
				value={secretValue}
				autoComplete={autoComplete ?? "off"}
				aria-describedby={isVisible ? undefined : hiddenHintId}
				aria-label={assistiveLabel}
				data-secret-visible={secretState}
				data-secret-input="github-pat"
				data-secret-mask-mode={isVisible ? "plain-text" : "visual-mask"}
				data-1p-ignore="true"
				data-op-ignore="true"
				data-form-type="other"
				inputMode="text"
				style={inputStyle}
				onChange={handleChange}
				onCopy={(event) => {
					if (!isVisible) {
						event.preventDefault();
						return;
					}
					onCopy?.(event);
				}}
				onCut={(event) => {
					if (!isVisible) {
						event.preventDefault();
						return;
					}
					onCut?.(event);
				}}
				onDragStart={(event) => {
					if (!isVisible) {
						event.preventDefault();
						return;
					}
					onDragStart?.(event);
				}}
				onDrop={(event) => {
					if (!isVisible && !props.readOnly && !props.disabled) {
						const droppedText =
							event.dataTransfer.getData("text/plain") ||
							event.dataTransfer.getData("text");
						if (droppedText) {
							event.preventDefault();
							replaceSelection(droppedText);
							return;
						}
						return;
					}
					onDrop?.(event);
				}}
				onKeyDown={(event) => {
					if (!isVisible && !props.readOnly && !props.disabled) {
						const isWordBackwardShortcut =
							event.key === "Backspace" &&
							(event.altKey || event.ctrlKey) &&
							!event.metaKey;
						if (isWordBackwardShortcut) {
							event.preventDefault();
							deleteWord("backward");
							return;
						}
						const isWordForwardShortcut =
							event.key === "Delete" &&
							(event.altKey || event.ctrlKey) &&
							!event.metaKey;
						if (isWordForwardShortcut) {
							event.preventDefault();
							deleteWord("forward");
							return;
						}
					}
					onKeyDown?.(event);
				}}
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
				onMouseDown={(event) => {
					if (!isVisible) {
						event.preventDefault();
					}
				}}
				onClick={(event) => {
					const nextVisible = !isVisible;
					setIsVisible(nextVisible);
					requestAnimationFrame(() => {
						if (nextVisible) {
							inputRef.current?.focus();
							return;
						}
						event.currentTarget.focus();
					});
				}}
			>
				{isVisible ? <EyeOff /> : <Eye />}
			</Button>
		</div>
	);
}
