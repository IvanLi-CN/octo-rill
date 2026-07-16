export type RichClipboardPayload = {
	html: string;
	text: string;
	markdown?: string | null;
};

function normalizeClipboardText(raw: string) {
	return raw
		.replace(/\r\n/g, "\n")
		.replace(/\n{3,}/g, "\n\n")
		.trim();
}

export function buildRichClipboardPayload(
	root: HTMLElement,
	options?: {
		markdown?: string | null;
	},
): RichClipboardPayload {
	const html = root.innerHTML.trim();
	const text = normalizeClipboardText(root.innerText || root.textContent || "");
	return {
		html,
		text,
		markdown: options?.markdown?.trim() || null,
	};
}

function createClipboardBlob(type: string, value: string) {
	return new Blob([value], { type });
}

export async function writeRichClipboard(payload: RichClipboardPayload) {
	if (typeof navigator === "undefined" || !navigator.clipboard) {
		throw new Error("当前环境不支持剪贴板复制。");
	}

	const clipboard = navigator.clipboard;
	const markdown = payload.markdown?.trim() || null;
	const fallbackText = markdown || payload.text;

	if (
		typeof clipboard.write === "function" &&
		typeof ClipboardItem !== "undefined"
	) {
		const richItems: Record<string, Blob> = {
			"text/html": createClipboardBlob("text/html", payload.html),
			"text/plain": createClipboardBlob("text/plain", payload.text),
		};
		if (markdown) {
			richItems["text/markdown"] = createClipboardBlob(
				"text/markdown",
				markdown,
			);
		}
		try {
			await clipboard.write([new ClipboardItem(richItems)]);
			return;
		} catch (error) {
			if (!markdown) {
				throw error;
			}
			await clipboard.write([
				new ClipboardItem({
					"text/html": createClipboardBlob("text/html", payload.html),
					"text/plain": createClipboardBlob("text/plain", payload.text),
				}),
			]);
			return;
		}
	}

	if (typeof clipboard.writeText === "function") {
		await clipboard.writeText(fallbackText);
		return;
	}

	throw new Error("当前环境不支持剪贴板复制。");
}
