import { ArrowUpRight, Languages, RefreshCcw } from "lucide-react";
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

function formatIsoShort(iso: string | null) {
	if (!iso) return null;
	const noZ = iso.replace("Z", "");
	const noFrac = noZ.includes(".") ? noZ.split(".")[0] : noZ;
	return noFrac.replace("T", " ");
}

export function ReleaseDetailCard(props: { releaseId: string | null }) {
	const { releaseId } = props;
	const [loading, setLoading] = useState(false);
	const [translating, setTranslating] = useState(false);
	const [error, setError] = useState<string | null>(null);
	const [showOriginal, setShowOriginal] = useState(false);
	const [detail, setDetail] = useState<ReleaseDetailResponse | null>(null);
	const translateRequestSeqRef = useRef(0);

	useEffect(() => {
		translateRequestSeqRef.current += 1;
		setTranslating(false);
		if (!releaseId) {
			setDetail(null);
			setError(null);
			return;
		}
		let active = true;
		setLoading(true);
		setError(null);
		setShowOriginal(false);
		void apiGetReleaseDetail(releaseId)
			.then((res) => {
				if (!active) return;
				setDetail(res);
			})
			.catch((err) => {
				if (!active) return;
				setError(err instanceof Error ? err.message : String(err));
				setDetail(null);
			})
			.finally(() => {
				if (!active) return;
				setLoading(false);
			});

		return () => {
			active = false;
		};
	}, [releaseId]);

	const onTranslate = useCallback(() => {
		if (!detail || translating) return;
		const requestSeq = translateRequestSeqRef.current + 1;
		translateRequestSeqRef.current = requestSeq;
		const requestReleaseId = detail.release_id;
		setTranslating(true);
		setError(null);
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
				setError(err instanceof Error ? err.message : String(err));
			})
			.finally(() => {
				if (translateRequestSeqRef.current !== requestSeq) return;
				setTranslating(false);
			});
	}, [detail, translating]);

	const display = useMemo(() => {
		if (!detail) return null;
		const originalTitle = detail.name ?? detail.tag_name;
		const translatedTitle =
			detail.translated?.status === "ready" ? detail.translated.title : null;
		const title = showOriginal
			? originalTitle
			: translatedTitle?.trim() || originalTitle;

		const translatedBody =
			detail.translated?.status === "ready" ? detail.translated.summary : null;
		const originalBody = detail.body?.trim() ? detail.body : null;
		const body = showOriginal
			? originalBody
			: translatedBody?.trim()
				? translatedBody
				: originalBody;

		return { title, body };
	}, [detail, showOriginal]);

	if (!releaseId) {
		return null;
	}

	return (
		<Card className="bg-card/80 shadow-sm">
			<CardHeader>
				<div className="flex items-start justify-between gap-3">
					<div className="min-w-0">
						<CardTitle className="text-base">Release 详情</CardTitle>
						<CardDescription className="font-mono text-xs">
							{loading ? "加载中…" : `#${releaseId}`}
							{detail?.published_at
								? ` · ${formatIsoShort(detail.published_at)}`
								: ""}
						</CardDescription>
					</div>

					<div className="flex shrink-0 items-center gap-2">
						{detail?.translated?.status === "disabled" ? (
							<span className="text-muted-foreground font-mono text-[11px]">
								AI 未配置
							</span>
						) : (
							<>
								<Button
									variant="ghost"
									size="sm"
									className="font-mono text-xs"
									onClick={() => setShowOriginal((v) => !v)}
									disabled={loading || !detail}
								>
									<Languages className="size-4" />
									{showOriginal ? "中文" : "原文"}
								</Button>
								<Button
									variant="ghost"
									size="sm"
									className="font-mono text-xs"
									onClick={onTranslate}
									disabled={loading || !detail || translating}
								>
									<RefreshCcw className="size-4" />
									{translating ? "翻译中…" : "翻译"}
								</Button>
							</>
						)}

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
			</CardHeader>

			<CardContent className="pt-0">
				{error ? (
					<p className="text-destructive text-sm">{error}</p>
				) : loading ? (
					<p className="text-muted-foreground text-sm">
						正在加载 release 详情…
					</p>
				) : display ? (
					<div className="space-y-3">
						<h3 className="text-sm font-semibold tracking-tight">
							{display.title}
						</h3>
						{detail?.repo_full_name ? (
							<p className="text-muted-foreground font-mono text-xs">
								{detail.repo_full_name}
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
