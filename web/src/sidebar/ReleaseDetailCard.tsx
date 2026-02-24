import { ArrowUpRight, Languages, RefreshCcw, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type ReleaseDetailResponse,
	apiGetReleaseDetail,
	apiTranslateReleaseDetail,
} from "@/api";
import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import {
	Card,
	CardContent,
	CardDescription,
	CardHeader,
	CardTitle,
} from "@/components/ui/card";
import { normalizeReleaseId } from "@/lib/releaseId";

function formatIsoShort(iso: string | null) {
	if (!iso) return null;
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

export function ReleaseDetailCard(props: {
	releaseId: string | null;
	onClose: () => void;
}) {
	const { releaseId, onClose } = props;
	const normalizedReleaseId = useMemo(
		() => normalizeReleaseId(releaseId),
		[releaseId],
	);
	const [loading, setLoading] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [loadError, setLoadError] = useState<string | null>(null);
	const [translateError, setTranslateError] = useState<string | null>(null);
	const [showOriginal, setShowOriginal] = useState(false);
	const [detail, setDetail] = useState<ReleaseDetailResponse | null>(null);
	const translateRequestSeqRef = useRef(0);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		setTranslating(false);
		if (!normalizedReleaseId) {
			setDetail(null);
			setLoadError(null);
			setTranslateError(null);
			return;
		}
		let active = true;
		setDetail(null);
		setLoading(true);
		setLoadError(null);
		setTranslateError(null);
		setShowOriginal(false);
		void apiGetReleaseDetail(normalizedReleaseId)
			.then((res) => {
				if (!active) return;
				setDetail(res);
			})
			.catch((err) => {
				if (!active) return;
				setLoadError(err instanceof Error ? err.message : String(err));
				setDetail(null);
			})
			.finally(() => {
				if (!active) return;
				setLoading(false);
			});

		return () => {
			active = false;
		};
	}, [normalizedReleaseId]);

	const activeDetail = useMemo(() => {
		if (!normalizedReleaseId || !detail) return null;
		return detail.release_id === normalizedReleaseId ? detail : null;
	}, [detail, normalizedReleaseId]);

	const onTranslate = useCallback(() => {
		if (!activeDetail || translating) return;
		const requestSeq = translateRequestSeqRef.current + 1;
		translateRequestSeqRef.current = requestSeq;
		const requestReleaseId = activeDetail.release_id;
		setTranslating(true);
		setTranslateError(null);
		void apiTranslateReleaseDetail(requestReleaseId)
			.then((translated) => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setDetail((prev) => {
					if (!prev) return prev;
					if (prev.release_id !== requestReleaseId) return prev;
					return { ...prev, translated };
				});
			})
			.catch((err) => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslateError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslating(false);
			});
	}, [activeDetail, translating]);

	const display = useMemo(() => {
		if (!activeDetail) return null;
		const originalTitle =
			activeDetail.name?.trim() && activeDetail.name.trim().length > 0
				? activeDetail.name
				: activeDetail.tag_name;
		const translatedTitle =
			activeDetail.translated?.status === "ready"
				? activeDetail.translated.title
				: null;
		const title = showOriginal
			? originalTitle
			: translatedTitle?.trim() || originalTitle;

		const translatedBody =
			activeDetail.translated?.status === "ready"
				? activeDetail.translated.summary
				: null;
		const originalBody = activeDetail.body?.trim() ? activeDetail.body : null;
		const body = showOriginal
			? originalBody
			: translatedBody?.trim()
				? translatedBody
				: originalBody;

		return { title, body };
	}, [activeDetail, showOriginal]);

	const hasReadyTranslation = useMemo(() => {
		if (!activeDetail || activeDetail.translated?.status !== "ready")
			return false;
		const titleReady = Boolean(activeDetail.translated.title?.trim());
		const summaryReady = Boolean(activeDetail.translated.summary?.trim());
		return titleReady || summaryReady;
	}, [activeDetail]);

	if (!normalizedReleaseId) {
		return null;
	}

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<div className="flex items-start justify-between gap-3">
					<div className="min-w-0">
						<CardTitle className="text-base">Release 详情</CardTitle>
						<CardDescription className="font-mono text-xs">
							{loading ? "加载中…" : `#${normalizedReleaseId}`}
							{activeDetail?.published_at
								? ` · ${formatIsoShort(activeDetail.published_at)}`
								: ""}
						</CardDescription>
					</div>

					<div className="flex shrink-0 items-center gap-2">
						{activeDetail?.translated?.status === "disabled" ? (
							<span className="text-muted-foreground font-mono text-[11px]">
								AI 未配置
							</span>
						) : (
							<>
								{hasReadyTranslation ? (
									<Button
										variant="ghost"
										size="sm"
										className="font-mono text-xs"
										onClick={() => setShowOriginal((v) => !v)}
										disabled={loading || !activeDetail}
									>
										<Languages className="size-4" />
										{showOriginal ? "中文" : "原文"}
									</Button>
								) : null}
								<Button
									variant="ghost"
									size="sm"
									className="font-mono text-xs"
									onClick={onTranslate}
									disabled={loading || !activeDetail || translating}
								>
									<RefreshCcw className="size-4" />
									{translating ? "翻译中…" : "翻译"}
								</Button>
							</>
						)}

						{activeDetail?.html_url ? (
							<Button
								asChild
								variant="outline"
								size="sm"
								className="font-mono text-xs"
							>
								<a
									href={activeDetail.html_url}
									target="_blank"
									rel="noreferrer"
								>
									<ArrowUpRight className="size-4" />
									GitHub
								</a>
							</Button>
						) : null}
						<Button
							variant="ghost"
							size="sm"
							className="font-mono text-xs"
							onClick={onClose}
						>
							<X className="size-4" />
							关闭
						</Button>
					</div>
				</div>
			</CardHeader>

			<CardContent className="pt-0">
				{loadError ? (
					<p className="text-destructive text-sm">{loadError}</p>
				) : loading ? (
					<p className="text-muted-foreground text-sm">
						正在加载 release 详情…
					</p>
				) : display ? (
					<div className="space-y-3">
						{translateError ? (
							<p className="text-destructive text-xs">{translateError}</p>
						) : null}
						<h3 className="text-sm font-semibold tracking-tight">
							{display.title}
						</h3>
						{activeDetail?.repo_full_name ? (
							<p className="text-muted-foreground font-mono text-xs">
								{activeDetail.repo_full_name}
							</p>
						) : null}
						{display.body ? (
							<div className="bg-muted/10 max-h-96 overflow-auto rounded-lg border p-4">
								<Markdown content={display.body} />
							</div>
						) : (
							<p className="text-muted-foreground text-sm">
								该 release 无正文。
							</p>
						)}
					</div>
				) : (
					<p className="text-muted-foreground text-sm">未找到该 release。</p>
				)}
			</CardContent>
		</Card>
	);
}
