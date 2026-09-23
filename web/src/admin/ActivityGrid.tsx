import {
	createElement,
	type CSSProperties,
	type FocusEvent as ReactFocusEvent,
	type KeyboardEvent as ReactKeyboardEvent,
	type MouseEvent as ReactMouseEvent,
	type PointerEvent as ReactPointerEvent,
	type ReactElement,
	type ReactNode,
	type RefObject,
	useCallback,
	useEffect,
	useLayoutEffect,
	useMemo,
	useRef,
	useState,
} from "react";
import { createPortal } from "react-dom";

import { useMediaQuery } from "@/lib/useMediaQuery";

export type ActivityGridLayout = "wrapped-rows" | "matrix";
export type ActivityGridScrollPolicy = "page" | "panel";

export type ActivityGridPreview = {
	title: string;
	lines: ReactNode[];
	content?: ReactNode;
};

export type ActivityGridCell = {
	id: string;
	ariaLabel: string;
	className?: string;
	color?: string;
	preview: ActivityGridPreview;
	data?: unknown;
	decorate?: (element: ReactElement) => ReactNode;
	ariaControls?: string;
	dataOutcome?: string;
};

export type ActivityGridWrappedRow = {
	id: string;
	label: string;
	title?: string;
	cells: ActivityGridCell[];
};

export type ActivityGridMatrixColumn = {
	id: string;
	label: string;
	title?: string;
};

export type ActivityGridMatrixRow = {
	id: string;
	label: string;
	mobileLabel?: string;
	cells: ActivityGridCell[];
};

export type ActivityGridModel =
	| {
			layout: "wrapped-rows";
			scrollPolicy: "page";
			ariaLabel: string;
			rows: ActivityGridWrappedRow[];
			cellSize?: number;
			cellGap?: number;
			rowLabelWidth?: number;
			minRowHeight?: number;
			testId?: string;
	  }
	| {
			layout: "matrix";
			scrollPolicy: "panel";
			ariaLabel: string;
			columns: ActivityGridMatrixColumn[];
			rows: ActivityGridMatrixRow[];
			cellSize?: number;
			cellGap?: number;
			rowLabelWidth?: number;
			mobileRowLabelWidth?: number;
			testId?: string;
	  };

type ActivityGridProps = {
	model: ActivityGridModel;
	loading?: boolean;
	refreshing?: boolean;
	onActivate: (cell: ActivityGridCell) => void;
	onSelectionChange?: (cell: ActivityGridCell | null) => void;
	showPreview?: boolean;
	previewTestId?: string;
	className?: string;
};

type Rect = { left: number; top: number; right: number; bottom: number };
type PreviewPlacement = { left: number; top: number };

const PREVIEW_GAP = 10;
const VIEWPORT_MARGIN = 10;
const LONG_PRESS_MS = 150;
const TOUCH_MOVE_THRESHOLD = 8;
const DENSE_CELL_LIMIT = 8_000;

function clamp(value: number, min: number, max: number) {
	return Math.min(Math.max(value, min), max);
}

function getViewportRect() {
	return {
		width: typeof window === "undefined" ? 0 : window.innerWidth,
		height: typeof window === "undefined" ? 0 : window.innerHeight,
	};
}

function placePreview(
	anchor: Rect,
	preview: { width: number; height: number },
): PreviewPlacement {
	const viewport = getViewportRect();
	const preferredLeft =
		anchor.left + (anchor.right - anchor.left) / 2 - preview.width / 2;
	const aboveTop = anchor.top - preview.height - PREVIEW_GAP;
	const belowTop = anchor.bottom + PREVIEW_GAP;
	const top =
		aboveTop >= VIEWPORT_MARGIN
			? aboveTop
			: belowTop + preview.height <= viewport.height - VIEWPORT_MARGIN
				? belowTop
				: clamp(
						aboveTop,
						VIEWPORT_MARGIN,
						Math.max(
							VIEWPORT_MARGIN,
							viewport.height - preview.height - VIEWPORT_MARGIN,
						),
					);
	return {
		left: clamp(
			preferredLeft,
			VIEWPORT_MARGIN,
			Math.max(
				VIEWPORT_MARGIN,
				viewport.width - preview.width - VIEWPORT_MARGIN,
			),
		),
		top,
	};
}

function cellIdentity(cell: ActivityGridCell) {
	return cell.id;
}

