import { useMemo, useState } from "react";

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
	const [candidateIndex, setCandidateIndex] = useState(0);

	const candidate = candidates[candidateIndex] ?? null;
	const kind = candidate?.kind ?? "text_only";

	return (
		<div
			className={cn("inline-flex min-w-0 items-center gap-2", className)}
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
						className="size-full object-cover"
						data-repo-visual-image={candidate.kind}
						onError={() =>
							setCandidateIndex((current) =>
								current < candidates.length - 1
									? current + 1
									: candidates.length,
							)
						}
					/>
				</span>
			) : null}
			<span className={cn("truncate", labelClassName)}>{repoFullName}</span>
		</div>
	);
}
