import {
	Copy,
	GripHorizontal,
	Inspect,
	Minimize2,
	RefreshCcw,
} from "lucide-react";
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { useRouterState } from "@tanstack/react-router";

import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Label } from "@/components/ui/label";
import {
	Select,
	SelectContent,
	SelectItem,
	SelectTrigger,
	SelectValue,
} from "@/components/ui/select";
import {
	Sheet,
	SheetContent,
	SheetDescription,
	SheetHeader,
	SheetTitle,
} from "@/components/ui/sheet";
import { Switch } from "@/components/ui/switch";
import { useAppShellChrome } from "@/layout/AppShell";
import { useMediaQuery } from "@/lib/useMediaQuery";
import { cn } from "@/lib/utils";
import { router } from "@/router";
import { DEMO_SCENES } from "@/demo/registry";
import {
	buildCurrentDemoHref,
	patchDemoShareState,
	resetDemoScene,
	resolveCurrentDemoScene,
	syncDemoRuntimeWithHref,
	updateDemoPanelLayout,
	useDemoSnapshot,
} from "@/demo/runtime";

function clamp(value: number, min: number, max: number) {
	return Math.min(max, Math.max(min, value));
}

const DESKTOP_PANEL_WIDTH = 380;
const DESKTOP_PANEL_GAP = 16;
const DESKTOP_PANEL_MIN_HEIGHT = 360;
const DESKTOP_PANEL_TARGET_HEIGHT = 640;
const DESKTOP_PANEL_COLLAPSED_HEIGHT = 52;
const DESKTOP_PANEL_TOAST_GAP = 12;

function getOpenToastBottomInViewport(bounds: { left: number; right: number }) {
	if (typeof document === "undefined") return 0;
	const viewport = document.querySelector<HTMLElement>(
		'[data-slot="toast-viewport"]',
	);
	if (!viewport) return 0;
	const viewportRect = viewport.getBoundingClientRect();
	const overlapsHorizontally =
		bounds.right > viewportRect.left - DESKTOP_PANEL_TOAST_GAP &&
		bounds.left < viewportRect.right + DESKTOP_PANEL_TOAST_GAP;
	if (!overlapsHorizontally) return 0;

	const openToasts = Array.from(
		viewport.querySelectorAll<HTMLElement>(
			'[data-slot="toast"][data-state="open"]',
		),
	).filter((toast) => {
		const rect = toast.getBoundingClientRect();
		return rect.width > 0 && rect.height > 0;
	});
	if (openToasts.length === 0) return 0;
	return Math.max(
		...openToasts.map((toast) => toast.getBoundingClientRect().bottom),
	);
}

function resolveDesktopPanelMetrics(input: {
	collapsed: boolean;
	edge: "left" | "right";
	x: number;
	y: number;
	headerHeight: number;
	viewportTopInset: number;
	viewportBottomInset: number;
	frameWidth: number;
}) {
	if (typeof window === "undefined") {
		return {
			x: input.x,
			y: input.y,
			maxHeight: undefined as number | undefined,
		};
	}

	const nextX = clamp(
		input.x,
		DESKTOP_PANEL_GAP,
		Math.max(
			DESKTOP_PANEL_GAP,
			window.innerWidth - input.frameWidth - DESKTOP_PANEL_GAP,
		),
	);
	const frameLeft =
		input.edge === "left"
			? nextX
			: window.innerWidth - input.frameWidth - nextX;
	const frameRight = frameLeft + input.frameWidth;
	const chromeTop =
		input.viewportTopInset + input.headerHeight + DESKTOP_PANEL_GAP;
	const toastBottom = getOpenToastBottomInViewport({
		left: frameLeft,
		right: frameRight,
	});
	const minY = Math.max(
		DESKTOP_PANEL_GAP,
		chromeTop,
		toastBottom > 0 ? Math.ceil(toastBottom + DESKTOP_PANEL_TOAST_GAP) : 0,
	);
	const minVisibleHeight = input.collapsed
		? DESKTOP_PANEL_COLLAPSED_HEIGHT
		: DESKTOP_PANEL_MIN_HEIGHT;
	const maxY = Math.max(
		minY,
		window.innerHeight -
			input.viewportBottomInset -
			DESKTOP_PANEL_GAP -
			minVisibleHeight,
	);
	const nextY = clamp(input.y, minY, maxY);
	const availableHeight = Math.max(
		0,
		window.innerHeight - input.viewportBottomInset - DESKTOP_PANEL_GAP - nextY,
	);

	return {
		x: nextX,
		y: nextY,
		maxHeight: availableHeight,
	};
}