function useSafePreview(
	cell: ActivityGridCell | null,
	anchor: Rect | null,
	previewRef: RefObject<HTMLDivElement | null>,
) {
	const [placement, setPlacement] = useState<PreviewPlacement | null>(null);

	const reposition = useCallback(() => {
		if (!cell || !anchor || !previewRef.current) return;
		const rect = previewRef.current.getBoundingClientRect();
		setPlacement(
			placePreview(anchor, { width: rect.width, height: rect.height }),
		);
	}, [anchor, cell, previewRef]);

	useLayoutEffect(() => {
		if (!cell || !anchor) {
			setPlacement(null);
			return;
		}
		reposition();
	}, [anchor, cell, reposition]);

	useEffect(() => {
		if (!cell) return;
		const update = () => reposition();
		window.addEventListener("resize", update);
		window.addEventListener("scroll", update, true);
		return () => {
			window.removeEventListener("resize", update);
			window.removeEventListener("scroll", update, true);
		};
	}, [cell, reposition]);

	return placement;
}

function PreviewLayer({
	cell,
	anchor,
	pinned,
	testId,
	onClose,
}: {
	cell: ActivityGridCell | null;
	anchor: Rect | null;
	pinned: boolean;
	testId?: string;
	onClose: () => void;
}) {
	const previewRef = useRef<HTMLDivElement>(null);
	const placement = useSafePreview(cell, anchor, previewRef);
	if (!cell) return null;
	return createPortal(
		<div
			ref={previewRef}
			{...(pinned
				? { role: "dialog" as const, "aria-label": cell.preview.title }
				: { role: "tooltip" as const })}
			aria-live="polite"
			data-testid={testId}
			data-activity-grid-preview="true"
			className={`${pinned ? "pointer-events-auto" : "pointer-events-none"} fixed z-50 w-[min(28rem,calc(100vw-1.25rem))] rounded-md border bg-popover p-3 text-popover-foreground shadow-lg`}
			style={
				placement
					? { left: placement.left, top: placement.top }
					: { left: 0, top: 0, visibility: "hidden" }
			}
		>
			<div className="flex items-start justify-between gap-3">
				<p
					id="activity-grid-preview-title"
					className="min-w-0 break-words text-sm font-medium"
				>
					{cell.preview.title}
				</p>
				{pinned ? (
					<button
						type="button"
						className="pointer-events-auto shrink-0 text-xs text-muted-foreground underline-offset-2 hover:underline"
						onClick={onClose}
					>
						关闭
					</button>
				) : null}
			</div>
			{cell.preview.content ?? (
				<div className="mt-1 space-y-0.5 text-xs text-muted-foreground">
					{cell.preview.lines.map((line, index) => (
						<p key={index} className="break-words">
							{line}
						</p>
					))}
				</div>
			)}
		</div>,
		document.body,
	);
}

function ActivityCellButton({
	cell,
	selected,
	setRef,
	onPointerEnter,
	onPointerLeave,
	onFocus,
	onBlur,
	onPointerDown,
	onPointerMove,
	onPointerUp,
	onClick,
	onKeyDown,
}: {
	cell: ActivityGridCell;
	selected: boolean;
	setRef: (element: HTMLButtonElement | null) => void;
	onPointerEnter: (event: ReactPointerEvent<HTMLButtonElement>) => void;
	onPointerLeave: (event: ReactPointerEvent<HTMLButtonElement>) => void;
	onFocus: (event: ReactFocusEvent<HTMLButtonElement>) => void;
	onBlur: (event: ReactFocusEvent<HTMLButtonElement>) => void;
	onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => void;
	onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => void;
	onPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) => void;
	onClick: (event: ReactMouseEvent<HTMLButtonElement>) => void;
	onKeyDown: (event: ReactKeyboardEvent<HTMLButtonElement>) => void;
}) {
	const element = createElement(
		"button",
		{
			ref: setRef,
			type: "button",
			className: [
				"relative block h-full w-full shrink-0 rounded-[2px] ring-1 transition-colors focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
				cell.className,
				selected ? "ring-foreground/90" : "",
			]
				.filter(Boolean)
				.join(" "),
			style: cell.color ? { backgroundColor: cell.color } : undefined,
			"aria-label": cell.ariaLabel,
			"aria-controls": cell.ariaControls,
			"aria-expanded": cell.ariaControls ? selected : undefined,
			"aria-pressed": selected,
			"data-activity-cell-id": cell.id,
			"data-activity-outcome": cell.dataOutcome,
			onPointerEnter,
			onPointerLeave,
			onFocus,
			onBlur,
			onPointerDown,
			onPointerMove,
			onPointerUp,
			onClick,
			onKeyDown,
		},
		cell.color ? null : undefined,
	) as ReactElement;
	return cell.decorate ? cell.decorate(element) : element;
}

