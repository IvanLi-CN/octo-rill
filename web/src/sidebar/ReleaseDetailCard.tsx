import {
	ArrowUpRight,
	FileText,
	Languages,
	RefreshCcw,
	Sparkles,
	X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type ReleaseDetailResponse,
	type TranslationRequestResponse,
	ApiError,
	apiGetReleaseDetail,
	apiGetReleaseDetailByRepoTag,
	apiResolveTranslationResults,
	apiGetTranslationRequest,
	apiTranslateReleaseDetail,
	isPendingTranslationResultStatus,
	mapTranslationResultToReleaseDetailSmart,
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
import type { DashboardReleaseTarget } from "@/dashboard/routeState";
import { formatIsoShortLocal } from "@/lib/datetime";
import {
	describeUnknownError,
	resolveErrorDetail,
	resolveErrorSummary,
} from "@/lib/errorPresentation";
import { normalizeReleaseId } from "@/lib/releaseId";
import { cn } from "@/lib/utils";

const REQUEST_STATUS_POLL_INTERVAL_MS = 600;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_NOT_FOUND_ERROR_CODE = "not_found";
const SMART_RESOLVE_MAX_WAIT_MS = 5_000;
const RELEASE_FEED_BODY_MAX_CHARS = 3_000;

type ReleaseDetailUiError = {
	summary: string;
	detail?: string | null;
};

type ReleaseDetailLane = "original" | "translated" | "smart";

const RELEASE_DETAIL_LANES: Array<{
	lane: ReleaseDetailLane;
	label: string;
	icon: typeof FileText;
}> = [
	{ lane: "original", label: "原文", icon: FileText },
	{ lane: "translated", label: "翻译", icon: Languages },
	{ lane: "smart", label: "润色", icon: Sparkles },
];

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

function shouldResolveSmart(
	smart: ReleaseDetailResponse["smart"] | null | undefined,
) {
	return (
		!smart ||
		(smart.status === "missing" && smart.auto_translate !== false) ||
		(smart.status === "error" && smart.auto_translate !== false)
	);
}

function truncateChars(raw: string, maxChars: number) {
	return Array.from(raw).slice(0, maxChars).join("");
}

function releaseFeedBody(body: string | null | undefined) {
	const trimmed = body?.replace(/\r\n/g, "\n").trim();
	if (!trimmed) return null;
	return truncateChars(trimmed, RELEASE_FEED_BODY_MAX_CHARS);
}

function releaseSmartMetadataText(detail: ReleaseDetailResponse) {
	const repoFullName = detail.repo_full_name?.trim() ?? "";
	let metadata = `repo=${repoFullName}\nhead_tag=${detail.tag_name}`;
	const previousTagName = detail.previous_tag_name?.trim();
	if (previousTagName) {
		metadata += `\ncompare_base_tag=${previousTagName}`;
	}
	return metadata;
}

function buildReleaseSmartRequestItem(detail: ReleaseDetailResponse) {
	const title =
		detail.name?.trim() || detail.tag_name || `release:${detail.release_id}`;
	const body = releaseFeedBody(detail.body);
	const metadata = releaseSmartMetadataText(detail);
	return {
		producer_ref: `feed.smart:release:${detail.release_id}`,
		kind: "release_smart" as const,
		variant: "feed_card",
		entity_id: detail.release_id,
		target_lang: "zh-CN",
		max_wait_ms: SMART_RESOLVE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title" as const, text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh" as const, "body_md" as const],
	};
}

function normalizeReleaseTarget(
	target: DashboardReleaseTarget | null,
): DashboardReleaseTarget | null {
	if (!target) return null;
	return {
		releaseId: normalizeReleaseId(target.releaseId) ?? null,
		locator: target.locator ?? null,
		fromTab: target.fromTab,
	};
}

function releaseTargetKey(target: DashboardReleaseTarget | null) {
	if (!target) return null;
	if (target.releaseId) {
		return `release:${target.releaseId}`;
	}
	if (target.locator) {
		return `locator:${target.locator.owner}/${target.locator.repo}#${target.locator.tag}`;
	}
	return null;
}

