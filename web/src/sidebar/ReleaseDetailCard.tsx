import { ArrowUpRight, Languages, RefreshCcw, X } from "lucide-react";
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
import { useAppToast } from "@/components/feedback/AppToast";
import { ErrorStatePanel } from "@/components/feedback/ErrorStatePanel";
import { Markdown } from "@/components/Markdown";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Button } from "@/components/ui/button";
import {
	Dialog,
	DialogContent,
	DialogDescription,
	DialogHeader,
	DialogTitle,
} from "@/components/ui/dialog";
import { formatIsoShortLocal } from "@/lib/datetime";
import {
	describeUnknownError,
	resolveErrorDetail,
	resolveErrorSummary,
} from "@/lib/errorPresentation";
import { normalizeReleaseId } from "@/lib/releaseId";

const REQUEST_STATUS_POLL_INTERVAL_MS = 600;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_NOT_FOUND_ERROR_CODE = "not_found";

type ReleaseDetailUiError = {
	summary: string;
	detail?: string | null;
};

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

function toUiError(
	value:
		| {
				error?: string | null;
				error_summary?: string | null;
				error_detail?: string | null;
		  }
		| null
		| undefined,
	fallback: string,
): ReleaseDetailUiError {
	return {
		summary: resolveErrorSummary(value, fallback),
		detail: resolveErrorDetail(value),
	};
}

function toUnknownUiError(
	error: unknown,
	fallback: string,
): ReleaseDetailUiError {
	return {
		summary: describeUnknownError(error, fallback),
		detail: error instanceof Error ? error.message : null,
	};
}

function hasReadyTranslatedContent(
	translated: ReleaseDetailResponse["translated"] | null | undefined,
) {
	if (translated?.status !== "ready") {
		return false;
	}
	return Boolean(translated.title?.trim() || translated.summary?.trim());
}