function ActivityGridSkeleton({ model }: { model: ActivityGridModel }) {
	const cellSize = model.cellSize ?? 12;
	const gap = model.cellGap ?? 2;
	if (model.layout === "matrix") {
		return (
			<div
				aria-hidden="true"
				className="grid animate-pulse gap-y-2"
				style={{
					gridTemplateColumns: `${model.rowLabelWidth ?? 152}px repeat(${Math.min(model.columns.length || 12, 24)}, minmax(0, 1fr))`,
				}}
			>
				<div className="h-5 rounded bg-muted" />
				{Array.from(
					{ length: Math.min(model.columns.length || 12, 24) },
					(_, index) => (
						<div key={index} className="h-5 rounded bg-muted" />
					),
				)}
				{Array.from(
					{ length: Math.max(model.rows.length, 3) },
					(_, rowIndex) => (
						<FragmentRow
							key={rowIndex}
							count={Math.min(model.columns.length || 12, 24)}
							cellSize={cellSize}
							gap={gap}
						/>
					),
				)}
			</div>
		);
	}
	return (
		<div aria-hidden="true" className="animate-pulse space-y-2">
			{model.rows.map((row) => (
				<div
					key={row.id}
					className="grid grid-cols-[3.5rem_minmax(0,1fr)] gap-2 border-t border-border/70 py-1 first:border-t-0"
				>
					<div className="h-3 w-10 translate-y-0.5 rounded bg-muted" />
					<div className="min-w-0" style={wrappedRowGridStyle(cellSize, gap)}>
						{Array.from(
							{ length: Math.max(row.cells.length, 6) },
							(_, index) => (
								<span
									key={index}
									className="rounded-[2px] bg-muted"
									style={{ width: cellSize, height: cellSize }}
								/>
							),
						)}
					</div>
				</div>
			))}
		</div>
	);
}

function FragmentRow({
	count,
	cellSize,
	gap,
}: {
	count: number;
	cellSize: number;
	gap: number;
}) {
	return (
		<>
			<div className="h-4 rounded bg-muted" />
			<div className="flex min-w-0 flex-wrap content-start" style={{ gap }}>
				{Array.from({ length: count }, (_, index) => (
					<span
						key={index}
						className="rounded-[2px] bg-muted"
						style={{ width: cellSize, height: cellSize }}
					/>
				))}
			</div>
		</>
	);
}

function wrappedRowGridStyle(cellSize: number, gap: number): CSSProperties {
	return {
		display: "grid",
		gridTemplateColumns: `repeat(auto-fill, ${cellSize}px)`,
		gridAutoRows: `${cellSize}px`,
		gap,
		minHeight: cellSize,
		alignContent: "start",
	};
}

function useActivityCells(model: ActivityGridModel) {
	return useMemo(() => {
		if (model.layout === "wrapped-rows") {
			return model.rows.flatMap((row, rowIndex) =>
				row.cells.map((cell, cellIndex) => ({ cell, rowIndex, cellIndex })),
			);
		}
		return model.rows.flatMap((row, rowIndex) =>
			row.cells.map((cell, cellIndex) => ({ cell, rowIndex, cellIndex })),
		);
	}, [model]);
}

function WrappedRowsGrid({
	model,
	activeId,
	setCellRef,
	getHandlers,
}: {
	model: Extract<ActivityGridModel, { layout: "wrapped-rows" }>;
	activeId: string | null;
	setCellRef: (id: string, element: HTMLButtonElement | null) => void;
	getHandlers: (
		cell: ActivityGridCell,
		rowIndex: number,
		cellIndex: number,
	) => Parameters<typeof ActivityCellButton>[0];
}) {
	const cellSize = model.cellSize ?? 12;
	const gap = model.cellGap ?? 2;
	return (
		<div
			className="space-y-2"
			data-testid={model.testId ?? "activity-grid-wrapped-rows"}
			style={
				{
					"--activity-cell-size": `${cellSize}px`,
					"--activity-cell-gap": `${gap}px`,
				} as CSSProperties
			}
		>
			{model.rows.map((row, rowIndex) => (
				<div
					key={row.id}
					data-testid="activity-grid-wrapped-row"
					className="grid grid-cols-[3.5rem_minmax(0,1fr)] gap-2 border-t border-border/70 py-1 first:border-t-0"
				>
					<time
						className="translate-y-0.5 font-mono text-xs leading-3 tabular-nums text-muted-foreground"
						title={row.title}
					>
						{row.label}
					</time>
					<div className="min-w-0" style={wrappedRowGridStyle(cellSize, gap)}>
						{row.cells.map((cell, cellIndex) => {
							const handlers = getHandlers(cell, rowIndex, cellIndex);
							return (
								<div
									key={cellIdentity(cell)}
									style={{ width: cellSize, height: cellSize }}
								>
									<ActivityCellButton
										{...handlers}
										cell={cell}
										selected={activeId === cell.id}
										setRef={(element) => setCellRef(cell.id, element)}
									/>
								</div>
							);
						})}
					</div>
				</div>
			))}
		</div>
	);
}

