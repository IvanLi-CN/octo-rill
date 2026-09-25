import type { RepoVisual } from "@/lib/repoVisual";
import {
	PublicReleasePageFrame,
	PublicReleaseTitleBand,
} from "@/pages/PublicReleasePageFrame";
import { PublicReleaseLoadingSkeleton } from "@/pages/PublicReleaseLoadingSkeleton";

export function PublicReleaseLoadingPage(props: {
	owner: string;
	repo: string;
	repoVisual?: RepoVisual | null;
}) {
	return (
		<PublicReleasePageFrame owner={props.owner} repo={props.repo}>
			<PublicReleaseTitleBand
				owner={props.owner}
				repo={props.repo}
				repoVisual={props.repoVisual ?? null}
			/>
			<PublicReleaseLoadingSkeleton />
		</PublicReleasePageFrame>
	);
}