export function ReleaseDetailCard(props: {
	releaseId: string | null;
	onClose: () => void;
}) {
	const { releaseId, onClose } = props;
	const { pushErrorToast } = useAppToast();
	const normalizedReleaseId = useMemo(
		() => normalizeReleaseId(releaseId),
		[releaseId],
	);
	const [loading, setLoading] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [loadError, setLoadError] = useState<ReleaseDetailUiError | null>(null);
	const [translateError, setTranslateError] =
		useState<ReleaseDetailUiError | null>(null);
	const [showOriginal, setShowOriginal] = useState(false);
	const [detail, setDetail] = useState<ReleaseDetailResponse | null>(null);
	const translateRequestSeqRef = useRef(0);
	const loadRequestSeqRef = useRef(0);
	const pendingTranslationRequestRef = useRef<{
		releaseId: string;
		requestId: string;
	} | null>(null);

	const loadDetail = useCallback(
		(targetReleaseId: string, options?: { resetDisplay?: boolean }) => {
			const requestSeq = loadRequestSeqRef.current + 1;
			loadRequestSeqRef.current = requestSeq;
			pendingTranslationRequestRef.current = null;
			setLoading(true);
			setLoadError(null);
			setTranslateError(null);
			if (options?.resetDisplay !== false) {
				setShowOriginal(false);
				setDetail(null);
			}
			void apiGetReleaseDetail(targetReleaseId)
				.then((response) => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setDetail(response);
				})
				.catch((error) => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setDetail(null);
					setLoadError(
						toUnknownUiError(error, "Release 详情加载失败，请稍后重试。"),
					);
				})
				.finally(() => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setLoading(false);
				});
		},
		[],
	);

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
		loadDetail(normalizedReleaseId, { resetDisplay: true });
	}, [loadDetail, normalizedReleaseId]);

	const activeDetail = useMemo(() => {
		if (!normalizedReleaseId || !detail) return null;
		return detail.release_id === normalizedReleaseId ? detail : null;
	}, [detail, normalizedReleaseId]);

	const detailTranslationError = useMemo(() => {
		if (activeDetail?.translated?.status !== "error") {
			return null;
		}
		return toUiError(activeDetail.translated, "这次翻译没有成功完成。");
	}, [activeDetail]);

	const activeTranslationError = showOriginal
		? null
		: (translateError ?? detailTranslationError);

	const onTranslate = useCallback(() => {
		if (!activeDetail || translating) return;
		const requestSeq = translateRequestSeqRef.current + 1;
		translateRequestSeqRef.current = requestSeq;
		const requestReleaseId = activeDetail.release_id;
		const preserveReadyTranslation = hasReadyTranslatedContent(
			activeDetail.translated,
		);
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
				throw new Error(resolveErrorSummary(response.result, "翻译失败"));
			}
			if (preserveReadyTranslation && translated.status !== "ready") {
				const failure =
					translated.status === "disabled"
						? toUiError(response.result, "AI 未配置，暂时无法重新翻译。")
						: toUiError(response.result, "翻译失败，请稍后重试。");
				pushErrorToast(
					translated.status === "disabled" ? "翻译不可用" : "翻译失败",
					failure.summary,
					{ detail: failure.detail },
				);
				return;
			}
			setDetail((prev) => {
				if (!prev) return prev;
				if (prev.release_id !== requestReleaseId) return prev;
				return { ...prev, translated };
			});
			setTranslateError(
				translated.status === "error"
					? toUiError(translated, "这次翻译没有成功完成。")
					: null,
			);
			if (translated.status === "ready") {
				setShowOriginal(false);
			}
		})()
			.catch((error) => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				if (preserveReadyTranslation) {
					const failure = toUnknownUiError(error, "翻译失败，请稍后重试。");
					pushErrorToast("翻译失败", failure.summary, {
						detail: failure.detail,
					});
					return;
				}
				setTranslateError(toUnknownUiError(error, "翻译失败，请稍后重试。"));
			})
			.finally(() => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslating(false);
			});
	}, [activeDetail, pushErrorToast, translating]);

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
		return hasReadyTranslatedContent(activeDetail?.translated);
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
			<DialogContent
				showCloseButton={false}
				className="flex max-h-[calc(100vh-2rem)] max-w-[calc(100%-2rem)] flex-col gap-0 overflow-hidden p-0 sm:max-w-4xl"
			>
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
											onClick={() => setShowOriginal((value) => !value)}
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
								className="shrink-0 font-mono text-xs"
								onClick={onClose}
							>
								<X className="size-4" />
								关闭
							</Button>
						</div>
					</div>
				</DialogHeader>

				<div className="overflow-y-auto px-6 py-5">
					{loadError ? (
						<ErrorStatePanel
							title="Release 详情加载失败"
							summary={loadError.summary}
							detail={loadError.detail}
							actions={
								<div className="flex flex-wrap gap-2">
									<Button
										variant="outline"
										size="sm"
										className="font-mono text-xs"
										onClick={() =>
											loadDetail(normalizedReleaseId, { resetDisplay: true })
										}
									>
										<RefreshCcw className="size-4" />
										重试
									</Button>
								</div>
							}
						/>
					) : loading ? (
						<p className="text-muted-foreground text-sm">
							正在加载 release 详情…
						</p>
					) : activeDetail ? (
						activeTranslationError ? (
							<ErrorStatePanel
								title="翻译失败"
								summary={activeTranslationError.summary}
								detail={activeTranslationError.detail}
								actions={
									<div className="flex flex-wrap gap-2">
										<Button
											variant="outline"
											size="sm"
											className="font-mono text-xs"
											onClick={onTranslate}
											disabled={translating}
										>
											<RefreshCcw className="size-4" />
											{translating ? "翻译中…" : "重试翻译"}
										</Button>
										<Button
											variant="ghost"
											size="sm"
											className="font-mono text-xs"
											onClick={() => setShowOriginal(true)}
										>
											<Languages className="size-4" />
											查看原文
										</Button>
										{activeDetail.html_url ? (
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
								}
							/>
						) : display ? (
							<div className="space-y-3">
								<h3 className="text-sm font-semibold tracking-tight">
									{display.title}
								</h3>
								<RepoIdentity
									repoFullName={activeDetail.repo_full_name ?? null}
									repoVisual={activeDetail.repo_visual ?? null}
									className="max-w-full"
									labelClassName="font-mono text-base font-medium tracking-tight text-foreground/80"
									visualClassName="size-8"
								/>
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
							<p className="text-muted-foreground text-sm">
								未找到该 release。
							</p>
						)
					) : (
						<p className="text-muted-foreground text-sm">未找到该 release。</p>
					)}
				</div>
			</DialogContent>
		</Dialog>
	);
}
