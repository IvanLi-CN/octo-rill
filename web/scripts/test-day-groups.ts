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
		"08:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
	);

	assert(groups.length === 1, "expected one raw group");
	assert(groups[0].kind === "raw", "expected raw group without brief");
	assert(
		groups[0].id === "2026-05-05@08:00",
		"raw group id should stay window-start based",
	);
	assert(
		groups[0].displayDate === "2026-05-06",
		"raw group should display the brief/window end date",
	);
	assert(
		groups[0].briefDate === "2026-05-06",
		"generated brief date should remain the window end date",
	);

	const historicalGroups = groupFeedItemsByDay(
		[earlyMorningRawRelease],
		"08:00",
		"Asia/Shanghai",
		480,
		[
			{
				id: "brief-2026-05-06",
				date: "2026-05-06",
				window_start: "2026-05-05T00:00:00+00:00",
				window_end: "2026-05-06T00:00:00+00:00",
				release_ids: [earlyMorningRawRelease.id],
			},
		],
		new Date("2026-05-07T12:00:00+08:00"),
	);

	assert(historicalGroups.length === 1, "expected one historical group");
	assert(
		historicalGroups[0].kind === "historical",
		"expected release to fold into its brief",
	);
	assert(
		historicalGroups[0].displayDate === "2026-05-06",
		"historical group should keep brief date",
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
		"08:00",
		"Asia/Shanghai",
		480,
		[],
		new Date("2026-05-07T12:00:00+08:00"),
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

	console.log("day group display date checks passed");
} finally {
	await server.close();
}