function MatrixGrid({
	model,
	activeId,
	setCellRef,
	getHandlers,
}: {
	model: Extract<ActivityGridModel, { layout: "matrix" }>;
	activeId: string | null;
	setCellRef: (id: string, element: HTMLButtonElement | null) => void;
	getHandlers: (
		cell: ActivityGridCell,
		rowIndex: number,
		cellIndex: number,
	) => Parameters<typeof ActivityCellButton>[0];
}) {
	const [width, setWidth] = useState(0);
	const surfaceRef = useRef<HTMLDivElement>(null);
	const isDesktop = useMediaQuery("(min-width: 1024px)");
	const isTablet = useMediaQuery("(min-width: 640px)");
	const labelWidth = isDesktop
		? (model.rowLabelWidth ?? 152)
		: isTablet
			? (model.rowLabelWidth ?? 112)
			: (model.mobileRowLabelWidth ?? 28);
	const cellSize = isDesktop
		? (model.cellSize ?? 12)
		: isTablet
			? Math.min(model.cellSize ?? 12, 11)
			: Math.min(model.cellSize ?? 12, 9);
	const minimumMatrixGap = model.cellGap ?? 2;
	useLayoutEffect(() => {
		const surface = surfaceRef.current;
		if (!surface) return;
		const update = () =>
			setWidth(Math.round(surface.getBoundingClientRect().width));
		update();
		const observer = new ResizeObserver(update);
		observer.observe(surface);
		return () => observer.disconnect();
	}, []);
	const visibleCount =
		width > 0
			? Math.max(
					1,
					Math.floor(
						(width - labelWidth + minimumMatrixGap) /
							(cellSize + minimumMatrixGap),
					),
				)
			: Math.min(model.columns.length, isDesktop ? 50 : isTablet ? 36 : 25);
	const visibleStart = Math.max(0, model.columns.length - visibleCount);
	const columns = model.columns.slice(visibleStart);
	const matrixGap =
		width > 0 && columns.length > 0
			? Math.max(
					minimumMatrixGap,
					(width - labelWidth - columns.length * cellSize) / columns.length,
				)
			: minimumMatrixGap;
	const labelStride = columns.length > 30 ? 12 : 6;
	const gridTemplateColumns = `${labelWidth}px repeat(${columns.length}, ${cellSize}px)`;
	return (
		<div
			ref={surfaceRef}
			className="min-w-0 max-h-[min(30vh,12rem)] overflow-y-auto"
			data-testid={model.testId ?? "activity-grid-matrix-surface"}
		>
			<div
				className="grid items-end"
				style={{ gridTemplateColumns, columnGap: matrixGap }}
			>
				<div className="h-6" />
				{columns.map((column, index) => (
					<div
						key={column.id}
						className="relative h-6 min-w-0 text-xs text-muted-foreground"
						title={column.title ?? column.label}
						data-testid="llm-activity-time-label"
					>
						{index === columns.length - 1 ||
						((index + visibleStart) % labelStride === 0 &&
							columns.length - 1 - index >= 3) ? (
							<span
								className={`absolute bottom-0 whitespace-nowrap ${
									index === 0
										? "left-0"
										: index === columns.length - 1
											? "right-0"
											: "left-1/2 -translate-x-1/2"
								}`}
							>
								{column.label}
							</span>
						) : null}
					</div>
				))}
				{model.rows.map((row, rowIndex) => [
					<div
						key={`${row.id}:label`}
						className="sticky left-0 z-10 flex min-w-0 items-center bg-card pr-1 sm:pr-2"
					>
						<span
							className="hidden truncate font-mono text-xs sm:inline"
							title={row.label}
						>
							{row.label}
						</span>
						<span className="font-mono text-[10px] text-muted-foreground sm:hidden">
							{row.mobileLabel ?? row.label}
						</span>
						<span className="sr-only">{row.label}</span>
					</div>,
					...columns.map((column, visibleIndex) => {
						const cellIndex = visibleStart + visibleIndex;
						const cell = row.cells[cellIndex];
						if (!cell)
							return (
								<div
									key={`${row.id}:${column.id}`}
									className="aspect-square min-w-0"
								/>
							);
						const handlers = getHandlers(cell, rowIndex, cellIndex);
						return (
							<div
								key={`${row.id}:${column.id}`}
								className="min-w-0"
								style={{ paddingBottom: matrixGap }}
							>
								<div style={{ aspectRatio: "1 / 1" }}>
									<ActivityCellButton
										{...handlers}
										cell={cell}
										selected={activeId === cell.id}
										setRef={(element) => setCellRef(cell.id, element)}
									/>
								</div>
							</div>
						);
					}),
				])}
			</div>
		</div>
	);
}