function releaseTargetDescription(target: DashboardReleaseTarget | null) {
	if (!target) return null;
	if (target.releaseId) {
		return `#${target.releaseId}`;
	}
	if (target.locator) {
		return `${target.locator.owner}/${target.locator.repo} · ${target.locator.tag}`;
	}
	return null;
}

async function fetchReleaseDetail(target: DashboardReleaseTarget) {
	if (target.locator) {
		return apiGetReleaseDetailByRepoTag(target.locator);
	}
	if (target.releaseId) {
		return apiGetReleaseDetail(target.releaseId);
	}
	throw new Error("missing release detail target");
}

export function ReleaseDetailCard(props: {
	target: DashboardReleaseTarget | null;
	onClose: () => void;
	onResolvedDetail?: (detail: ReleaseDetailResponse) => void;
}) {
	const { target, onClose, onResolvedDetail } = props;
	const { pushErrorToast } = useAppToast();
	const normalizedTarget = useMemo(
		() => normalizeReleaseTarget(target),
		[target],
	);
	const activeTargetKey = useMemo(
		() => releaseTargetKey(normalizedTarget),
		[normalizedTarget],
	);
	const [loading, setLoading] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [smartResolving, setSmartResolving] = useState(false);
	const [loadError, setLoadError] = useState<ReleaseDetailUiError | null>(null);
	const [translateError, setTranslateError] =
		useState<ReleaseDetailUiError | null>(null);
	const [smartError, setSmartError] = useState<ReleaseDetailUiError | null>(
		null,
	);
	const [selectedLane, setSelectedLane] = useState<ReleaseDetailLane>("smart");
	const [detail, setDetail] = useState<ReleaseDetailResponse | null>(null);
	const [detailTargetKey, setDetailTargetKey] = useState<string | null>(null);
	const translateRequestSeqRef = useRef(0);
	const smartRequestSeqRef = useRef(0);
	const loadRequestSeqRef = useRef(0);
	const smartAutoAttemptedKeyRef = useRef<string | null>(null);
	const pendingTranslationRequestRef = useRef<{
		releaseId: string;
		requestId: string;
	} | null>(null);

	const loadDetail = useCallback(
		(
			targetRelease: DashboardReleaseTarget,
			options?: { resetDisplay?: boolean },
		) => {
			const requestSeq = loadRequestSeqRef.current + 1;
			loadRequestSeqRef.current = requestSeq;
			pendingTranslationRequestRef.current = null;
			setLoading(true);
			setLoadError(null);
			setTranslateError(null);
			setSmartError(null);
			if (options?.resetDisplay !== false) {
				setSelectedLane("smart");
				setDetail(null);
				setDetailTargetKey(null);
			}
			void fetchReleaseDetail(targetRelease)
				.then((response) => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setDetail(response);
					setDetailTargetKey(releaseTargetKey(targetRelease));
					onResolvedDetail?.(response);
				})
				.catch((error) => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setDetail(null);
					setDetailTargetKey(null);
					setLoadError(
						toUnknownUiError(error, "Release 详情加载失败，请稍后重试。"),
					);
				})
				.finally(() => {
					if (loadRequestSeqRef.current !== requestSeq) return;
					setLoading(false);
				});
		},
		[onResolvedDetail],
	);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		smartRequestSeqRef.current += 1;
		smartAutoAttemptedKeyRef.current = null;
		setTranslating(false);
		setSmartResolving(false);
		if (!normalizedTarget || !activeTargetKey) {
			pendingTranslationRequestRef.current = null;
			setDetail(null);
			setDetailTargetKey(null);
			setLoadError(null);
			setTranslateError(null);
			setSmartError(null);
			return;
		}
		loadDetail(normalizedTarget, { resetDisplay: true });
	}, [activeTargetKey, loadDetail, normalizedTarget]);

	const activeDetail = useMemo(() => {
		if (!detail || !activeTargetKey || detailTargetKey !== activeTargetKey) {
			return null;
		}
		return detail;
	}, [activeTargetKey, detail, detailTargetKey]);

	const detailTranslationError = useMemo(() => {
		if (activeDetail?.translated?.status !== "error") {
			return null;
		}
		return toUiError(activeDetail.translated, "这次翻译没有成功完成。");
	}, [activeDetail]);

	const activeTranslationError =
		selectedLane === "translated"
			? (translateError ?? detailTranslationError)
			: null;

	const detailSmartError = useMemo(() => {
		if (activeDetail?.smart?.status !== "error") {
			return null;
		}
		return toUiError(activeDetail.smart, "这次润色没有成功完成。");
	}, [activeDetail]);

	const activeSmartError =
		selectedLane === "smart" ? (smartError ?? detailSmartError) : null;

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
				setSelectedLane("translated");
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

	const onResolveSmart = useCallback(
		(options?: { selectLane?: boolean; retryOnError?: boolean }) => {
			if (!activeDetail || smartResolving) return;
			if (options?.selectLane !== false) {
				setSelectedLane("smart");
			}
			const requestSeq = smartRequestSeqRef.current + 1;
			smartRequestSeqRef.current = requestSeq;
			const requestReleaseId = activeDetail.release_id;
			setSmartResolving(true);
			setSmartError(null);
			void (async () => {
				const requestItem = buildReleaseSmartRequestItem(activeDetail);
				const deadline = Date.now() + REQUEST_STATUS_POLL_WINDOW_MS;
				let result = (
					await apiResolveTranslationResults({
						items: [requestItem],
						retry_on_error: options?.retryOnError ?? true,
					})
				).items.find((item) => item.producer_ref === requestItem.producer_ref);

				while (result && isPendingTranslationResultStatus(result.status)) {
					if (Date.now() >= deadline) {
						return;
					}
					if (smartRequestSeqRef.current !== requestSeq) return;
					await sleep(REQUEST_STATUS_POLL_INTERVAL_MS);
					if (smartRequestSeqRef.current !== requestSeq) return;
					result = (
						await apiResolveTranslationResults({
							items: [requestItem],
							retry_on_error: false,
						})
					).items.find(
						(item) => item.producer_ref === requestItem.producer_ref,
					);
				}

				if (!result) {
					throw new Error("release polish result missing");
				}
				if (smartRequestSeqRef.current !== requestSeq) return;
				const smart = mapTranslationResultToReleaseDetailSmart(result);
				if (!smart) {
					throw new Error(resolveErrorSummary(result, "润色失败"));
				}
				setDetail((prev) => {
					if (!prev) return prev;
					if (prev.release_id !== requestReleaseId) return prev;
					return { ...prev, smart };
				});
				setSmartError(
					smart.status === "error"
						? toUiError(smart, "这次润色没有成功完成。")
						: null,
				);
			})()
				.catch((error) => {
					if (smartRequestSeqRef.current !== requestSeq) return;
					setSmartError(toUnknownUiError(error, "润色失败，请稍后重试。"));
				})
				.finally(() => {
					if (smartRequestSeqRef.current !== requestSeq) return;
					setSmartResolving(false);
				});
		},
		[activeDetail, smartResolving],
	);

	useEffect(() => {
		if (!activeDetail || selectedLane !== "smart" || smartResolving) return;
		if (!shouldResolveSmart(activeDetail.smart)) return;
		const autoAttemptKey = activeDetail.release_id;
		if (smartAutoAttemptedKeyRef.current === autoAttemptKey) return;
		smartAutoAttemptedKeyRef.current = autoAttemptKey;
		onResolveSmart({ selectLane: false, retryOnError: true });
	}, [activeDetail, onResolveSmart, selectedLane, smartResolving]);

	useEffect(() => {
		if (!activeDetail) return;
		if (selectedLane === "smart" && activeDetail.smart?.status === "disabled") {
			setSelectedLane("original");
		}
		if (
			selectedLane === "translated" &&
			activeDetail.translated?.status === "disabled"
		) {
			setSelectedLane("original");
		}
	}, [activeDetail, selectedLane]);

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
		const smartTitle =
			activeDetail.smart?.status === "ready" ? activeDetail.smart.title : null;
		const title =
			selectedLane === "translated"
				? translatedTitle?.trim() || originalTitle
				: selectedLane === "smart"
					? smartTitle?.trim() || originalTitle
					: originalTitle;

		const translatedBody =
			activeDetail.translated?.status === "ready"
				? activeDetail.translated.summary
				: null;
		const smartBody =
			activeDetail.smart?.status === "ready"
				? activeDetail.smart.summary
				: null;
		const originalBody = activeDetail.body?.trim() ? activeDetail.body : null;
		const body =
			selectedLane === "translated"
				? translatedBody?.trim()
					? translatedBody
					: originalBody
				: selectedLane === "smart"
					? smartBody?.trim()
						? smartBody
						: originalBody
					: originalBody;

		return { title, body };
	}, [activeDetail, selectedLane]);

	if (!normalizedTarget || !activeTargetKey) {
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
								{loading
									? "加载中…"
									: (releaseTargetDescription(normalizedTarget) ?? "Release")}
								{activeDetail?.published_at
									? ` · ${formatIsoShortLocal(activeDetail.published_at)}`
									: ""}
							</DialogDescription>
						</div>

						<div className="flex shrink-0 flex-wrap items-center gap-2">
							<div
								role="tablist"
								aria-label="Release 详情阅读模式"
								className="flex rounded-full border border-border/55 bg-muted/35 p-0.5 shadow-sm"
							>
								{RELEASE_DETAIL_LANES.map((option) => {
									const isSelected = selectedLane === option.lane;
									const isBusy =
										(option.lane === "translated" && translating) ||
										(option.lane === "smart" && smartResolving);
									const wasSelected = selectedLane === option.lane;
									const isDisabled =
										loading ||
										!activeDetail ||
										(option.lane === "translated" &&
											activeDetail.translated?.status === "disabled") ||
										(option.lane === "smart" &&
											activeDetail.smart?.status === "disabled");
									const Icon = isBusy ? RefreshCcw : option.icon;
									return (
										<Button
											key={option.lane}
											type="button"
											role="tab"
											aria-selected={isSelected}
											variant="ghost"
											size="sm"
											className={cn(
												"h-8 rounded-full px-3 font-mono text-xs",
												isSelected &&
													"bg-background text-foreground shadow-sm hover:bg-background",
												isBusy && "text-foreground",
											)}
											onClick={() => {
												setSelectedLane(option.lane);
												if (
													option.lane === "translated" &&
													(wasSelected ||
														activeDetail?.translated?.status !== "ready") &&
													activeDetail?.translated?.status !== "disabled"
												) {
													onTranslate();
												}
												if (
													option.lane === "smart" &&
													activeDetail &&
													shouldResolveSmart(activeDetail.smart)
												) {
													onResolveSmart({ retryOnError: true });
												}
											}}
											disabled={isDisabled}
											aria-busy={isBusy ? "true" : undefined}
										>
											<Icon
												className={cn("size-4", isBusy && "animate-spin")}
											/>
											{isBusy ? `${option.label}中…` : option.label}
										</Button>
									);
								})}
							</div>

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
											loadDetail(normalizedTarget, { resetDisplay: true })
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
											onClick={() => setSelectedLane("original")}
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
						) : activeSmartError ? (
							<ErrorStatePanel
								title="润色失败"
								summary={activeSmartError.summary}
								detail={activeSmartError.detail}
								actions={
									<div className="flex flex-wrap gap-2">
										<Button
											variant="outline"
											size="sm"
											className="font-mono text-xs"
											onClick={() =>
												onResolveSmart({ selectLane: true, retryOnError: true })
											}
											disabled={smartResolving}
										>
											<RefreshCcw
												className={cn(
													"size-4",
													smartResolving && "animate-spin",
												)}
											/>
											{smartResolving ? "润色中…" : "重试润色"}
										</Button>
										<Button
											variant="ghost"
											size="sm"
											className="font-mono text-xs"
											onClick={() => setSelectedLane("original")}
										>
											<FileText className="size-4" />
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
