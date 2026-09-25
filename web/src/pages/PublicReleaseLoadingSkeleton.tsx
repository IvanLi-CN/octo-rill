import { cn } from "@/lib/utils";

const directoryRows = Array.from({ length: 14 }, (_, index) => index);
const detailCards = [
	{ title: "w-2/3", lines: ["w-full", "w-[92%]", "w-[78%]"], body: "h-24" },
	{ title: "w-3/5", lines: ["w-full", "w-[84%]"], body: "h-16" },
	{
		title: "w-3/4",
		lines: ["w-full", "w-[96%]", "w-[88%]", "w-[72%]"],
		body: "h-36",
	},
];

function SkeletonBlock(props: { className: string }) {
	return (
		<div
			className={cn(
				"animate-pulse rounded-md bg-muted/70 motion-reduce:animate-none",
				props.className,
			)}
			data-testid="public-release-skeleton-block"
		/>
	);
}

export function PublicReleaseLoadingSkeleton() {
	return (
		<section
			aria-label="Release loading skeleton"
			data-testid="public-release-loading-skeleton"
			className="grid h-[min(72dvh,880px)] min-h-[20rem] grid-cols-1 overflow-hidden rounded-2xl border border-border/70 bg-card/30 lg:h-[calc(100dvh-11rem)] lg:min-h-[32rem] lg:grid-cols-[minmax(15rem,18rem)_minmax(0,1fr)]"
		>
			<div
				className="hidden min-h-0 flex-col border-r border-border/70 bg-muted/15 lg:flex"
				aria-hidden="true"
				data-testid="public-release-loading-directory"
			>
				<div className="border-b border-border/60 px-4 py-3">
					<SkeletonBlock className="h-3 w-16" />
				</div>
				<div className="min-h-0 flex-1 overflow-hidden">
					{directoryRows.map((row) => (
						<div
							key={row}
							className="flex h-[62px] flex-col justify-center gap-2 border-l-2 border-transparent px-4 py-3"
						>
							<SkeletonBlock className="h-3.5 w-16" />
							<SkeletonBlock className="h-3 w-28" />
						</div>
					))}
				</div>
			</div>

			<div
				className="min-h-0 overflow-hidden px-3 py-4 sm:px-5 sm:py-5 lg:h-full"
				aria-hidden="true"
				data-testid="public-release-loading-details"
			>
				<div className="space-y-3 sm:space-y-4">
					{detailCards.map((card, index) => (
						<div
							key={index}
							className="space-y-4 rounded-2xl border border-border/70 bg-card/82 p-5 shadow-sm sm:p-6"
						>
							<div className="flex items-start gap-3">
								<SkeletonBlock className="size-10 shrink-0 rounded-full" />
								<div className="min-w-0 flex-1 space-y-2">
									<SkeletonBlock className={cn("h-4", card.title)} />
									<SkeletonBlock className="h-3 w-28" />
								</div>
							</div>
							<div className="space-y-2.5">
								{card.lines.map((width) => (
									<SkeletonBlock key={width} className={cn("h-3.5", width)} />
								))}
							</div>
							<SkeletonBlock className={cn("w-full rounded-xl", card.body)} />
						</div>
					))}
				</div>
			</div>
		</section>
	);
}
