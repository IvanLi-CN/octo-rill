import { ArrowUpRight, Languages, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { apiGet, apiPostJson } from "@/api";
import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type {
	ReleaseDetail,
	ReleaseDetailTranslateResponse,
} from "@/releases/types";

type LanguageMode = "zh" | "original";
const DRAWER_ANIMATION_MS = 220;

function formatIsoShort(iso: string | null) {
	if (!iso) return "unknown time";
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

export function ReleaseDetailDrawer(props: {
	releaseId: string | null;
	onClose: () => void;
}) {
	const { releaseId, onClose } = props;

	const [detail, setDetail] = useState<ReleaseDetail | null>(null);
	const [translated, setTranslated] =
		useState<ReleaseDetailTranslateResponse | null>(null);
	const [language, setLanguage] = useState<LanguageMode>("zh");
	const [loadingDetail, setLoadingDetail] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [translateError, setTranslateError] = useState<string | null>(null);
	const detailRequestSeqRef = useRef(0);
	const translateRequestSeqRef = useRef(0);
	const closeTimerRef = useRef<number | null>(null);

	const open = Boolean(releaseId);
	const [rendered, setRendered] = useState(open);
	const [visible, setVisible] = useState(open);

	useEffect(() => {
		if (open) {
			if (closeTimerRef.current !== null) {
				window.clearTimeout(closeTimerRef.current);
				closeTimerRef.current = null;
			}
			setRendered(true);
			const rafId = window.requestAnimationFrame(() => setVisible(true));
			return () => window.cancelAnimationFrame(rafId);
		}

		setVisible(false);
		if (!rendered) return;
		if (closeTimerRef.current !== null) {
			window.clearTimeout(closeTimerRef.current);
		}
		closeTimerRef.current = window.setTimeout(() => {
			setRendered(false);
			closeTimerRef.current = null;
		}, DRAWER_ANIMATION_MS);
	}, [open, rendered]);

	useEffect(
		() => () => {
			if (closeTimerRef.current !== null) {
				window.clearTimeout(closeTimerRef.current);
			}
		},
		[],
	);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		setTranslating(false);

		if (!open || !releaseId) {
			detailRequestSeqRef.current += 1;
			if (!open && rendered) return;
			setDetail(null);
			setTranslated(null);
			setError(null);
			setTranslateError(null);
			setLanguage("zh");
			return;
		}

		const requestSeq = detailRequestSeqRef.current + 1;
		detailRequestSeqRef.current = requestSeq;

		setLoadingDetail(true);
		setDetail(null);
		setTranslated(null);
		setError(null);
		setTranslateError(null);
		setLanguage("zh");

		void apiGet<ReleaseDetail>(
			`/api/releases/${encodeURIComponent(releaseId)}/detail`,
		)
			.then((res) => {
				if (detailRequestSeqRef.current !== requestSeq) return;
				setDetail(res);
			})
			.catch((err) => {
				if (detailRequestSeqRef.current !== requestSeq) return;
				setError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				if (detailRequestSeqRef.current !== requestSeq) return;
				setLoadingDetail(false);
			});
	}, [open, releaseId, rendered]);

	useEffect(() => {
		if (!open || !releaseId || !detail || language !== "zh") return;
		if (translated || translating) return;

		const requestSeq = translateRequestSeqRef.current + 1;
		translateRequestSeqRef.current = requestSeq;
		setTranslating(true);
		setTranslateError(null);
		void apiPostJson<ReleaseDetailTranslateResponse>(
			"/api/translate/release/detail",
			{
				release_id: releaseId,
			},
		)
			.then((res) => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslated(res);
				if (res.status === "disabled") {
					setLanguage("original");
				}
			})
			.catch((err) => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslateError(err instanceof Error ? err.message : String(err));
				setLanguage("original");
			})
			.finally(() => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslating(false);
			});
	}, [open, releaseId, detail, translated, translating, language]);

	useEffect(() => {
		if (!open) return;
		const onKey = (event: KeyboardEvent) => {
			if (event.key === "Escape") onClose();
		};
		window.addEventListener("keydown", onKey);
		return () => window.removeEventListener("keydown", onKey);
	}, [open, onClose]);

	const displayTitle = useMemo(() => {
		if (language === "zh" && translated?.title?.trim()) return translated.title;
		return detail?.title ?? "(no title)";
	}, [detail, translated, language]);

	const bodyContent = useMemo(() => {
		if (!detail) return "";
		if (language === "zh" && translated?.status === "ready") {
			return translated.body_markdown?.trim() || detail.body;
		}
		return detail.body;
	}, [detail, translated, language]);

	if (!rendered) return null;

	return (
		<div
			className={cn(
				"fixed inset-0 z-50 flex justify-end",
				open ? "pointer-events-auto" : "pointer-events-none",
			)}
		>
			<button
				type="button"
				className={cn(
					"bg-background/40 absolute inset-0 backdrop-blur-[1px] transition-opacity duration-200",
					visible ? "opacity-100" : "opacity-0",
				)}
				onClick={onClose}
				disabled={!open}
				aria-label="Close release detail"
			/>

			<section
				className={cn(
					"bg-card relative z-10 flex h-full w-full max-w-3xl flex-col border-l shadow-2xl transition-all duration-200 ease-out",
					visible ? "translate-x-0 opacity-100" : "translate-x-8 opacity-0",
				)}
				style={{ transitionDuration: `${DRAWER_ANIMATION_MS}ms` }}
			>
				<header className="border-b p-4">
					<div className="flex items-start justify-between gap-3">
						<div className="min-w-0">
							<p className="text-muted-foreground font-mono text-xs">
								{detail?.full_name ?? "Loading…"} ·{" "}
								{detail ? formatIsoShort(detail.published_at) : ""}
								{detail?.is_prerelease ? " · 预发布" : ""}
							</p>
							<h2 className="mt-1 text-base font-semibold tracking-tight">
								{displayTitle}
							</h2>
						</div>
						<div className="flex shrink-0 items-center gap-2">
							<Button
								variant="ghost"
								size="sm"
								className="font-mono text-xs"
								disabled={!detail || translated?.status === "disabled"}
								onClick={() =>
									setLanguage((prev) => (prev === "zh" ? "original" : "zh"))
								}
							>
								<Languages className="size-4" />
								{language === "zh" ? "原文" : "中文"}
							</Button>
							<Button
								asChild
								variant="outline"
								size="sm"
								className="font-mono text-xs"
								disabled={!detail}
							>
								<a
									href={detail?.html_url ?? "#"}
									target="_blank"
									rel="noreferrer"
								>
									<ArrowUpRight className="size-4" />
									GitHub
								</a>
							</Button>
							<Button
								variant="ghost"
								size="icon"
								onClick={onClose}
								aria-label="Close release detail"
							>
								<X className="size-4" />
							</Button>
						</div>
					</div>
				</header>

				<div className="min-h-0 flex-1 overflow-auto p-4">
					{loadingDetail ? (
						<p className="text-muted-foreground text-sm">加载中…</p>
					) : null}

					{error ? <p className="text-destructive text-sm">{error}</p> : null}

					{translated?.status === "disabled" ? (
						<p className="text-muted-foreground mb-3 text-xs">
							AI 未配置，默认展示原文。
						</p>
					) : null}

					{translateError ? (
						<p className="text-destructive mb-3 text-xs">{translateError}</p>
					) : null}

					{translating && language === "zh" ? (
						<p className="text-muted-foreground mb-3 text-xs">翻译中…</p>
					) : null}

					{detail ? (
						<div className="bg-muted/10 rounded-lg border p-4">
							<Markdown content={bodyContent} />
						</div>
					) : null}
				</div>
			</section>
		</div>
	);
}
