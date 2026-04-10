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
}) {
	const {
		repoFullName,
		repoVisual = null,
		className,
		labelClassName,
		visualClassName,
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
		/>
	);
}

function RepoIdentityContent(props: {
	repoFullName: string;
	repoVisual: RepoVisual | null;
	className?: string;
	labelClassName?: string;
	visualClassName?: string;
}) {
	const {
		repoFullName,
		repoVisual,
		className,
		labelClassName,
		visualClassName,
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
	const kind = candidate?.kind ?? "text_only";

	return (
		<div
			className={cn("inline-flex min-w-0 items-center gap-2.5", className)}
			data-repo-visual-kind={kind}
		>
			{candidate ? (
				<span
					className={cn(
						"relative shrink-0 overflow-hidden border border-border/60 bg-muted/35 shadow-sm",
						candidate.kind === "owner_avatar" ? "rounded-full" : "rounded-md",
						visualClassName ?? "size-5",
					)}
					data-repo-visual-slot={candidate.kind}
				>
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
				</span>
			) : null}
			<span className="flex min-w-0 self-stretch items-center">
				<span className={cn("block truncate", labelClassName)}>
					{repoFullName}
				</span>
			</span>
		</div>
	);
}
