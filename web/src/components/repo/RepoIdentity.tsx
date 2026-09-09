import type { ReactNode } from "react";
import {
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";

import type { RepoVisual } from "@/lib/repoVisual";
import { resolveRepoVisualCandidates } from "@/lib/repoVisual";
import { cn } from "@/lib/utils";

const MAX_REPO_IDENTITY_LINES = 2;
const REPO_TAIL_LENGTH = 8;

type RepoIdentityDisplay = {
	mode: "full" | "compact";
	text: string;
};

function splitRepoFullName(repoFullName: string) {
	const slashIndex = repoFullName.indexOf("/");
	if (slashIndex <= 0) {
		return { owner: "", repo: repoFullName };
	}
	return {
		owner: repoFullName.slice(0, slashIndex),
		repo: repoFullName.slice(slashIndex + 1),
	};
}

function middleEllipsis(value: string, head: number, tail: number) {
	const characters = Array.from(value);
	if (characters.length <= head + tail + 1) {
		return value;
	}
	return `${characters.slice(0, head).join("")}…${characters
		.slice(-tail)
		.join("")}`;
}

function buildCompactRepoFullName(
	repoFullName: string,
	ownerHead: number,
	ownerTail: number,
	repoHead: number,
) {
	const { owner, repo } = splitRepoFullName(repoFullName);
	const compactRepo = middleEllipsis(repo, repoHead, REPO_TAIL_LENGTH);
	if (!owner) return compactRepo;
	return `${middleEllipsis(owner, ownerHead, ownerTail)}/${compactRepo}`;
}

function compactRepoFullNameCandidates(repoFullName: string) {
	const { owner } = splitRepoFullName(repoFullName);
	const ownerVariants = owner
		? ([
				[Number.MAX_SAFE_INTEGER, 0],
				[12, 4],
				[8, 4],
				[4, 4],
			] as const)
		: ([[0, 0]] as const);
	const repoHeads = [16, 12, 8, 4];

	return ownerVariants.flatMap(([ownerHead, ownerTail]) =>
		repoHeads.map((repoHead) =>
			buildCompactRepoFullName(repoFullName, ownerHead, ownerTail, repoHead),
		),
	);
}

function lineHeightInPixels(element: HTMLElement) {
	const computedStyle = window.getComputedStyle(element);
	const lineHeight = Number.parseFloat(computedStyle.lineHeight);
	if (Number.isFinite(lineHeight)) return lineHeight;
	const fontSize = Number.parseFloat(computedStyle.fontSize);
	return Number.isFinite(fontSize) ? fontSize * 1.2 : 16 * 1.2;
}

export function RepoIdentity(props: {
	repoFullName: string | null;
	repoVisual?: RepoVisual | null;
	labelAs?: "span" | "h1";
	className?: string;
	labelClassName?: string;
	visualClassName?: string;
	labelSuffix?: ReactNode;
	children?: ReactNode;
}) {
	const {
		repoFullName,
		repoVisual = null,
		className,
		labelAs = "span",
		labelClassName,
		visualClassName,
		labelSuffix,
		children,
	} = props;

	if (!repoFullName) return null;

	const resetKey = [
		repoFullName,
		repoVisual?.owner_avatar_url ?? "",
		repoVisual?.open_graph_image_url ?? "",
		repoVisual?.uses_custom_open_graph_image ? "1" : "0",
	].join("|");

	return (
		<RepoIdentityContent
			key={resetKey}
			repoFullName={repoFullName}
			repoVisual={repoVisual}
			className={className}
			labelAs={labelAs}
			labelClassName={labelClassName}
			visualClassName={visualClassName}
			labelSuffix={labelSuffix}
		>
			{children}
		</RepoIdentityContent>
	);
}

function RepoIdentityContent(props: {
	repoFullName: string;
	repoVisual: RepoVisual | null;
	labelAs: "span" | "h1";
	className?: string;
	labelClassName?: string;
	visualClassName?: string;
	labelSuffix?: ReactNode;
	children?: ReactNode;
}) {
	const {
		repoFullName,
		repoVisual,
		className,
		labelAs,
		labelClassName,
		visualClassName,
		labelSuffix,
		children,
	} = props;
	const candidates = useMemo(
		() => resolveRepoVisualCandidates(repoVisual),
		[repoVisual],
	);
	const [failedCandidateKeys, setFailedCandidateKeys] = useState<Set<string>>(
		() => new Set(),
	);
	const [repoIdentityDisplay, setRepoIdentityDisplay] =
		useState<RepoIdentityDisplay>({ mode: "full", text: repoFullName });
	const compactCandidates = useMemo(
		() => compactRepoFullNameCandidates(repoFullName),
		[repoFullName],
	);
	const autoRetryPendingRef = useRef(true);
	const labelRowRef = useRef<HTMLDivElement | null>(null);
	const labelRef = useRef<HTMLElement | null>(null);

	useEffect(() => {
		if (failedCandidateKeys.size === 0) return;

		const resetFallbacks = () => {
			setFailedCandidateKeys((current) =>
				current.size === 0 ? current : new Set(),
			);
		};
		const handleVisibilityChange = () => {
			if (document.visibilityState === "visible") {
				resetFallbacks();
			}
		};

		const retryTimer = autoRetryPendingRef.current
			? window.setTimeout(() => {
					autoRetryPendingRef.current = false;
					resetFallbacks();
				}, 15_000)
			: null;

		window.addEventListener("focus", resetFallbacks);
		window.addEventListener("online", resetFallbacks);
		document.addEventListener("visibilitychange", handleVisibilityChange);
		return () => {
			if (retryTimer !== null) {
				window.clearTimeout(retryTimer);
			}
			window.removeEventListener("focus", resetFallbacks);
			window.removeEventListener("online", resetFallbacks);
			document.removeEventListener("visibilitychange", handleVisibilityChange);
		};
	}, [failedCandidateKeys]);

	const measureCandidate = useCallback((candidateText: string) => {
		const label = labelRef.current;
		if (!label || label.clientWidth <= 0) return null;

		const probe = label.cloneNode(false) as HTMLElement;
		probe.textContent = candidateText;
		Object.assign(probe.style, {
			position: "absolute",
			left: "-100000px",
			top: "0",
			visibility: "hidden",
			pointerEvents: "none",
			display: "block",
			width: `${label.clientWidth}px`,
			height: "auto",
			maxHeight: "none",
			overflow: "visible",
			whiteSpace: "normal",
			overflowWrap: "anywhere",
			wordBreak: "normal",
			textOverflow: "clip",
		});
		document.body.append(probe);
		const fits =
			probe.getBoundingClientRect().height <=
			lineHeightInPixels(label) * MAX_REPO_IDENTITY_LINES + 1;
		probe.remove();
		return fits;
	}, []);

	useLayoutEffect(() => {
		const labelRow = labelRowRef.current;
		if (!labelRow || !labelRef.current) return;

		let frameId: number | null = null;
		const updateDisplay = () => {
			frameId = null;
			const fullFits = measureCandidate(repoFullName);
			if (fullFits === null) return;

			if (fullFits) {
				setRepoIdentityDisplay((current) =>
					current.mode === "full" && current.text === repoFullName
						? current
						: { mode: "full", text: repoFullName },
				);
				return;
			}

			const compactText =
				compactCandidates.find(
					(candidate) => measureCandidate(candidate) === true,
				) ??
				compactCandidates.at(-1) ??
				repoFullName;
			setRepoIdentityDisplay((current) =>
				current.mode === "compact" && current.text === compactText
					? current
					: { mode: "compact", text: compactText },
			);
		};
		const scheduleUpdate = () => {
			if (frameId !== null) return;
			frameId = window.requestAnimationFrame(updateDisplay);
		};

		scheduleUpdate();
		const resizeObserver =
			typeof ResizeObserver === "undefined"
				? null
				: new ResizeObserver(scheduleUpdate);
		resizeObserver?.observe(labelRow);
		window.addEventListener("resize", scheduleUpdate);
		return () => {
			resizeObserver?.disconnect();
			window.removeEventListener("resize", scheduleUpdate);
			if (frameId !== null) window.cancelAnimationFrame(frameId);
		};
	}, [compactCandidates, measureCandidate, repoFullName]);

	const candidate =
		candidates.find(
			(entry) => !failedCandidateKeys.has(`${entry.kind}:${entry.src}`),
		) ?? null;
	const kind = candidate?.kind ?? "fallback";
	const repoNameParts = repoFullName.split("/").filter((part) => part.trim());
	const fallbackSource =
		repoNameParts.length > 0
			? repoNameParts[repoNameParts.length - 1].trim()
			: repoFullName.trim();
	const fallbackLabel = fallbackSource.slice(0, 1).toUpperCase() || "?";
	const Label = labelAs;

	return (
		<div
			className={cn("flex min-w-0 max-w-full items-center gap-2.5", className)}
			data-repo-visual-kind={kind}
		>
			<span
				className={cn(
					"relative inline-flex shrink-0 items-center justify-center overflow-hidden rounded-full border border-border/60 bg-muted/35 font-mono text-[0.72em] font-semibold uppercase text-muted-foreground shadow-sm",
					visualClassName ?? "size-5",
				)}
				data-repo-visual-slot={kind}
			>
				{candidate ? (
					<img
						src={candidate.src}
						alt=""
						loading="lazy"
						decoding="async"
						referrerPolicy="no-referrer"
						className="size-full object-cover"
						data-repo-visual-image={candidate.kind}
						onError={() => {
							const failedKey = `${candidate.kind}:${candidate.src}`;
							setFailedCandidateKeys((current) => {
								if (current.has(failedKey)) return current;
								const next = new Set(current);
								next.add(failedKey);
								return next;
							});
						}}
					/>
				) : (
					<span aria-hidden="true">{fallbackLabel}</span>
				)}
			</span>
			<div className="flex min-w-0 max-w-full flex-1 flex-col justify-center">
				<div
					ref={labelRowRef}
					className="flex min-w-0 max-w-full flex-1 items-start gap-1.5"
					data-repo-identity-label-row="true"
				>
					<Label
						ref={(element) => {
							labelRef.current = element;
						}}
						className={cn(
							"block min-w-0 max-w-full flex-1 whitespace-normal [overflow-wrap:anywhere]",
							labelClassName,
						)}
						title={repoFullName}
						data-repo-identity-label="true"
						data-repo-identity-label-mode={repoIdentityDisplay.mode}
					>
						{repoIdentityDisplay.mode === "compact" ? (
							<>
								<span aria-hidden="true">{repoIdentityDisplay.text}</span>
								<span className="sr-only">{repoFullName}</span>
							</>
						) : (
							repoFullName
						)}
					</Label>
					{labelSuffix}
				</div>
				{children}
			</div>
		</div>
	);
}
