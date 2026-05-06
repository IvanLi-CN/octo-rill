import type { ReactNode } from "react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { RepoVisual } from "@/lib/repoVisual";
import { resolveRepoVisualCandidates } from "@/lib/repoVisual";
import { cn } from "@/lib/utils";

export function RepoIdentity(props: {
	repoFullName: string | null;
	repoVisual?: RepoVisual | null;
	className?: string;
	labelClassName?: string;
	visualClassName?: string;
	children?: ReactNode;
}) {
	const {
		repoFullName,
		repoVisual = null,
		className,
		labelClassName,
		visualClassName,
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
			labelClassName={labelClassName}
			visualClassName={visualClassName}
		>
			{children}
		</RepoIdentityContent>
	);
}

function RepoIdentityContent(props: {
	repoFullName: string;
	repoVisual: RepoVisual | null;
	className?: string;
	labelClassName?: string;
	visualClassName?: string;
	children?: ReactNode;
}) {
	const {
		repoFullName,
		repoVisual,
		className,
		labelClassName,
		visualClassName,
		children,
	} = props;
	const candidates = useMemo(
		() => resolveRepoVisualCandidates(repoVisual),
		[repoVisual],
	);
	const [failedCandidateKeys, setFailedCandidateKeys] = useState<Set<string>>(
		() => new Set(),
	);
	const autoRetryPendingRef = useRef(true);

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

	return (
		<div
			className={cn("inline-flex min-w-0 items-center gap-2.5", className)}
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
			<span className="flex min-w-0 flex-col justify-center">
				<span className={cn("block truncate", labelClassName)}>
					{repoFullName}
				</span>
				{children}
			</span>
		</div>
	);
}
