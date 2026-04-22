import { Eye, EyeOff } from "lucide-react";
import type * as React from "react";
import {
	useCallback,
	useEffect,
	useId,
	useMemo,
	useRef,
	useState,
} from "react";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

type GitHubPatInputProps = Omit<React.ComponentProps<typeof Input>, "type"> & {
	inputClassName?: string;
	toggleClassName?: string;
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

export function GitHubPatInput({
	className,
	inputClassName,
	toggleClassName,
	id,
	value,
	defaultValue,
	"aria-label": ariaLabel,
	autoComplete,
	onChange,
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
	const historyInputTypeRef = useRef<"historyUndo" | "historyRedo" | null>(
		null,
	);
	const toggleLabel = isVisible ? "隐藏 GitHub PAT" : "显示 GitHub PAT";
	const isControlled = value !== undefined;
	const secretValue = isControlled ? String(value ?? "") : localValue;
	const historyRef = useRef<string[]>([secretValue]);
	const historyIndexRef = useRef(0);
	const secretState = useMemo(
		() => (isVisible ? "true" : "false"),
		[isVisible],
	);

	useEffect(() => {
		if (!secretValue) {
			setIsVisible(false);
		}
	}, [secretValue]);

	useEffect(() => {
		const currentHistoryValue =
			historyRef.current[historyIndexRef.current] ?? "";
		if (currentHistoryValue === secretValue) {
			return;
		}
		historyRef.current = [secretValue];
		historyIndexRef.current = 0;
	}, [secretValue]);

	const queueSelection = useCallback(
		(nextSelectionStart: number, nextSelectionEnd = nextSelectionStart) => {
			requestAnimationFrame(() => {
				const input = inputRef.current;
				if (!input || document.activeElement !== input) {
					return;
				}
				input.setSelectionRange(nextSelectionStart, nextSelectionEnd);
			});
		},
		[],
	);

	const recordHistory = useCallback((nextValue: string) => {
		const currentHistoryValue =
			historyRef.current[historyIndexRef.current] ?? "";
		if (currentHistoryValue === nextValue) {
			return;
		}
		historyRef.current = [
			...historyRef.current.slice(0, historyIndexRef.current + 1),
			nextValue,
		];
		historyIndexRef.current = historyRef.current.length - 1;
	}, []);

	const emitSyntheticChange = useCallback(
		(nextValue: string) => {
			recordHistory(nextValue);
			if (!isControlled) {
				setLocalValue(nextValue);
			}
			onChange?.({
				target: { value: nextValue },
				currentTarget: { value: nextValue },
			} as React.ChangeEvent<HTMLInputElement>);
		},
		[isControlled, onChange, recordHistory],
	);

	const applyHiddenMutation = useCallback(
		(
			nextValue: string,
			nextSelectionStart: number,
			nextSelectionEnd = nextSelectionStart,
		) => {
			if (nextValue === secretValue) {
				return;
			}
			emitSyntheticChange(nextValue);
			queueSelection(nextSelectionStart, nextSelectionEnd);
		},
		[emitSyntheticChange, queueSelection, secretValue],
	);

	const replaceSelection = (insertedText: string) => {
		const input = inputRef.current;
		if (!input) {
			return;
		}
		input.focus();
		const selectionStart = input.selectionStart ?? secretValue.length;
		const selectionEnd = input.selectionEnd ?? selectionStart;
		const nextValue =
			secretValue.slice(0, selectionStart) +
			insertedText +
			secretValue.slice(selectionEnd);
		const nextCaret = selectionStart + insertedText.length;
		applyHiddenMutation(nextValue, nextCaret);
	};

	const resolveDropCaret = (input: HTMLInputElement, clientX: number) => {
		if (!Number.isFinite(clientX)) {
			const selectionStart = input.selectionStart ?? secretValue.length;
			return selectionStart;
		}
		const rect = input.getBoundingClientRect();
		const styles = getComputedStyle(input);
		const leftInset =
			parseFloat(styles.borderLeftWidth || "0") +
			parseFloat(styles.paddingLeft || "0");
		const rightInset =
			parseFloat(styles.borderRightWidth || "0") +
			parseFloat(styles.paddingRight || "0");
		const contentWidth = Math.max(0, rect.width - leftInset - rightInset);
		const relativeX = Math.min(
			Math.max(clientX - rect.left - leftInset + input.scrollLeft, 0),
			contentWidth + input.scrollLeft,
		);
		const context = document.createElement("canvas").getContext("2d");
		if (!context) {
			return input.selectionStart ?? secretValue.length;
		}
		context.font =
			styles.font ||
			`${styles.fontStyle} ${styles.fontVariant} ${styles.fontWeight} ${styles.fontSize} ${styles.fontFamily}`;

		let previousWidth = 0;
		for (let index = 0; index <= secretValue.length; index += 1) {
			const currentWidth = context.measureText(
				secretValue.slice(0, index),
			).width;
			if (relativeX <= currentWidth) {
				return relativeX - previousWidth <= currentWidth - relativeX
					? index - 1
					: index;
			}
			previousWidth = currentWidth;
		}
		return secretValue.length;
	};

	const updateDropSelection = (clientX: number) => {
		const input = inputRef.current;
		if (!input) {
			return;
		}
		input.focus();
		const nextCaret = Math.max(0, resolveDropCaret(input, clientX));
		input.setSelectionRange(nextCaret, nextCaret);
	};

	const deleteRange = useCallback(
		(selectionStart: number, selectionEnd: number) => {
			if (selectionStart === selectionEnd) {
				return;
			}
			const nextValue =
				secretValue.slice(0, selectionStart) + secretValue.slice(selectionEnd);
			applyHiddenMutation(nextValue, selectionStart);
		},
		[applyHiddenMutation, secretValue],
	);

	const deleteWord = useCallback(
		(direction: "backward" | "forward") => {
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
		},
		[deleteRange, secretValue],
	);

	const replayHistory = useCallback(
		(direction: "undo" | "redo") => {
			const currentIndex = historyIndexRef.current;
			const nextIndex =
				direction === "undo"
					? Math.max(0, currentIndex - 1)
					: Math.min(historyRef.current.length - 1, currentIndex + 1);
			if (nextIndex === currentIndex) {
				return;
			}
			historyIndexRef.current = nextIndex;
			const nextValue = historyRef.current[nextIndex] ?? "";
			if (!isControlled) {
				setLocalValue(nextValue);
			}
			onChange?.({
				target: { value: nextValue },
				currentTarget: { value: nextValue },
			} as React.ChangeEvent<HTMLInputElement>);
			queueSelection(nextValue.length);
		},
		[isControlled, onChange, queueSelection],
	);

	useEffect(() => {
		const input = inputRef.current;
		if (!input) {
			return;
		}

		const replayNativeHistory = (direction: "undo" | "redo") => {
			const currentIndex = historyIndexRef.current;
			const nextIndex =
				direction === "undo"
					? Math.max(0, currentIndex - 1)
					: Math.min(historyRef.current.length - 1, currentIndex + 1);
			if (nextIndex === currentIndex) {
				return false;
			}
			historyIndexRef.current = nextIndex;
			const nextValue = historyRef.current[nextIndex] ?? "";
			if (!isControlled) {
				setLocalValue(nextValue);
			}
			onChange?.({
				target: { value: nextValue },
				currentTarget: { value: nextValue },
			} as React.ChangeEvent<HTMLInputElement>);
			queueSelection(nextValue.length);
			return true;
		};

		const handleBeforeInput = (event: InputEvent) => {
			if (isVisible || props.readOnly || props.disabled) {
				return;
			}
			let shouldHandle = false;
			if (event.inputType === "historyUndo") {
				shouldHandle = replayNativeHistory("undo");
			} else if (event.inputType === "historyRedo") {
				shouldHandle = replayNativeHistory("redo");
			} else if (event.inputType === "deleteWordBackward") {
				deleteWord("backward");
				shouldHandle = true;
			} else if (event.inputType === "deleteWordForward") {
				deleteWord("forward");
				shouldHandle = true;
			}
			if (shouldHandle) {
				if (
					event.inputType === "historyUndo" ||
					event.inputType === "historyRedo"
				) {
					historyInputTypeRef.current = event.inputType;
				}
				event.preventDefault();
			}
		};

		const handleInput = (event: Event) => {
			if (isVisible || props.readOnly || props.disabled) {
				return;
			}
			const nativeEvent = event as InputEvent;
			if (
				historyInputTypeRef.current &&
				historyInputTypeRef.current === nativeEvent.inputType
			) {
				historyInputTypeRef.current = null;
				return;
			}
			if (nativeEvent.inputType === "historyUndo") {
				replayNativeHistory("undo");
				return;
			}
			if (nativeEvent.inputType === "historyRedo") {
				replayNativeHistory("redo");
			}
		};

		const handleNativeKeyDown = (event: KeyboardEvent) => {
			if (isVisible || props.readOnly || props.disabled) {
				return;
			}
			const isWordBackwardShortcut =
				event.key === "Backspace" &&
				(event.altKey || event.ctrlKey) &&
				!event.metaKey;
			if (isWordBackwardShortcut) {
				event.preventDefault();
				event.stopPropagation();
				deleteWord("backward");
				return;
			}
			const isWordForwardShortcut =
				event.key === "Delete" &&
				(event.altKey || event.ctrlKey) &&
				!event.metaKey;
			if (isWordForwardShortcut) {
				event.preventDefault();
				event.stopPropagation();
				deleteWord("forward");
				return;
			}
			const wantsUndo =
				event.key.toLowerCase() === "z" &&
				((event.metaKey && !event.ctrlKey) ||
					(event.ctrlKey && !event.metaKey)) &&
				!event.shiftKey;
			if (wantsUndo) {
				event.preventDefault();
				event.stopPropagation();
				replayHistory("undo");
				return;
			}
			const wantsRedo =
				(event.key.toLowerCase() === "z" &&
					((event.metaKey && !event.ctrlKey) ||
						(event.ctrlKey && !event.metaKey)) &&
					event.shiftKey) ||
				(event.key.toLowerCase() === "y" && event.ctrlKey && !event.metaKey);
			if (wantsRedo) {
				event.preventDefault();
				event.stopPropagation();
				replayHistory("redo");
			}
		};

		input.addEventListener("keydown", handleNativeKeyDown);
		input.addEventListener("beforeinput", handleBeforeInput);
		input.addEventListener("input", handleInput);
		return () => {
			input.removeEventListener("keydown", handleNativeKeyDown);
			input.removeEventListener("beforeinput", handleBeforeInput);
			input.removeEventListener("input", handleInput);
		};
	}, [
		isVisible,
		isControlled,
		onChange,
		props.disabled,
		props.readOnly,
		queueSelection,
		deleteWord,
		replayHistory,
	]);

	const handleChange = (event: React.ChangeEvent<HTMLInputElement>) => {
		recordHistory(event.target.value);
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
			data-secret-mask-mode={isVisible ? "plain-text" : "native-password"}
		>
			{!isVisible ? (
				<span className="sr-only" id={hiddenHintId}>
					当前内容已隐藏，仍可直接编辑。使用“显示 GitHub
					PAT”按钮可临时查看明文。
				</span>
			) : null}
			<Input
				{...props}
				ref={inputRef}
				id={inputId}
				type={isVisible ? "text" : "password"}
				value={secretValue}
				autoComplete={autoComplete ?? "new-password"}
				aria-describedby={isVisible ? undefined : hiddenHintId}
				aria-label={assistiveLabel}
				data-secret-visible={secretState}
				data-secret-input="github-pat"
				data-secret-mask-mode={isVisible ? "plain-text" : "native-password"}
				data-1p-ignore="true"
				data-op-ignore="true"
				data-form-type="other"
				inputMode="text"
				onChange={handleChange}
				onCopy={(event) => {
					if (!isVisible) {
						event.preventDefault();
					}
				}}
				onCut={(event) => {
					if (!isVisible) {
						event.preventDefault();
					}
				}}
				onDragStart={(event) => {
					if (!isVisible) {
						event.preventDefault();
					}
					props.onDragStart?.(event);
				}}
				onDragOver={(event) => {
					if (!isVisible && !props.readOnly && !props.disabled) {
						event.preventDefault();
						updateDropSelection(event.clientX);
						if (event.dataTransfer) {
							event.dataTransfer.dropEffect = "copy";
						}
					}
					props.onDragOver?.(event);
				}}
				onDrop={(event) => {
					if (!isVisible && !props.readOnly && !props.disabled) {
						const droppedText =
							event.dataTransfer.getData("text/plain") ||
							event.dataTransfer.getData("text");
						if (droppedText) {
							event.preventDefault();
							updateDropSelection(event.clientX);
							replaceSelection(droppedText);
						}
					}
					props.onDrop?.(event);
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
						const wantsUndo =
							event.key.toLowerCase() === "z" &&
							((event.metaKey && !event.ctrlKey) ||
								(event.ctrlKey && !event.metaKey)) &&
							!event.shiftKey;
						if (wantsUndo) {
							event.preventDefault();
							replayHistory("undo");
							return;
						}
						const wantsRedo =
							(event.key.toLowerCase() === "z" &&
								((event.metaKey && !event.ctrlKey) ||
									(event.ctrlKey && !event.metaKey)) &&
								event.shiftKey) ||
							(event.key.toLowerCase() === "y" &&
								event.ctrlKey &&
								!event.metaKey);
						if (wantsRedo) {
							event.preventDefault();
							replayHistory("redo");
							return;
						}
					}
					props.onKeyDown?.(event);
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
					const toggleButton = event.currentTarget;
					setIsVisible(nextVisible);
					requestAnimationFrame(() => {
						if (nextVisible) {
							inputRef.current?.focus();
							return;
						}
						toggleButton.focus();
					});
				}}
			>
				{isVisible ? <EyeOff /> : <Eye />}
			</Button>
		</div>
	);
}
