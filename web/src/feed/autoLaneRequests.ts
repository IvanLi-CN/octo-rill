import type { TranslationRequestItemInput } from "../api";
import {
	isAnnouncementFeedItem,
	isLaneCapableFeedItem,
	type FeedItem,
	type SmartItem,
	type TranslatedItem,
} from "./types";

const AUTO_LANE_MAX_WAIT_MS = 500;

function buildReleaseSummaryRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `release:${item.id}`;
	const body = item.body?.trim();
	const metadata = [item.repo_full_name, item.reason, item.subject_type]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: `feed.auto_translate:release:${item.id}`,
		kind: "release_summary",
		variant: "feed_body",
		entity_id: item.id,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_LANE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "body_md"],
	};
}

function buildAnnouncementSummaryRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `announcement:${item.id}`;
	const body = item.body?.trim();
	const discussionNumber =
		isAnnouncementFeedItem(item) && item.discussion_number
			? `discussion_number=${item.discussion_number}`
			: null;
	const metadata = [item.repo_full_name, discussionNumber]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	const entityId =
		isAnnouncementFeedItem(item) && item.discussion_key
			? item.discussion_key
			: item.id;
	return {
		producer_ref: `feed.auto_translate:announcement:${entityId}`,
		kind: "announcement_summary",
		variant: "feed_body",
		entity_id: entityId,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_LANE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "body_md"],
	};
}

export function buildSummaryRequestItem(item: FeedItem) {
	return isAnnouncementFeedItem(item)
		? buildAnnouncementSummaryRequestItem(item)
		: buildReleaseSummaryRequestItem(item);
}

function buildReleaseSmartRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `release:${item.id}`;
	const body = item.body?.trim();
	const metadata = [item.repo_full_name, title]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	return {
		producer_ref: `feed.smart:release:${item.id}`,
		kind: "release_smart",
		variant: "feed_card",
		entity_id: item.id,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_LANE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "body_md"],
	};
}

function buildAnnouncementSmartRequestItem(
	item: FeedItem,
): TranslationRequestItemInput {
	const title = item.title?.trim() || `announcement:${item.id}`;
	const body = item.body?.trim();
	const discussionNumber =
		isAnnouncementFeedItem(item) && item.discussion_number
			? `discussion_number=${item.discussion_number}`
			: null;
	const metadata = [item.repo_full_name, discussionNumber]
		.filter((value): value is string => Boolean(value?.trim()))
		.join("\n");
	const entityId =
		isAnnouncementFeedItem(item) && item.discussion_key
			? item.discussion_key
			: item.id;
	return {
		producer_ref: `feed.smart:announcement:${entityId}`,
		kind: "announcement_smart",
		variant: "feed_card",
		entity_id: entityId,
		target_lang: "zh-CN",
		max_wait_ms: AUTO_LANE_MAX_WAIT_MS,
		source_blocks: [
			{ slot: "title", text: title },
			...(body ? [{ slot: "body_markdown" as const, text: body }] : []),
			...(metadata ? [{ slot: "metadata" as const, text: metadata }] : []),
		],
		target_slots: ["title_zh", "body_md"],
	};
}

export function buildSmartRequestItem(item: FeedItem) {
	return isAnnouncementFeedItem(item)
		? buildAnnouncementSmartRequestItem(item)
		: buildReleaseSmartRequestItem(item);
}

export function buildRequestSourceKey(item: TranslationRequestItemInput) {
	return JSON.stringify({
		producer_ref: item.producer_ref,
		kind: item.kind,
		variant: item.variant,
		entity_id: item.entity_id,
		target_lang: item.target_lang,
		source_blocks: item.source_blocks,
		target_slots: item.target_slots,
	});
}

export function buildTranslationSourceKey(item: FeedItem) {
	return buildRequestSourceKey(buildSummaryRequestItem(item));
}

export function buildSmartSourceKey(item: FeedItem) {
	return buildRequestSourceKey(buildSmartRequestItem(item));
}

export function applyTranslationIfSourceMatches(
	item: FeedItem,
	sourceKey: string,
	translated: TranslatedItem,
): FeedItem {
	if (
		!isLaneCapableFeedItem(item) ||
		buildTranslationSourceKey(item) !== sourceKey
	) {
		return item;
	}
	return { ...item, translated: { ...translated } };
}

export function applySmartIfSourceMatches(
	item: FeedItem,
	sourceKey: string,
	smart: SmartItem,
): FeedItem {
	if (!isLaneCapableFeedItem(item) || buildSmartSourceKey(item) !== sourceKey) {
		return item;
	}
	return { ...item, smart: { ...smart } };
}
