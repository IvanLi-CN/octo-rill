import { ArrowUpRight, Languages, RefreshCcw } from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type ReleaseDetailResponse,
	type TranslationRequestResponse,
	ApiError,
	apiGetReleaseDetail,
	apiGetTranslationRequest,
	apiTranslateReleaseDetail,
	isPendingTranslationResultStatus,
	mapTranslationResultToReleaseDetailTranslated,
} from "@/api";
import { Markdown } from "@/components/Markdown";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { formatIsoShortLocal } from "@/lib/datetime";
import { normalizeReleaseId } from "@/lib/releaseId";

const REQUEST_STATUS_POLL_INTERVAL_MS = 600;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_NOT_FOUND_ERROR_CODE = "not_found";

function sleep(ms: number) {
	return new Promise((resolve) => {
		window.setTimeout(resolve, ms);
	});
}

function isMissingTranslationRequestError(error: unknown) {
	return (
		error instanceof ApiError &&
		(error.status === 404 || error.code === REQUEST_NOT_FOUND_ERROR_CODE)
	);
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
	const pendingTranslationRequestRef = useRef<{
		releaseId: string;
		requestId: string;
	} | null>(null);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		setTranslating(false);
		if (!normalizedReleaseId) {
			pendingTranslationRequestRef.current = null;
			setDetail(null);
			setLoadError(null);
			setTranslateError(null);
			return;
		}
		let active = true;
		pendingTranslationRequestRef.current = null;
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
		void (async () => {
			let requestId =
				pendingTranslationRequestRef.current?.releaseId === requestReleaseId
					? pendingTranslationRequestRef.current.requestId
					: null;
			let response: TranslationRequestResponse | null = null;
			for (let attempt = 0; attempt < 2; attempt += 1) {
				try {
					response = requestId
						? await apiGetTranslationRequest(requestId)
						: await apiTranslateReleaseDetail(activeDetail);
					const deadline = Date.now() + REQUEST_STATUS_POLL_WINDOW_MS;
					while (isPendingTranslationResultStatus(response.result.status)) {
						pendingTranslationRequestRef.current = {
							releaseId: requestReleaseId,
							requestId: response.request_id,
						};
						if (Date.now() >= deadline) {
							throw new Error(
								"translation is still processing; please try again shortly",
							);
						}
						if (translateRequestSeqRef.current !== requestSeq) return;
						await sleep(REQUEST_STATUS_POLL_INTERVAL_MS);
						if (translateRequestSeqRef.current !== requestSeq) return;
						response = await apiGetTranslationRequest(response.request_id);
					}
					break;
				} catch (error) {
					if (!isMissingTranslationRequestError(error) || attempt === 1) {
						throw error;
					}
					pendingTranslationRequestRef.current = null;
					requestId = null;
				}
			}
			if (!response) {
				throw new Error("translation request could not be recovered");
			}
			pendingTranslationRequestRef.current = null;
			if (translateRequestSeqRef.current !== requestSeq) return;
			const translated = mapTranslationResultToReleaseDetailTranslated(
				response.result,
			);
			if (!translated) {
				throw new Error(response.result.error ?? "translate failed");
			}
			setDetail((prev) => {
				if (!prev) return prev;
				if (prev.release_id !== requestReleaseId) return prev;
				return { ...prev, translated };
			});
		})()
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
		<Dialog
			open
			onOpenChange={(open) => {
				if (!open) onClose();
			}}
		>
			<DialogContent className="flex max-h-[calc(100vh-2rem)] max-w-[calc(100%-2rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-4xl">
				<DialogHeader className="border-b px-6 py-5 text-left">
					<div className="flex flex-col gap-4 sm:flex-row sm:items-start sm:justify-between">
						<div className="min-w-0">
							<DialogTitle className="text-base">Release 详情</DialogTitle>
							<DialogDescription className="font-mono text-xs">
								{loading ? "加载中…" : `#${normalizedReleaseId}`}
								{activeDetail?.published_at
									? ` · ${formatIsoShortLocal(activeDetail.published_at)}`
									: ""}
							</DialogDescription>
						</div>

						<div className="flex shrink-0 flex-wrap items-center gap-2">
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
						</div>
					</div>
				</DialogHeader>

				<div className="overflow-y-auto px-6 py-5">
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
								<div className="bg-muted/10 rounded-lg border p-4">
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
				</div>
			</DialogContent>
		</Dialog>
	);
}