export function DemoInspector() {
	const snapshot = useDemoSnapshot();
	const scene = resolveCurrentDemoScene();
	const isMobile = useMediaQuery("(max-width: 767px)");
	const [mobileOpen, setMobileOpen] = useState(false);

	const routeLocationKey = useRouterState({
		select: (state) =>
			JSON.stringify({
				pathname: state.location.pathname,
				search: state.location.search,
				hash: state.location.hash,
			}),
	});

	useEffect(() => {
		if (!snapshot.active) return;
		syncDemoRuntimeWithHref(
			`${window.location.pathname}${window.location.search}${window.location.hash}`,
		);
	}, [routeLocationKey, snapshot.active]);

	const shareHref = useMemo(
		() => buildCurrentDemoHref(),
		[snapshot.shareState],
	);

	if (!snapshot.active) {
		return null;
	}

	const copyShareLink = async () => {
		const absoluteHref = new URL(shareHref, window.location.origin).toString();
		try {
			await navigator.clipboard.writeText(absoluteHref);
		} catch {
			// ignore clipboard failures in demo-only tooling
		}
	};

	const navigateWithShareState = (
		next: Partial<typeof snapshot.shareState>,
		options?: {
			reseed?: boolean;
		},
	) => {
		const href = buildCurrentDemoHref(next);
		patchDemoShareState(next, { reseed: options?.reseed });
		void router.navigate({
			href,
			replace: true,
		});
	};

	const setCollapsed = (collapsed: boolean) => {
		updateDemoPanelLayout({ collapsed });
	};

	const panel = (
		<DemoInspectorPanel
			snapshot={snapshot}
			sceneTitle={scene.title}
			shareHref={shareHref}
			onSceneChange={(sceneId) =>
				navigateWithShareState({ sceneId }, { reseed: true })
			}
			onPersonaChange={(personaId) =>
				navigateWithShareState({ personaId }, { reseed: true })
			}
			onNetworkChange={(networkMode) => navigateWithShareState({ networkMode })}
			onIncludeOwnReleasesChange={(includeOwnReleases) =>
				navigateWithShareState({ includeOwnReleases }, { reseed: true })
			}
			onPublicationStateChange={(publicationState) =>
				navigateWithShareState({ publicationState }, { reseed: true })
			}
			onReset={resetDemoScene}
			onCopyShareLink={copyShareLink}
		/>
	);

	if (isMobile) {
		return (
			<>
				<Button
					type="button"
					size="sm"
					className="fixed bottom-4 right-4 z-50 rounded-full shadow-lg"
					onClick={() => setMobileOpen(true)}
				>
					<Inspect className="size-4" />
					Demo
				</Button>
				<Sheet open={mobileOpen} onOpenChange={setMobileOpen}>
					<SheetContent side="bottom" className="h-[82vh] rounded-t-3xl p-0">
						<SheetHeader className="border-b pb-3">
							<SheetTitle>Demo Inspector</SheetTitle>
							<SheetDescription>
								切换 scene / persona / network，并复制当前 share deep link。
							</SheetDescription>
						</SheetHeader>
						<div className="h-full overflow-y-auto p-4">{panel}</div>
					</SheetContent>
				</Sheet>
			</>
		);
	}

	return (
		<DesktopInspectorChrome
			collapsed={snapshot.panelLayout.collapsed}
			edge={snapshot.panelLayout.edge}
			x={snapshot.panelLayout.x}
			y={snapshot.panelLayout.y}
			onCollapse={() => setCollapsed(true)}
			onExpand={() => setCollapsed(false)}
		>
			{panel}
		</DesktopInspectorChrome>
	);
}