function DenseCanvasGrid({
	model,
	activeId,
	setActiveId,
	onActivate,
	setAnchor,
}: {
	model: Extract<ActivityGridModel, { layout: "wrapped-rows" }>;
	activeId: string | null;
	setActiveId: (id: string | null) => void;
	onActivate: (cell: ActivityGridCell) => void;
	setAnchor: (rect: Rect | null) => void;
}) {
	const rootRef = useRef<HTMLButtonElement>(null);
	const [width, setWidth] = useState(0);
	const cellSize = model.cellSize ?? 12;
	const gap = model.cellGap ?? 2;
	const step = cellSize + gap;
	const labelWidth = model.rowLabelWidth ?? 56;
	const columns = Math.max(1, Math.floor((width - labelWidth - gap) / step));
	const rowLayouts = useMemo(() => {
		let top = 0;
		let start = 0;
		return model.rows.map((row) => {
			const lineCount = Math.max(1, Math.ceil(row.cells.length / columns));
			const height = 24 + lineCount * step + 8;
			const layout = { top, height, start, lineCount };
			top += height;
			start += row.cells.length;
			return layout;
		});
	}, [columns, model.rows, step]);
	const contentHeight = Math.max(
		1,
		rowLayouts.reduce((sum, item) => sum + item.height, 0),
	);
	const flatCells = useMemo(
		() => model.rows.flatMap((row) => row.cells),
		[model.rows],
	);
	const canvasRefs = useRef(new Map<number, HTMLCanvasElement>());

	useLayoutEffect(() => {
		const root = rootRef.current;
		if (!root) return;
		const update = () =>
			setWidth(Math.round(root.getBoundingClientRect().width));
		update();
		const observer = new ResizeObserver(update);
		observer.observe(root);
		return () => observer.disconnect();
	}, []);

	useEffect(() => {
		if (!width) return;
		for (let rowIndex = 0; rowIndex < model.rows.length; rowIndex += 1) {
			const canvas = canvasRefs.current.get(rowIndex);
			const layout = rowLayouts[rowIndex];
			if (!canvas || !layout) continue;
			const dpr = window.devicePixelRatio || 1;
			canvas.width = Math.ceil(width * dpr);
			canvas.height = Math.ceil(layout.height * dpr);
			canvas.style.width = `${width}px`;
			canvas.style.height = `${layout.height}px`;
			const context = canvas.getContext("2d");
			if (!context) continue;
			context.setTransform(dpr, 0, 0, dpr, 0, 0);
			context.clearRect(0, 0, width, layout.height);
			context.fillStyle = getComputedStyle(canvas).color;
			context.font = "12px ui-monospace, SFMono-Regular, Menlo, monospace";
			context.textBaseline = "top";
			context.fillText(model.rows[rowIndex].label, 4, 5);
			for (
				let index = 0;
				index < model.rows[rowIndex].cells.length;
				index += 1
			) {
				const cell = model.rows[rowIndex].cells[index];
				const x = labelWidth + (index % columns) * step;
				const y = 24 + Math.floor(index / columns) * step;
				context.fillStyle = cell.color ?? "#9ca3af";
				context.fillRect(x, y, cellSize, cellSize);
				if (cell.id === activeId) {
					context.strokeStyle = "#f8fafc";
					context.lineWidth = 2;
					context.strokeRect(x + 1, y + 1, cellSize - 2, cellSize - 2);
				}
			}
		}
	}, [
		activeId,
		cellSize,
		columns,
		labelWidth,
		model.rows,
		rowLayouts,
		step,
		width,
	]);

	const hitTest = useCallback(
		(
			event: {
				clientX: number;
				clientY: number;
				currentTarget: HTMLCanvasElement;
			},
			rowIndex: number,
		) => {
			const canvas = event.currentTarget;
			const rect = canvas.getBoundingClientRect();
			const x = event.clientX - rect.left;
			const y = event.clientY - rect.top;
			if (x < labelWidth || y < 24) return null;
			const column = Math.floor((x - labelWidth) / step);
			const line = Math.floor((y - 24) / step);
			if (
				column < 0 ||
				line < 0 ||
				(x - labelWidth) % step >= cellSize ||
				(y - 24) % step >= cellSize
			)
				return null;
			const cellIndex = line * columns + column;
			return model.rows[rowIndex].cells[cellIndex] ?? null;
		},
		[cellSize, columns, labelWidth, model.rows, step],
	);
	const onGridKeyDown = useCallback(
		(event: ReactKeyboardEvent<HTMLButtonElement>) => {
			if (flatCells.length === 0) return;
			const current = Math.max(
				0,
				flatCells.findIndex((cell) => cell.id === activeId),
			);
			let next = current;
			if (event.key === "ArrowLeft") next = Math.max(0, current - 1);
			if (event.key === "ArrowRight")
				next = Math.min(flatCells.length - 1, current + 1);
			if (event.key === "ArrowUp") next = Math.max(0, current - columns);
			if (event.key === "ArrowDown")
				next = Math.min(flatCells.length - 1, current + columns);
			if (event.key === "Home") next = 0;
			if (event.key === "End") next = flatCells.length - 1;
			if (event.key === "Enter") {
				event.preventDefault();
				onActivate(flatCells[current]);
				return;
			}
			if (event.key === "Escape") {
				setActiveId(null);
				return;
			}
			if (next === current && !["Home", "End"].includes(event.key)) return;
			event.preventDefault();
			setActiveId(flatCells[next].id);
		},
		[activeId, columns, flatCells, onActivate, setActiveId],
	);

	return (
		<button
			type="button"
			ref={rootRef}
			className="block w-full space-y-2 border-0 bg-transparent p-0 text-left"
			data-testid={model.testId ?? "activity-grid-canvas-grid"}
			style={{ minHeight: contentHeight }}
			aria-label={model.ariaLabel}
			tabIndex={0}
			onKeyDown={onGridKeyDown}
			onFocus={() => {
				if (!activeId && flatCells[0]) setActiveId(flatCells[0].id);
			}}
		>
			{model.rows.map((row, rowIndex) => {
				const layout = rowLayouts[rowIndex];
				return (
					<div
						key={row.id}
						className="relative"
						style={{ height: layout.height }}
					>
						<canvas
							ref={(node) => {
								if (node) canvasRefs.current.set(rowIndex, node);
								else canvasRefs.current.delete(rowIndex);
							}}
							className="block text-muted-foreground"
							onPointerMove={(event) => {
								const cell = hitTest(event, rowIndex);
								if (!cell) return;
								setActiveId(cell.id);
								setAnchor(event.currentTarget.getBoundingClientRect());
							}}
							onPointerLeave={() => setActiveId(null)}
							onClick={(event) => {
								const cell = hitTest(event, rowIndex);
								if (cell) {
									setActiveId(cell.id);
									onActivate(cell);
								}
							}}
						/>
					</div>
				);
			})}
			{activeId ? (
				<span className="sr-only" aria-live="polite">
					{flatCells.find((cell) => cell.id === activeId)?.ariaLabel}
				</span>
			) : null}
		</button>
	);
}

