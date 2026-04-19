import { useCallback, useEffect, useRef, useState } from "react";

import {
	ApiError,
	apiCheckReactionToken,
	apiGetReactionTokenStatus,
	apiPutReactionToken,
	type ReactionTokenStatusResponse,
} from "@/api";

export type PatCheckState = "idle" | "checking" | "valid" | "invalid" | "error";

export const PAT_CREATE_PATH =
	"Settings → Developer settings → Personal access tokens → Tokens (classic)";

export function sessionExpiredHint() {
	return `当前页面（${window.location.origin}）的 OctoRill 登录已失效（不是 PAT 本身）。请先点右上角 Logout，再重新 Login with GitHub；若同时开了多个本地实例，请只保留这个端口。`;
}

export function normalizePatStatus(
	status: ReactionTokenStatusResponse["check"]["state"],
	message: string | null,
) {
	if (status === "valid") {
		return {
			status,
			message: message === "token is valid" || !message ? "PAT 可用" : message,
		};
	}
	if (status === "invalid") {
		return {
			status,
			message: message || "PAT 无效或已过期，请重新填写并校验。",
		};
	}
	if (status === "error") {
		return {
			status,
			message: message || "PAT 校验失败，请稍后重试。",
		};
	}
	return {
		status: "idle" as const,
		message,
	};
}

export function isReactionTokenUsable(status: ReactionTokenStatusResponse) {
	return status.configured && status.check.state === "valid";
}

type UseReactionTokenEditorOptions = {
	autoLoad?: boolean;
	onStatusLoaded?: (status: ReactionTokenStatusResponse) => void;
	onPatSaved?: (status: ReactionTokenStatusResponse) => void;
};

export function useReactionTokenEditor(
	options: UseReactionTokenEditorOptions = {},
) {
	const { autoLoad = true, onStatusLoaded, onPatSaved } = options;
	const [reactionTokenLoading, setReactionTokenLoading] = useState(autoLoad);
	const [reactionTokenConfigured, setReactionTokenConfigured] = useState(false);
	const [reactionTokenMasked, setReactionTokenMasked] = useState<string | null>(
		null,
	);
	const [patInput, setPatInput] = useState("");
	const [patCheckState, setPatCheckState] = useState<PatCheckState>("idle");
	const [patCheckMessage, setPatCheckMessage] = useState<string | null>(null);
	const [patCheckedAt, setPatCheckedAt] = useState<string | null>(null);
	const [patSaving, setPatSaving] = useState(false);
	const patCheckSeqRef = useRef(0);
	const patInputRef = useRef("");

	useEffect(() => {
		patInputRef.current = patInput;
	}, [patInput]);

	const loadReactionToken = useCallback(async () => {
		setReactionTokenLoading(true);
		try {
			const res = await apiGetReactionTokenStatus();
			const normalized = normalizePatStatus(res.check.state, res.check.message);
			const hasDraft = patInputRef.current.trim().length > 0;
			setReactionTokenConfigured(res.configured);
			setReactionTokenMasked(res.masked_token);
			if (!hasDraft) {
				setPatCheckState(normalized.status);
				setPatCheckMessage(normalized.message);
				setPatCheckedAt(res.check.checked_at);
			}
			onStatusLoaded?.(res);
			return res;
		} catch (err) {
			if (patInputRef.current.trim().length > 0) {
				return null;
			}
			const message = err instanceof Error ? err.message : String(err);
			setPatCheckState("error");
			setPatCheckMessage(message);
			return null;
		} finally {
			setReactionTokenLoading(false);
		}
	}, [onStatusLoaded]);

	useEffect(() => {
		if (!autoLoad) return;
		void loadReactionToken();
	}, [autoLoad, loadReactionToken]);

	useEffect(() => {
		patCheckSeqRef.current += 1;
		const seq = patCheckSeqRef.current;
		const token = patInput.trim();
		if (!token) {
			if (reactionTokenConfigured) {
				setPatCheckState("idle");
				return;
			}
			setPatCheckState("idle");
			setPatCheckMessage(null);
			setPatCheckedAt(null);
			return;
		}

		setPatCheckState("checking");
		setPatCheckMessage("正在检查 PAT 可用性…");
		const timer = window.setTimeout(() => {
			void apiCheckReactionToken(token)
				.then((res) => {
					if (seq !== patCheckSeqRef.current) return;
					const normalized = normalizePatStatus(res.state, res.message);
					setPatCheckState(normalized.status);
					setPatCheckMessage(normalized.message);
					setPatCheckedAt(new Date().toISOString());
				})
				.catch((err) => {
					if (seq !== patCheckSeqRef.current) return;
					if (err instanceof ApiError && err.status === 401) {
						setPatCheckState("invalid");
						setPatCheckMessage(sessionExpiredHint());
						return;
					}
					setPatCheckState("error");
					setPatCheckMessage(err instanceof Error ? err.message : String(err));
				});
		}, 800);

		return () => window.clearTimeout(timer);
	}, [patInput, reactionTokenConfigured]);

	const clearPatDraft = useCallback(() => {
		patCheckSeqRef.current += 1;
		setPatInput("");
		setPatCheckState("idle");
		setPatCheckMessage(null);
		setPatCheckedAt(null);
	}, []);

	const savePat = useCallback(async () => {
		const token = patInput.trim();
		if (!token || patCheckState !== "valid") return null;
		setPatSaving(true);
		try {
			const res = await apiPutReactionToken(token);
			const normalized = normalizePatStatus(res.check.state, res.check.message);
			setReactionTokenConfigured(res.configured);
			setReactionTokenMasked(res.masked_token);
			setPatCheckState(normalized.status);
			setPatCheckMessage(normalized.message);
			setPatCheckedAt(res.check.checked_at);
			setPatInput("");
			onPatSaved?.(res);
			return res;
		} catch (err) {
			if (err instanceof ApiError && err.status === 401) {
				setPatCheckState("invalid");
				setPatCheckMessage(sessionExpiredHint());
				return null;
			}
			setPatCheckState("error");
			setPatCheckMessage(err instanceof Error ? err.message : String(err));
			return null;
		} finally {
			setPatSaving(false);
		}
	}, [onPatSaved, patCheckState, patInput]);

	return {
		reactionTokenLoading,
		reactionTokenConfigured,
		reactionTokenMasked,
		patInput,
		setPatInput,
		patCheckState,
		patCheckMessage,
		patCheckedAt,
		patSaving,
		canSavePat: patInput.trim().length > 0 && patCheckState === "valid",
		loadReactionToken,
		savePat,
		clearPatDraft,
	};
}