function DesktopInspectorChrome(props: {
	children: ReactNode;
	collapsed: boolean;
	edge: "left" | "right";
	x: number;
	y: number;
	onCollapse: () => void;
	onExpand: () => void;
}) {
	const { children, collapsed, edge, x, y, onCollapse, onExpand } = props;
	const { headerHeight, viewportBottomInset, viewportTopInset } =
		useAppShellChrome();
	const panelRef = useRef<HTMLDivElement | null>(null);
	const dragOffsetRef = useRef<{ x: number; y: number } | null>(null);
	const [dragging, setDragging] = useState(false);
	const [viewportRevision, setViewportRevision] = useState(0);

	useEffect(() => {
		if (typeof window === "undefined") return;
		const syncViewport = () => {
			setViewportRevision((current) => current + 1);
		};
		const visualViewport = window.visualViewport;
		window.addEventListener("resize", syncViewport);
		visualViewport?.addEventListener("resize", syncViewport);
		visualViewport?.addEventListener("scroll", syncViewport);
		return () => {
			window.removeEventListener("resize", syncViewport);
			visualViewport?.removeEventListener("resize", syncViewport);
			visualViewport?.removeEventListener("scroll", syncViewport);
		};
	}, []);

	useEffect(() => {
		if (typeof document === "undefined") return;
		const observer = new MutationObserver(() => {
			setViewportRevision((current) => current + 1);
		});
		observer.observe(document.body, {
			subtree: true,
			childList: true,
			attributes: true,
			attributeFilter: ["data-state"],
		});
		return () => observer.disconnect();
	}, []);

	const frameWidth =
		panelRef.current?.offsetWidth ?? (collapsed ? 176 : DESKTOP_PANEL_WIDTH);
	const panelMetrics = resolveDesktopPanelMetrics({
		collapsed,
		edge,
		x,
		y,
		headerHeight,
		viewportTopInset,
		viewportBottomInset,
		frameWidth,
	});
	const panelHeight = collapsed
		? undefined
		: Math.min(
				panelMetrics.maxHeight ?? DESKTOP_PANEL_TARGET_HEIGHT,
				DESKTOP_PANEL_TARGET_HEIGHT,
			);

	useEffect(() => {
		if (!dragging) return;
		const initialOffset = dragOffsetRef.current;
		if (!initialOffset) return;

		let currentY = panelMetrics.y;

		const onMove = (event: PointerEvent) => {
			const panelWidth = panelRef.current?.offsetWidth ?? DESKTOP_PANEL_WIDTH;
			const currentX = clamp(
				event.clientX - initialOffset.x,
				DESKTOP_PANEL_GAP,
				Math.max(
					DESKTOP_PANEL_GAP,
					window.innerWidth - panelWidth - DESKTOP_PANEL_GAP,
				),
			);
			const nextMetrics = resolveDesktopPanelMetrics({
				collapsed: false,
				edge: "left",
				x: currentX,
				y: event.clientY - initialOffset.y,
				headerHeight,
				viewportTopInset,
				viewportBottomInset,
				frameWidth: panelWidth,
			});
			currentY = nextMetrics.y;
			updateDemoPanelLayout(
				{ edge: "left", x: currentX, y: currentY },
				{ emit: true },
			);
		};

		const onUp = (event: PointerEvent) => {
			const snapLeft = event.clientX < window.innerWidth / 2;
			const nextMetrics = resolveDesktopPanelMetrics({
				collapsed: false,
				edge: snapLeft ? "left" : "right",
				x: DESKTOP_PANEL_GAP,
				y: currentY,
				headerHeight,
				viewportTopInset,
				viewportBottomInset,
				frameWidth: panelRef.current?.offsetWidth ?? DESKTOP_PANEL_WIDTH,
			});
			updateDemoPanelLayout({
				edge: snapLeft ? "left" : "right",
				x: DESKTOP_PANEL_GAP,
				y: nextMetrics.y,
			});
			dragOffsetRef.current = null;
			setDragging(false);
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};

		window.addEventListener("pointermove", onMove);
		window.addEventListener("pointerup", onUp);
		return () => {
			window.removeEventListener("pointermove", onMove);
			window.removeEventListener("pointerup", onUp);
		};
	}, [
		dragging,
		headerHeight,
		panelMetrics.y,
		viewportBottomInset,
		viewportRevision,
		viewportTopInset,
	]);

	const positionStyle =
		edge === "left"
			? { left: panelMetrics.x, top: panelMetrics.y }
			: { right: panelMetrics.x, top: panelMetrics.y };

	if (collapsed) {
		return (
			<Button
				type="button"
				size="sm"
				className="fixed z-50 max-w-[calc(100vw-24px)] rounded-full px-4 py-5 shadow-lg"
				data-demo-inspector-bubble="desktop"
				style={positionStyle}
				onClick={onExpand}
			>
				<Inspect className="size-4" />
				Demo Inspector
			</Button>
		);
	}

	return (
		<div
			ref={panelRef}
			className="fixed z-50 flex w-[380px] max-w-[calc(100vw-24px)] overflow-hidden"
			data-demo-inspector-chrome="desktop"
			style={{
				...positionStyle,
				height: panelHeight,
				maxHeight: panelMetrics.maxHeight,
			}}
		>
			<div
				className="flex h-full min-h-0 w-full flex-1 flex-col overflow-hidden rounded-3xl border bg-background/95 shadow-2xl backdrop-blur"
				data-demo-inspector-surface="true"
			>
				<div
					className="flex cursor-move items-center justify-between border-b px-4 py-3"
					data-demo-inspector-title="true"
					onPointerDown={(event) => {
						if (
							event.target instanceof HTMLElement &&
							event.target.closest("button")
						) {
							return;
						}
						const rect = panelRef.current?.getBoundingClientRect();
						if (!rect) return;
						dragOffsetRef.current = {
							x: event.clientX - rect.left,
							y: event.clientY - rect.top,
						};
						setDragging(true);
					}}
				>
					<div className="flex items-center gap-2">
						<GripHorizontal className="size-4 text-muted-foreground" />
						<p className="font-medium text-sm">Demo Inspector</p>
					</div>
					<Button
						type="button"
						size="icon"
						variant="ghost"
						onPointerDown={(event) => event.stopPropagation()}
						onClick={onCollapse}
					>
						<Minimize2 className="size-4" />
					</Button>
				</div>
				<div
					className="min-h-0 flex-1 overflow-y-auto p-4"
					data-demo-inspector-scroller="true"
				>
					{children}
				</div>
			</div>
		</div>
	);
}

