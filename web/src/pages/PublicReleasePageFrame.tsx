import { ExternalLink } from "lucide-react";
import type { ReactNode } from "react";

import { AuthProviderIcon } from "@/components/brand/AuthProviderIcon";
import { BrandLogo } from "@/components/brand/BrandLogo";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Button } from "@/components/ui/button";
import { InternalLink } from "@/lib/internalNavigation";
import { resolveDemoNativeHref } from "@/demo/registry";
import type { RepoVisual } from "@/lib/repoVisual";
import { buildVersionReleaseHref } from "@/version/versionReleaseLink";
import { useVersionMonitor } from "@/version/versionMonitor";

export function PublicReleasePageFrame(props: {
	owner: string;
	repo: string;
	children: ReactNode;
}) {
	const { owner, repo, children } = props;
	const year = new Date().getFullYear();
	const { loadedVersion } = useVersionMonitor();
	const versionReleaseHref = buildVersionReleaseHref(loadedVersion);
	const repositoryHref = `https://github.com/${owner}/${repo}`;

	return (
		<main className="min-h-dvh bg-background text-foreground">
			<div className="mx-auto flex min-h-dvh w-full max-w-5xl flex-col px-4 py-5 sm:px-6 lg:px-8">
				<div className="flex min-h-full flex-col">
					<header className="flex flex-wrap items-center justify-between gap-3 border-b pb-4">
						<InternalLink
							href="/"
							to="/"
							className="inline-flex items-center gap-3"
						>
							<BrandLogo variant="wordmark" className="h-7 sm:h-8" />
						</InternalLink>
						<Button asChild variant="outline" size="sm">
							<a
								href={`https://github.com/${owner}/${repo}/releases`}
								target="_blank"
								rel="noreferrer"
							>
								<ExternalLink className="size-4" />
								GitHub
							</a>
						</Button>
					</header>

					{children}

					<footer className="mt-auto flex flex-wrap items-center justify-between gap-3 border-t pt-4 pb-1 font-mono text-[11px] text-muted-foreground">
						<span>© {year} Ivan Li</span>
						<div className="flex flex-wrap items-center justify-end gap-x-4 gap-y-1">
							<a
								href={repositoryHref}
								target="_blank"
								rel="noreferrer"
								className="inline-flex items-center gap-1.5 underline-offset-4 hover:text-foreground hover:underline"
							>
								<AuthProviderIcon provider="github" className="size-3" />
								GitHub
							</a>
							{versionReleaseHref ? (
								<a
									href={resolveDemoNativeHref(versionReleaseHref)}
									className="underline-offset-4 hover:text-foreground hover:underline"
								>
									Version {loadedVersion}
								</a>
							) : (
								<span>Version {loadedVersion}</span>
							)}
						</div>
					</footer>
				</div>
			</div>
		</main>
	);
}

export function PublicReleaseTitleBand(props: {
	owner: string;
	repo: string;
	repoVisual: RepoVisual | null;
	children?: ReactNode;
}) {
	return (
		<section className="py-6" data-testid="public-release-title-band">
			<div className="flex flex-wrap items-center gap-x-6 gap-y-3">
				<RepoIdentity
					repoFullName={`${props.owner}/${props.repo}`}
					repoVisual={props.repoVisual}
					labelAs="h1"
					className="min-w-0 max-w-full flex-[1_1_100%] sm:flex-1"
					labelClassName="break-words text-3xl font-semibold tracking-normal"
					visualClassName="size-10"
				/>
				{props.children}
			</div>
		</section>
	);
}
