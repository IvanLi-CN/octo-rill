import { createServer } from "vite";
import type { FeedItem, ReleaseFeedItem } from "../src/feed/types";

function assert(condition: unknown, message: string): asserts condition {
	if (!condition) {
		throw new Error(message);
	}
}

function release(id: string, ts: string): ReleaseFeedItem {
	return {
		kind: "release",
		ts,
		id,
		repo_full_name: "IvanLi-CN/dockrev",
		repo_visual: null,
		title: id,
		body: null,
		body_truncated: false,
		subtitle: null,
		reason: null,
		subject_type: null,
		html_url: `https://github.com/IvanLi-CN/dockrev/releases/tag/${id}`,
		unread: null,
		actor: null,
		translated: null,
		smart: null,
		reactions: null,
	};
}

const server = await createServer({
	configFile: new URL("../vite.config.ts", import.meta.url).pathname,
	server: { middlewareMode: true },
});

try {
	const { groupFeedItemsByDay } = await server.ssrLoadModule(
		"/src/feed/dayGroups.ts",
	);
	const earlyMorningRawRelease = release("318080539", "2026-05-05T21:55:09Z");
	const groups = groupFeedItemsByDay(
		[earlyMorningRawRelease],
		"00:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
		true,
	);

	assert(groups.length === 1, "expected one raw group");
	assert(groups[0].kind === "raw", "expected raw group without brief");
	assert(
		groups[0].id === "2026-05-06@00:00",
		"raw group id should stay natural-day based",
	);
	assert(
		groups[0].displayDate === "2026-05-06",
		"raw group should display the natural day",
	);
	assert(
		groups[0].briefDate === "2026-05-06",
		"generated brief API date should match the natural day being reviewed",
	);

	const historicalGroups = groupFeedItemsByDay(
		[earlyMorningRawRelease],
		"00:00",
		"Asia/Shanghai",
		480,
		[
			{
				id: "brief-2026-05-06",
				date: "2026-05-06",
				window_start: "2026-05-05T16:00:00+00:00",
				window_end: "2026-05-06T16:00:00+00:00",
				release_ids: [earlyMorningRawRelease.id],
			},
		],
		new Date("2026-05-07T12:00:00+08:00"),
		true,
	);

	assert(historicalGroups.length === 1, "expected one historical group");
	assert(
		historicalGroups[0].kind === "historical",
		"expected release to fold into its brief",
	);
	assert(
		historicalGroups[0].displayDate === "2026-05-06",
		"historical group should display the reviewed natural day",
	);
	assert(
		historicalGroups[0].briefId === "brief-2026-05-06",
		"historical group should preserve brief id",
	);

	const mixedItems: FeedItem[] = [
		release("318101716", "2026-05-05T23:24:41Z"),
		earlyMorningRawRelease,
	];
	const mixedGroups = groupFeedItemsByDay(
		mixedItems,
		"00:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
		true,
	);

	assert(
		mixedGroups.length === 1,
		"expected early morning releases to stay in the same window group",
	);
	assert(
		mixedGroups[0].displayDate === "2026-05-06",
		"mixed raw group should display May 6",
	);
	assert(
		mixedGroups[0].releaseCount === 2,
		"expected two releases in the raw group",
	);

	const currentWindowRelease = release("319182286", "2026-05-07T03:48:56Z");
	const currentGroups = groupFeedItemsByDay(
		[currentWindowRelease],
		"00:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
		true,
	);

	assert(currentGroups.length === 1, "expected one current raw group");
	assert(currentGroups[0].isCurrent === true, "expected current raw group");
	assert(
		currentGroups[0].id === "2026-05-07@00:00",
		"current raw group id should stay natural-day based",
	);
	assert(
		currentGroups[0].displayDate === "2026-05-07",
		"current raw group should display the current natural day",
	);
	assert(
		currentGroups[0].briefDate === "2026-05-07",
		"current raw group should keep the generated brief target date aligned to the natural day",
	);

	const releasesTabGroups = groupFeedItemsByDay(
		[earlyMorningRawRelease],
		"00:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
		false,
	);

	assert(releasesTabGroups.length === 1, "expected one releases tab group");
	assert(
		releasesTabGroups[0].displayDate === "2026-05-06",
		"release tab raw groups should keep the natural day date",
	);

	console.log("day group display date checks passed");
} finally {
	await server.close();
}