export function ActivityGrid({
	model,
	loading = false,
	refreshing = false,
	onActivate,
	onSelectionChange,
	showPreview = true,
	previewTestId = "activity-grid-preview",
	className,
}: ActivityGridProps) {
	const cells = useActivityCells(model);
	const cellRefs = useRef(new Map<string, HTMLButtonElement>());
	const longPressTimer = useRef<number | null>(null);
	const suppressTouchClickRef = useRef(false);
	const touchStart = useRef<{
		x: number;
		y: number;
		activated: boolean;
		cell: ActivityGridCell;
	} | null>(null);
	const [activeId, setActiveId] = useState<string | null>(null);
	const [pinnedId, setPinnedId] = useState<string | null>(null);
	const [anchor, setAnchor] = useState<Rect | null>(null);
	const gridRootRef = useRef<HTMLDivElement>(null);
	const activeCell =
		cells.find(({ cell }) => cell.id === (pinnedId ?? activeId))?.cell ?? null;
	const isMatrix = model.layout === "matrix";

	const syncAnchor = useCallback(() => {
		if (!activeCell || touchStart.current?.activated) return;
		const element = Array.from(
			gridRootRef.current?.querySelectorAll<HTMLButtonElement>(
				"[data-activity-cell-id]",
			) ?? [],
		).find((candidate) => candidate.dataset.activityCellId === activeCell.id);
		if (element) setAnchor(element.getBoundingClientRect());
	}, [activeCell]);

	useLayoutEffect(() => {
		if (!activeCell) {
			setAnchor(null);
			return;
		}
		syncAnchor();
		const frame = window.requestAnimationFrame(syncAnchor);
		return () => window.cancelAnimationFrame(frame);
	}, [activeCell, model, syncAnchor]);

	useEffect(() => {
		if (!activeCell) return;
		const update = () => syncAnchor();
		window.addEventListener("resize", update);
		window.addEventListener("scroll", update, true);
		return () => {
			window.removeEventListener("resize", update);
			window.removeEventListener("scroll", update, true);
		};
	}, [activeCell, syncAnchor]);

	useEffect(() => {
		if (pinnedId === null) return;
		const closeOutside = (event: PointerEvent) => {
			const target = event.target as Node | null;
			if (
				target &&
				target instanceof Element &&
				(target.closest("[data-activity-grid-root]") ||
					target.closest("[data-activity-grid-preview]"))
			)
				return;
			setPinnedId(null);
		};
		const closeOnEscape = (event: KeyboardEvent) => {
			if (event.key === "Escape") setPinnedId(null);
		};
		document.addEventListener("pointerdown", closeOutside);
		document.addEventListener("keydown", closeOnEscape);
		return () => {
			document.removeEventListener("pointerdown", closeOutside);
			document.removeEventListener("keydown", closeOnEscape);
		};
	}, [pinnedId]);

	useEffect(() => {
		onSelectionChange?.(activeCell);
	}, [activeCell, onSelectionChange]);

	useEffect(
		() => () => {
			if (longPressTimer.current !== null)
				window.clearTimeout(longPressTimer.current);
		},
		[],
	);

	const moveSelection = useCallback(
		(currentId: string | null, direction: "left" | "right" | "up" | "down") => {
			const currentIndex = Math.max(
				0,
				cells.findIndex(({ cell }) => cell.id === currentId),
			);
			let nextIndex = currentIndex;
			if (model.layout === "matrix") {
				const rowIndex = cells[currentIndex]?.rowIndex ?? 0;
				const columnIndex = cells[currentIndex]?.cellIndex ?? 0;
				const rowLength = model.columns.length;
				if (direction === "left") nextIndex = Math.max(0, currentIndex - 1);
				if (direction === "right")
					nextIndex = Math.min(cells.length - 1, currentIndex + 1);
				if (direction === "up")
					nextIndex = Math.max(0, currentIndex - rowLength);
				if (direction === "down")
					nextIndex = Math.min(cells.length - 1, currentIndex + rowLength);
				if (direction === "left" || direction === "right") {
					const targetRow = cells[nextIndex]?.rowIndex;
					if (targetRow !== rowIndex) nextIndex = currentIndex;
				}
				if (direction === "up" || direction === "down") {
					const targetColumn = cells[nextIndex]?.cellIndex;
					if (targetColumn !== columnIndex) nextIndex = currentIndex;
				}
			} else {
				if (direction === "left") nextIndex = Math.max(0, currentIndex - 1);
				if (direction === "right")
					nextIndex = Math.min(cells.length - 1, currentIndex + 1);
				if (direction === "up") nextIndex = Math.max(0, currentIndex - 1);
				if (direction === "down")
					nextIndex = Math.min(cells.length - 1, currentIndex + 1);
			}
			const next = cells[nextIndex]?.cell ?? null;
			if (!next) return;
			setActiveId(next.id);
			cellRefs.current.get(next.id)?.focus();
		},
		[cells, model],
	);

	const getHandlers = useCallback(
		(cell: ActivityGridCell, rowIndex: number, cellIndex: number) => {
			void rowIndex;
			void cellIndex;
			const setCurrent = (
				next: ActivityGridCell | null,
				element?: HTMLElement,
			) => {
				setActiveId(next?.id ?? null);
				if (element) setAnchor(element.getBoundingClientRect());
			};
			return {
				cell,
				selected: activeId === cell.id || pinnedId === cell.id,
				setRef: (element: HTMLButtonElement | null) => {
					if (element) cellRefs.current.set(cell.id, element);
					else cellRefs.current.delete(cell.id);
				},
				onPointerEnter: (event: ReactPointerEvent<HTMLButtonElement>) => {
					if (event.pointerType !== "touch" && pinnedId === null)
						setCurrent(cell, event.currentTarget);
				},
				onPointerLeave: (event: ReactPointerEvent<HTMLButtonElement>) => {
					if (
						!isMatrix &&
						event.pointerType !== "touch" &&
						pinnedId === null &&
						document.activeElement !== event.currentTarget
					)
						setCurrent(null);
				},
				onFocus: (event: ReactFocusEvent<HTMLButtonElement>) =>
					setCurrent(cell, event.currentTarget),
				onBlur: (event: ReactFocusEvent<HTMLButtonElement>) => {
					if (pinnedId === null && !event.currentTarget.matches(":hover"))
						setCurrent(null);
				},
				onPointerDown: (event: ReactPointerEvent<HTMLButtonElement>) => {
					if (event.pointerType !== "touch") return;
					suppressTouchClickRef.current = false;
					touchStart.current = {
						x: event.clientX,
						y: event.clientY,
						activated: false,
						cell,
					};
					if (longPressTimer.current !== null)
						window.clearTimeout(longPressTimer.current);
					longPressTimer.current = window.setTimeout(() => {
						if (!touchStart.current) return;
						touchStart.current.activated = true;
						setCurrent(cell, event.currentTarget);
						setAnchor({
							left: event.clientX,
							top: event.clientY,
							right: event.clientX,
							bottom: event.clientY,
						});
					}, LONG_PRESS_MS);
				},
				onPointerMove: (event: ReactPointerEvent<HTMLButtonElement>) => {
					const start = touchStart.current;
					if (event.pointerType !== "touch" || !start) return;
					const moved = Math.hypot(
						event.clientX - start.x,
						event.clientY - start.y,
					);
					if (!start.activated && moved > TOUCH_MOVE_THRESHOLD) {
						if (longPressTimer.current !== null)
							window.clearTimeout(longPressTimer.current);
						touchStart.current = null;
						return;
					}
					if (start.activated) {
						event.preventDefault();
						setCurrent(cell, event.currentTarget);
						setAnchor({
							left: event.clientX,
							top: event.clientY,
							right: event.clientX,
							bottom: event.clientY,
						});
					}
				},
				onPointerUp: (event: ReactPointerEvent<HTMLButtonElement>) => {
					if (event.pointerType !== "touch") return;
					if (longPressTimer.current !== null)
						window.clearTimeout(longPressTimer.current);
					const start = touchStart.current;
					touchStart.current = null;
					suppressTouchClickRef.current = true;
					window.setTimeout(() => {
						suppressTouchClickRef.current = false;
					}, 500);
					if (start?.activated) {
						setPinnedId(isMatrix ? cell.id : null);
						onActivate(cell);
					}
				},
				onClick: (event: ReactMouseEvent<HTMLButtonElement>) => {
					if (suppressTouchClickRef.current) {
						suppressTouchClickRef.current = false;
						return;
					}
					if (isMatrix) {
						setPinnedId((current) => (current === cell.id ? null : cell.id));
						setActiveId(cell.id);
						setAnchor(event.currentTarget.getBoundingClientRect());
						return;
					}
					onActivate(cell);
				},
				onKeyDown: (event: ReactKeyboardEvent<HTMLButtonElement>) => {
					if (event.key === "Enter") {
						event.preventDefault();
						if (isMatrix) setPinnedId(cell.id);
						onActivate(cell);
						return;
					}
					if (event.key === "Escape") {
						setPinnedId(null);
						setActiveId(null);
						return;
					}
					const directions: Record<string, "left" | "right" | "up" | "down"> = {
						ArrowLeft: "left",
						ArrowRight: "right",
						ArrowUp: "up",
						ArrowDown: "down",
					};
					const direction = directions[event.key];
					if (!direction) return;
					event.preventDefault();
					moveSelection(cell.id, direction);
				},
			};
		},
		[activeId, isMatrix, moveSelection, onActivate, pinnedId],
	);

	if (loading) {
		return (
			<div className={className} role="status" aria-busy="true">
				<ActivityGridSkeleton model={model} />
				{refreshing ? <span className="sr-only">更新中</span> : null}
			</div>
		);
	}

	const totalCells = cells.length;
	const useCanvas =
		model.layout === "wrapped-rows" && totalCells > DENSE_CELL_LIMIT;
	return (
		<div
			ref={gridRootRef}
			className={className}
			data-activity-grid-root="true"
			data-activity-grid-layout={model.layout}
			data-activity-grid-scroll={model.scrollPolicy}
		>
			{useCanvas ? (
				<DenseCanvasGrid
					model={{ ...model, testId: "collection-activity-canvas-grid" }}
					activeId={activeId}
					setActiveId={setActiveId}
					onActivate={onActivate}
					setAnchor={setAnchor}
				/>
			) : model.layout === "wrapped-rows" ? (
				<WrappedRowsGrid
					model={model}
					activeId={activeId}
					setCellRef={(id, element) => {
						if (element) cellRefs.current.set(id, element);
						else cellRefs.current.delete(id);
					}}
					getHandlers={getHandlers}
				/>
			) : (
				<MatrixGrid
					model={model}
					activeId={activeId}
					setCellRef={(id, element) => {
						if (element) cellRefs.current.set(id, element);
						else cellRefs.current.delete(id);
					}}
					getHandlers={getHandlers}
				/>
			)}
			{showPreview ? (
				<PreviewLayer
					cell={activeCell}
					anchor={anchor}
					pinned={pinnedId !== null}
					testId={previewTestId}
					onClose={() => setPinnedId(null)}
				/>
			) : null}
		</div>
	);
}

export { ActivityGridSkeleton };