export type DemoInspectorPanelProps = {
	snapshot: ReturnType<typeof useDemoSnapshot>;
	sceneTitle: string;
	shareHref: string;
	onSceneChange: (sceneId: (typeof DEMO_SCENES)[number]["id"]) => void;
	onPersonaChange: (personaId: "guest" | "member" | "admin") => void;
	onNetworkChange: (networkMode: "normal" | "slow" | "faulty") => void;
	onIncludeOwnReleasesChange: (checked: boolean) => void;
	onPublicationStateChange: (state: "published" | "unpublished") => void;
	onReset: () => void;
	onCopyShareLink: () => void;
};

export function DemoInspectorPanel(props: DemoInspectorPanelProps) {
	const { snapshot } = props;
	return (
		<div className="space-y-2">
			<Card className="border-dashed">
				<CardHeader className="p-4 pb-2">
					<CardTitle className="flex items-center justify-between text-base">
						<span>{props.sceneTitle}</span>
						<Badge variant="outline">Simulated Writes</Badge>
					</CardTitle>
				</CardHeader>
				<CardContent className="space-y-3 px-4 pb-4">
					<section className="space-y-2">
						<Label>Scene</Label>
						<Select
							value={snapshot.shareState.sceneId}
							onValueChange={(value) =>
								props.onSceneChange(value as (typeof DEMO_SCENES)[number]["id"])
							}
						>
							<SelectTrigger className="w-full">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								{DEMO_SCENES.map((scene) => (
									<SelectItem key={scene.id} value={scene.id}>
										{scene.title}
									</SelectItem>
								))}
							</SelectContent>
						</Select>
					</section>

					<section className="grid gap-3 sm:grid-cols-2">
						<div className="space-y-2">
							<Label>Persona</Label>
							<Select
								value={snapshot.shareState.personaId}
								onValueChange={(value) =>
									props.onPersonaChange(value as "guest" | "member" | "admin")
								}
							>
								<SelectTrigger className="w-full">
									<SelectValue />
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="guest">Guest</SelectItem>
									<SelectItem value="member">Member</SelectItem>
									<SelectItem value="admin">Admin</SelectItem>
								</SelectContent>
							</Select>
						</div>
						<div className="space-y-2">
							<Label>Network</Label>
							<Select
								value={snapshot.shareState.networkMode}
								onValueChange={(value) =>
									props.onNetworkChange(value as "normal" | "slow" | "faulty")
								}
							>
								<SelectTrigger className="w-full">
									<SelectValue />
								</SelectTrigger>
								<SelectContent>
									<SelectItem value="normal">Normal</SelectItem>
									<SelectItem value="slow">Slow</SelectItem>
									<SelectItem value="faulty">Faulty</SelectItem>
								</SelectContent>
							</Select>
						</div>
					</section>
				</CardContent>
			</Card>

			<Card>
				<CardHeader className="p-4 pb-2">
					<CardTitle className="text-base">Data</CardTitle>
				</CardHeader>
				<CardContent className="space-y-3 px-4 pb-4">
					<div className="flex items-center justify-between gap-3">
						<div className="space-y-0.5">
							<p className="font-medium text-sm">Include My Releases</p>
							<p className="text-muted-foreground text-xs">
								影响 Settings 回显和 Dashboard owner-only release 露出。
							</p>
						</div>
						<Switch
							checked={snapshot.shareState.includeOwnReleases}
							onCheckedChange={props.onIncludeOwnReleasesChange}
						/>
					</div>
					<div className="space-y-2">
						<Label>Repo Public Release State</Label>
						<Select
							value={snapshot.shareState.publicationState}
							onValueChange={(value) =>
								props.onPublicationStateChange(
									value as "published" | "unpublished",
								)
							}
						>
							<SelectTrigger className="w-full">
								<SelectValue />
							</SelectTrigger>
							<SelectContent>
								<SelectItem value="unpublished">Unpublished</SelectItem>
								<SelectItem value="published">Published</SelectItem>
							</SelectContent>
						</Select>
					</div>
				</CardContent>
			</Card>

			<Card>
				<CardHeader className="p-4 pb-2">
					<CardTitle className="text-base">Actions & Share</CardTitle>
				</CardHeader>
				<CardContent className="space-y-3 px-4 pb-4">
					<div className="flex flex-wrap gap-2">
						<Button type="button" variant="outline" onClick={props.onReset}>
							<RefreshCcw className="size-4" />
							Reset Scene
						</Button>
						<Button
							type="button"
							variant="outline"
							onClick={props.onCopyShareLink}
						>
							<Copy className="size-4" />
							Copy Share URL
						</Button>
					</div>
					<div className="space-y-2">
						<Label>Share</Label>
						<p className="rounded-xl border bg-muted/30 px-3 py-2 font-mono text-[11px] break-all">
							{props.shareHref}
						</p>
					</div>
					<p className="text-muted-foreground text-xs">
						{snapshot.mutations.length === 0
							? "No simulated writes recorded yet."
							: `${snapshot.mutations.length} simulated write${snapshot.mutations.length > 1 ? "s" : ""} recorded.`}
					</p>
				</CardContent>
			</Card>

			<details className="rounded-2xl border bg-muted/20 p-3">
				<summary className="cursor-pointer list-none">
					<div className="flex items-center justify-between gap-3">
						<p className="font-medium text-sm">Advanced</p>
						<div className="flex items-center gap-2">
							<Badge variant="outline">
								{snapshot.mutations.length === 0
									? "No writes yet"
									: `${snapshot.mutations.length} recorded`}
							</Badge>
							<Badge variant="outline">Raw JSON</Badge>
						</div>
					</div>
				</summary>
				<div className="mt-4 space-y-4">
					<section className="space-y-2">
						<p className="font-medium text-sm">Recent Mutations</p>
						{snapshot.mutations.length === 0 ? (
							<p className="text-muted-foreground text-sm">
								还没有 simulated
								write；直接在页面里保存、发布、取消或重试一次就会记录。
							</p>
						) : (
							<div className="space-y-2">
								{snapshot.mutations.map((mutation) => (
									<div
										key={mutation.id}
										className="rounded-xl border px-3 py-2"
									>
										<p className="font-medium text-sm">{mutation.label}</p>
										<p className="text-muted-foreground text-xs">
											{mutation.detail}
										</p>
									</div>
								))}
							</div>
						)}
					</section>
					<section className="space-y-2">
						<p className="font-medium text-sm">Raw JSON</p>
						<pre
							className={cn(
								"overflow-x-auto text-[11px] leading-5",
								"whitespace-pre-wrap break-all",
							)}
						>
							{JSON.stringify(snapshot.model, null, 2)}
						</pre>
					</section>
				</div>
			</details>
		</div>
	);
}
