import { chromium } from "playwright";

const STORY_URL =
	process.env.STORYBOOK_MOBILE_HEADER_URL ??
	"http://127.0.0.1:30160/iframe.html?id=pages-dashboard-header--evidence-mobile-shell&viewMode=story&globals=viewport.value%3Amobile2";

function assert(condition, message) {
	if (!condition) {
		throw new Error(message);
	}
}

async function readHeaderState(page) {
	return page.evaluate(() => {
		const header = document.querySelector("[data-dashboard-header-progress]");
		if (!(header instanceof HTMLElement)) {
			throw new Error("missing [data-dashboard-header-progress]");
		}

		return {
			scrollY: window.scrollY,
			progress: Number.parseFloat(
				header.getAttribute("data-dashboard-header-progress") ?? "0",
			),
			compact: header.getAttribute("data-dashboard-header-compact") === "true",
			interacting:
				header.getAttribute("data-dashboard-header-interacting") === "true",
		};
	});
}

async function openStory(page) {
	await page.goto(STORY_URL, {
		waitUntil: "domcontentloaded",
		timeout: 15_000,
	});
	await page.waitForSelector("[data-dashboard-header-progress]");
}

async function createTouchClient(page) {
	return page.context().newCDPSession(page);
}

async function dispatchTouch(client, type, y) {
	await client.send("Input.dispatchTouchEvent", {
		type,
		touchPoints:
			type === "touchEnd"
				? []
				: [{ x: 200, y, radiusX: 5, radiusY: 5, force: 1, id: 1 }],
	});
}

async function compactHeaderWithTouch(page, client) {
	await openStory(page);
	await dispatchTouch(client, "touchStart", 500);
	await dispatchTouch(client, "touchMove", 430);
	await page.waitForTimeout(80);
	await dispatchTouch(client, "touchMove", 360);
	await page.waitForTimeout(60);
	await dispatchTouch(client, "touchEnd");
	await page.waitForTimeout(240);

	const settledState = await readHeaderState(page);
	assert(!settledState.interacting, "touch drag should settle after release");
	assert(
		settledState.compact && settledState.progress >= 0.9,
		`touch drag release should snap to compact state, got ${JSON.stringify(
			settledState,
		)}`,
	);
}

async function verifyMouseDrag(page) {
	await openStory(page);
	await page.mouse.move(200, 520);
	await page.mouse.down();
	await page.mouse.move(200, 460, { steps: 8 });
	await page.waitForTimeout(80);

	const midState = await readHeaderState(page);
	assert(midState.interacting, "mouse drag should mark header as interacting");
	assert(
		midState.progress > 0.05 && midState.progress < 0.95,
		`mouse drag should expose a mid progress, got ${midState.progress}`,
	);

	await page.mouse.up();
	await page.waitForTimeout(200);
	const settledState = await readHeaderState(page);
	assert(
		!settledState.interacting,
		"mouse drag should settle after pointer up",
	);
	assert(
		!settledState.compact && settledState.progress === 0,
		`mouse drag release should snap back to expanded state, got ${JSON.stringify(
			settledState,
		)}`,
	);
}

async function verifyTouchDrag(page) {
	await openStory(page);
	const client = await createTouchClient(page);

	await dispatchTouch(client, "touchStart", 500);
	await dispatchTouch(client, "touchMove", 430);
	await page.waitForTimeout(80);

	const midState = await readHeaderState(page);
	assert(midState.interacting, "touch drag should mark header as interacting");
	assert(
		midState.progress > 0.25 && midState.progress < 0.95,
		`touch drag should expose a visible mid progress, got ${midState.progress}`,
	);

	await dispatchTouch(client, "touchMove", 360);
	await page.waitForTimeout(60);
	await dispatchTouch(client, "touchEnd");
	await page.waitForTimeout(240);

	const settledState = await readHeaderState(page);
	assert(!settledState.interacting, "touch drag should settle after release");
	assert(
		settledState.compact && settledState.progress >= 0.9,
		`touch drag release should snap to compact state, got ${JSON.stringify(
			settledState,
		)}`,
	);
}

async function verifySlowTouchExpansion(page) {
	const client = await createTouchClient(page);
	await compactHeaderWithTouch(page, client);

	await dispatchTouch(client, "touchStart", 360);
	await page.waitForTimeout(32);

	const progressSamples = [];
	for (const y of [372, 384, 396, 408, 420]) {
		await dispatchTouch(client, "touchMove", y);
		await page.waitForTimeout(48);
		const state = await readHeaderState(page);
		assert(
			state.interacting,
			`slow touch pull should stay interacting at y=${y}, got ${JSON.stringify(
				state,
			)}`,
		);
		progressSamples.push(state.progress);
	}

	for (let index = 1; index < progressSamples.length; index += 1) {
		assert(
			progressSamples[index] <= progressSamples[index - 1] + 0.02,
			`slow touch pull should not rebound while dragging, got ${progressSamples.join(
				", ",
			)}`,
		);
	}

	assert(
		progressSamples.at(-1) < progressSamples[0] - 0.15,
		`slow touch pull should visibly open the header, got ${progressSamples.join(
			", ",
		)}`,
	);

	await dispatchTouch(client, "touchEnd");
	await page.waitForTimeout(240);
}

async function verifyWheelHysteresis(page) {
	await openStory(page);

	await page.mouse.wheel(0, 180);
	const compactSamples = [];
	for (let index = 0; index < 18; index += 1) {
		await page.waitForTimeout(25);
		compactSamples.push(await readHeaderState(page));
	}
	const settledCompact = compactSamples.at(-1);
	assert(Boolean(settledCompact), "wheel down should produce samples");
	assert(
		compactSamples.every(
			(sample) => sample.progress === 0 || sample.progress === 1,
		),
		`wheel down should not jitter through mid states, got ${JSON.stringify(
			compactSamples,
		)}`,
	);
	assert(
		settledCompact.compact && !settledCompact.interacting,
		`wheel down should settle to compact, got ${JSON.stringify(
			settledCompact,
		)}`,
	);

	await page.mouse.wheel(0, -140);
	const expandedSamples = [];
	for (let index = 0; index < 22; index += 1) {
		await page.waitForTimeout(25);
		expandedSamples.push(await readHeaderState(page));
	}
	const settledExpanded = expandedSamples.at(-1);
	assert(Boolean(settledExpanded), "wheel up should produce samples");
	assert(
		expandedSamples.every(
			(sample) => sample.progress === 0 || sample.progress === 1,
		),
		`wheel up should not jitter through mid states, got ${JSON.stringify(
			expandedSamples,
		)}`,
	);
	assert(
		settledExpanded.progress === 0 && !settledExpanded.compact,
		`wheel up should settle to expanded, got ${JSON.stringify(
			settledExpanded,
		)}`,
	);
}

const browser = await chromium.launch({ headless: true });
const context = await browser.newContext({
	viewport: { width: 430, height: 932 },
	isMobile: true,
	hasTouch: true,
});
const page = await context.newPage();

try {
	await verifyMouseDrag(page);
	console.log("✓ mouse drag exposes a mid header state before release");
	await verifyTouchDrag(page);
	console.log("✓ touch drag exposes a mid header state before release");
	await verifySlowTouchExpansion(page);
	console.log("✓ slow touch pull stays stable before release");
	await verifyWheelHysteresis(page);
	console.log("✓ wheel input keeps discrete hysteresis and settles cleanly");
} finally {
	await browser.close();
}
