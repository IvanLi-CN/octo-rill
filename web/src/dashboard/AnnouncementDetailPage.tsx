import {
	ArrowLeft,
	ArrowUpRight,
	FileText,
	Languages,
	RefreshCcw,
	Sparkles,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import {
	type AnnouncementDetailResponse,
	type TranslationRequestResponse,
	ApiError,
	apiGetAnnouncementDetail,
	apiGetTranslationRequest,
	apiResolveTranslationResults,
	apiTranslateAnnouncementDetail,
	isPendingTranslationResultStatus,
	mapTranslationResultToAnnouncementDetailSmart,
	mapTranslationResultToAnnouncementDetailTranslated,
} from "@/api";
import { useAppToast } from "@/components/feedback/AppToast";
import { ErrorStatePanel } from "@/components/feedback/ErrorStatePanel";
import { Markdown } from "@/components/Markdown";
import { RepoIdentity } from "@/components/repo/RepoIdentity";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { formatIsoShortLocal } from "@/lib/datetime";
import {
	describeUnknownError,
	resolveErrorDetail,
	resolveErrorSummary,
} from "@/lib/errorPresentation";

const REQUEST_STATUS_POLL_INTERVAL_MS = 600;
const REQUEST_STATUS_POLL_WINDOW_MS = 20_000;
const REQUEST_NOT_FOUND_ERROR_CODE = "not_found";
const SMART_RESOLVE_MAX_WAIT_MS = 5_000;

type AnnouncementDetailUiError = {
	summary: string;
	detail?: string | null;
};

type AnnouncementDetailLane = "original" | "translated" | "smart";

const ANNOUNCEMENT_DETAIL_LANES: Array<{
	lane: AnnouncementDetailLane;
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
): AnnouncementDetailUiError {
	return {
		summary: resolveErrorSummary(value, fallback),
		detail: resolveErrorDetail(value),
	};
}

function toUnknownUiError(
	error: unknown,
	fallback: string,
): AnnouncementDetailUiError {
	return {
		summary: describeUnknownError(error, fallback),
		detail: error instanceof Error ? error.message : null,
	};
}

function hasReadyTranslatedContent(
	translated: AnnouncementDetailResponse["translated"] | null | undefined,
) {
	if (translated?.status !== "ready") {
		return false;
	}
	return Boolean(translated.title?.trim() || translated.summary?.trim());
}

function shouldResolveSmart(
	smart: AnnouncementDetailResponse["smart"] | null | undefined,
) {
	return (
		!smart ||
		(smart.status === "missing" && smart.auto_translate !== false) ||
		(smart.status === "error" && smart.auto_translate !== false)
	);
}

function buildAnnouncementSmartRequestItem(detail: AnnouncementDetailResponse) {
	const title = detail.title.trim() || `discussion:${detail.discussion_number}`;
	const body = detail.body?.trim();
	const metadata = [
		detail.repo_full_name,
		`discussion_number=${detail.discussion_number}`,
	]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: `feed.smart:announcement:${detail.discussion_key}`,
		kind: "announcement_smart" as const,
		variant: "feed_card",
		entity_id: detail.discussion_key,
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

export function AnnouncementDetailPage(props: {
	owner: string;
	repo: string;
	number: string;
	onBack: () => void;
}) {
	const { owner, repo, number, onBack } = props;
	const { pushErrorToast } = useAppToast();
	const [loading, setLoading] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [smartResolving, setSmartResolving] = useState(false);
	const [loadError, setLoadError] = useState<AnnouncementDetailUiError | null>(
		null,
	);
	const [translateError, setTranslateError] =
		useState<AnnouncementDetailUiError | null>(null);
	const [smartError, setSmartError] =
		useState<AnnouncementDetailUiError | null>(null);
	const [selectedLane, setSelectedLane] =
		useState<AnnouncementDetailLane>("smart");
	const [detail, setDetail] = useState<AnnouncementDetailResponse | null>(null);
	const translateRequestSeqRef = useRef(0);
	const smartRequestSeqRef = useRef(0);
	const loadRequestSeqRef = useRef(0);
	const smartAutoAttemptedKeyRef = useRef<string | null>(null);
	const pendingTranslationRequestRef = useRef<{
		discussionKey: string;
		requestId: string;
	} | null>(null);

	const loadDetail = useCallback(() => {
		const requestSeq = loadRequestSeqRef.current + 1;
		loadRequestSeqRef.current = requestSeq;
		pendingTranslationRequestRef.current = null;
		setLoading(true);
		setLoadError(null);
		setTranslateError(null);
		setSmartError(null);
		setSelectedLane("smart");
		setDetail(null);
		void apiGetAnnouncementDetail({ owner, repo, number })
			.then((response) => {
				if (loadRequestSeqRef.current !== requestSeq) return;
				setDetail(response);
			})
			.catch((error) => {
				if (loadRequestSeqRef.current !== requestSeq) return;
				setLoadError(toUnknownUiError(error, "公告详情加载失败，请稍后重试。"));
			})
			.finally(() => {
				if (loadRequestSeqRef.current !== requestSeq) return;
				setLoading(false);
			});
	}, [number, owner, repo]);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		smartRequestSeqRef.current += 1;
		smartAutoAttemptedKeyRef.current = null;
		setTranslating(false);
		setSmartResolving(false);
		loadDetail();
	}, [loadDetail]);

	const detailTranslationError = useMemo(() => {
		if (detail?.translated?.status !== "error") {
			return null;
		}
		return toUiError(detail.translated, "这次翻译没有成功完成。");
	}, [detail]);

	const activeTranslationError =
		selectedLane === "translated"
			? (translateError ?? detailTranslationError)
			: null;

	const detailSmartError = useMemo(() => {
		if (detail?.smart?.status !== "error") {
			return null;
		}
		return toUiError(detail.smart, "这次润色没有成功完成。");
	}, [detail]);

	const activeSmartError =
		selectedLane === "smart" ? (smartError ?? detailSmartError) : null;

	const onTranslate = useCallback(() => {
		if (!detail || translating) return;
		const requestSeq = translateRequestSeqRef.current + 1;
		translateRequestSeqRef.current = requestSeq;
		const requestDiscussionKey = detail.discussion_key;
		const preserveReadyTranslation = hasReadyTranslatedContent(
			detail.translated,
		);
		setTranslating(true);
		setTranslateError(null);
		void (async () => {
			let requestId =
				pendingTranslationRequestRef.current?.discussionKey ===
				requestDiscussionKey
					? pendingTranslationRequestRef.current.requestId
					: null;
			let response: TranslationRequestResponse | null = null;
			for (let attempt = 0; attempt < 2; attempt += 1) {
				try {
					response = requestId
						? await apiGetTranslationRequest(requestId)
						: await apiTranslateAnnouncementDetail(detail);
					const deadline = Date.now() + REQUEST_STATUS_POLL_WINDOW_MS;
					while (isPendingTranslationResultStatus(response.result.status)) {
						pendingTranslationRequestRef.current = {
							discussionKey: requestDiscussionKey,
							requestId: response.request_id,
						};
						if (Date.now() >= deadline) {
							return;
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
			const translated = mapTranslationResultToAnnouncementDetailTranslated(
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
				if (prev.discussion_key !== requestDiscussionKey) return prev;
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
	}, [detail, pushErrorToast, translating]);

	const onResolveSmart = useCallback(
		(options?: { selectLane?: boolean; retryOnError?: boolean }) => {
			if (!detail || smartResolving) return;
			if (options?.selectLane !== false) {
				setSelectedLane("smart");
			}
			const requestSeq = smartRequestSeqRef.current + 1;
			smartRequestSeqRef.current = requestSeq;
			const requestDiscussionKey = detail.discussion_key;
			setSmartResolving(true);
			setSmartError(null);
			void (async () => {
				const requestItem = buildAnnouncementSmartRequestItem(detail);
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
					throw new Error("announcement polish result missing");
				}
				if (smartRequestSeqRef.current !== requestSeq) return;
				const smart = mapTranslationResultToAnnouncementDetailSmart(result);
				if (!smart) {
					throw new Error(resolveErrorSummary(result, "润色失败"));
				}
				setDetail((prev) => {
					if (!prev) return prev;
					if (prev.discussion_key !== requestDiscussionKey) return prev;
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
		[detail, smartResolving],
	);

	useEffect(() => {
		if (!detail || selectedLane !== "smart" || smartResolving) return;
		if (!shouldResolveSmart(detail.smart)) return;
		const autoAttemptKey = detail.discussion_key;
		if (smartAutoAttemptedKeyRef.current === autoAttemptKey) return;
		smartAutoAttemptedKeyRef.current = autoAttemptKey;
		onResolveSmart({ selectLane: false, retryOnError: true });
	}, [detail, onResolveSmart, selectedLane, smartResolving]);

	useEffect(() => {
		if (!detail) return;
		if (selectedLane === "smart" && detail.smart?.status === "disabled") {
			setSelectedLane("original");
		}
		if (
			selectedLane === "translated" &&
			detail.translated?.status === "disabled"
		) {
			setSelectedLane("original");
		}
	}, [detail, selectedLane]);

	const display = useMemo(() => {
		if (!detail) return null;
		const originalTitle = detail.title;
		const translatedTitle =
			detail.translated?.status === "ready" ? detail.translated.title : null;
		const smartTitle =
			detail.smart?.status === "ready" ? detail.smart.title : null;
		const title =
			selectedLane === "translated"
				? translatedTitle?.trim() || originalTitle
				: selectedLane === "smart"
					? smartTitle?.trim() || originalTitle
					: originalTitle;
		const translatedBody =
			detail.translated?.status === "ready" ? detail.translated.summary : null;
		const smartBody =
			detail.smart?.status === "ready" ? detail.smart.summary : null;
		const originalBody = detail.body?.trim() ? detail.body : null;
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
	}, [detail, selectedLane]);

	return (
		<div className="space-y-4">
			<div className="flex flex-col gap-4 rounded-[28px] border bg-card/80 p-5 shadow-sm sm:p-6">
				<div className="flex flex-col gap-4 lg:flex-row lg:items-start lg:justify-between">
					<div className="min-w-0 space-y-3">
						<Button
							type="button"
							variant="ghost"
							size="sm"
							className="w-fit rounded-full px-3 font-mono text-xs"
							onClick={onBack}
						>
							<ArrowLeft className="size-4" />
							返回工作台
						</Button>
						{detail ? (
							<RepoIdentity
								repoFullName={detail.repo_full_name}
								repoVisual={detail.repo_visual}
								className="min-h-8 w-full min-w-0"
								labelClassName="font-mono text-base font-medium tracking-tight text-foreground/80"
								visualClassName="size-8"
							>
								<span className="block truncate font-mono text-[11px] text-muted-foreground sm:text-xs">
									{detail.occurred_at
										? formatIsoShortLocal(detail.occurred_at)
										: `Discussion #${detail.discussion_number}`}
									{detail.actor?.login ? ` · by ${detail.actor.login}` : ""}
								</span>
							</RepoIdentity>
						) : null}
					</div>

					<div className="flex flex-wrap items-center gap-2">
						<div
							role="tablist"
							aria-label="公告详情阅读模式"
							className="flex rounded-full border border-border/55 bg-muted/35 p-0.5 shadow-sm"
						>
							{ANNOUNCEMENT_DETAIL_LANES.map((option) => {
								const isSelected = selectedLane === option.lane;
								const isBusy =
									(option.lane === "translated" && translating) ||
									(option.lane === "smart" && smartResolving);
								const wasSelected = selectedLane === option.lane;
								const isDisabled =
									loading ||
									!detail ||
									(option.lane === "translated" &&
										detail.translated?.status === "disabled") ||
									(option.lane === "smart" &&
										detail.smart?.status === "disabled");
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
													detail?.translated?.status !== "ready") &&
												detail?.translated?.status !== "disabled"
											) {
												onTranslate();
											}
											if (
												option.lane === "smart" &&
												detail &&
												shouldResolveSmart(detail.smart)
											) {
												onResolveSmart({ retryOnError: true });
											}
										}}
										disabled={isDisabled}
										aria-busy={isBusy ? "true" : undefined}
									>
										<Icon className={cn("size-4", isBusy && "animate-spin")} />
										{isBusy ? `${option.label}中…` : option.label}
									</Button>
								);
							})}
						</div>
						{detail?.html_url ? (
							<Button
								asChild
								variant="outline"
								size="sm"
								className="font-mono text-xs"
							>
								<a href={detail.html_url} target="_blank" rel="noreferrer">
									<ArrowUpRight className="size-4" />
									GitHub
								</a>
							</Button>
						) : null}
					</div>
				</div>
			</div>

			<div className="rounded-[28px] border bg-card/80 p-5 shadow-sm sm:p-6">
				{loadError ? (
					<ErrorStatePanel
						title="公告详情加载失败"
						summary={loadError.summary}
						detail={loadError.detail}
						actions={
							<div className="flex flex-wrap gap-2">
								<Button
									variant="outline"
									size="sm"
									className="font-mono text-xs"
									onClick={loadDetail}
								>
									<RefreshCcw className="size-4" />
									重试
								</Button>
							</div>
						}
					/>
				) : loading ? (
					<p className="text-sm text-muted-foreground">正在加载公告详情…</p>
				) : detail ? (
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
											onResolveSmart({
												selectLane: true,
												retryOnError: true,
											})
										}
										disabled={smartResolving}
									>
										<RefreshCcw
											className={cn("size-4", smartResolving && "animate-spin")}
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
								</div>
							}
						/>
					) : display ? (
						<div className="space-y-4">
							<h1 className="text-balance text-[1.5rem] font-semibold leading-tight tracking-tight sm:text-[1.75rem]">
								{display.title}
							</h1>
							{display.body ? (
								<div className="rounded-2xl border bg-muted/10 p-4 sm:p-5">
									<Markdown content={display.body} />
								</div>
							) : (
								<p className="text-sm text-muted-foreground">该公告无正文。</p>
							)}
						</div>
					) : (
						<p className="text-sm text-muted-foreground">未找到该公告。</p>
					)
				) : (
					<p className="text-sm text-muted-foreground">未找到该公告。</p>
				)}
			</div>
		</div>
	);
}
