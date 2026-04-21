import { chromium } from "playwright";

const STORY_URL =
	process.env.STORYBOOK_MOBILE_HEADER_URL ??
	"http://127.0.0.1:55176/iframe.html?id=pages-dashboard-header--verification-mobile-shell-drag&viewMode=story&globals=viewport.value%3AdashboardHeaderMobile390";
const LANE_SWITCH_STORY_URL =
	process.env.STORYBOOK_MOBILE_LANE_SWITCH_URL ??
	"http://127.0.0.1:55176/iframe.html?id=pages-dashboard--verification-mobile-lane-switching&viewMode=story&globals=viewport.value%3AdashboardMobile390";
const ALL_TAB_SHELL_STORY_URL =
	process.env.STORYBOOK_MOBILE_ALL_TAB_SHELL_URL ??
	"http://127.0.0.1:55176/iframe.html?id=pages-dashboard--verification-mobile-all-tab-sticky-shell&viewMode=story&globals=viewport.value%3AdashboardMobile390";

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
		timeout: 60_000,
	});
	await page.waitForSelector("[data-dashboard-header-progress]", {
		timeout: 60_000,
	});
}

async function openLaneSwitchStory(page) {
	await page.goto(LANE_SWITCH_STORY_URL, {
		waitUntil: "domcontentloaded",
		timeout: 60_000,
	});
	await page.waitForSelector("[data-dashboard-mobile-lane-menu-trigger]", {
		timeout: 60_000,
	});
}

async function openAllTabShellStory(page) {
	await page.goto(ALL_TAB_SHELL_STORY_URL, {
		waitUntil: "domcontentloaded",
		timeout: 60_000,
	});
	await page.waitForSelector("[data-dashboard-header-progress]", {
		timeout: 60_000,
	});
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

async function dispatchTouchPoint(client, type, x, y) {
	await client.send("Input.dispatchTouchEvent", {
		type,
		touchPoints:
			type === "touchEnd"
				? []
				: [{ x, y, radiusX: 5, radiusY: 5, force: 1, id: 1 }],
	});
}

async function tapLocator(page, locator) {
	const box = await locator.boundingBox();
	assert(box, "expected locator to expose a bounding box");
	await page.touchscreen.tap(box.x + box.width / 2, box.y + box.height / 2);
}

async function dispatchPointerTouch(page, selector, events) {
	await page.evaluate(
		({ targetSelector, pointerEvents }) => {
			const target = document.querySelector(targetSelector);
			if (!(target instanceof HTMLElement)) {
				throw new Error(`missing pointer target: ${targetSelector}`);
			}
			const rect = target.getBoundingClientRect();
			const originX = rect.left + rect.width / 2;
			const originY = rect.top + rect.height / 2;
			for (const pointerEvent of pointerEvents) {
				target.dispatchEvent(
					new PointerEvent(pointerEvent.type, {
						bubbles: true,
						cancelable: true,
						pointerId: 91,
						pointerType: "touch",
						isPrimary: true,
						clientX: originX + (pointerEvent.offsetX ?? 0),
						clientY: originY + (pointerEvent.offsetY ?? 0),
						button: 0,
						buttons: pointerEvent.type === "pointerup" ? 0 : 1,
					}),
				);
			}
		},
		{
			targetSelector: selector,
			pointerEvents: events,
		},
	);
}

async function getLocatorCenter(locator) {
	const box = await locator.boundingBox();
	assert(box, "expected locator to expose a bounding box");
	return {
		x: box.x + box.width / 2,
		y: box.y + box.height / 2,
	};
}

async function expectTouchGuard(selector, page) {
	await page.evaluate((targetSelector) => {
		const trigger = document.querySelector(targetSelector);
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(trigger instanceof HTMLElement) || !(shell instanceof HTMLElement)) {
			throw new Error(
				`missing guarded target or app shell header for ${targetSelector}`,
			);
		}

		const states = [];
		trigger.addEventListener(
			"touchstart",
			() => {
				states.push(
					shell.getAttribute("data-app-shell-header-interacting") ?? "missing",
				);
			},
			{ once: true, passive: true },
		);
		window.__mobileTouchGuardStates = states;
	}, selector);

	const states = await page.evaluate(
		() => window.__mobileTouchGuardStates ?? [],
	);
	assert(states.length === 0, "touch guard probe should start empty");
}

async function readTouchGuardStates(page) {
	return page.evaluate(() => window.__mobileTouchGuardStates ?? []);
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

async function verifyMouseDragFromInteractiveControl(page) {
	await openStory(page);
	const laneMenuTrigger = page.getByRole("button", {
		name: "当前阅读模式：润色",
	});
	const center = await getLocatorCenter(laneMenuTrigger);

	await page.mouse.move(center.x, center.y);
	await page.mouse.down();
	await page.mouse.move(center.x, center.y - 84, { steps: 8 });
	await page.waitForTimeout(80);

	const midState = await readHeaderState(page);
	assert(
		midState.interacting,
		"mouse drag that starts on the lane menu should still enter header interaction",
	);
	assert(
		midState.progress > 0.05,
		`interactive mouse drag should move header progress, got ${midState.progress}`,
	);

	await page.mouse.up();
	await page.waitForTimeout(200);
	const settledState = await readHeaderState(page);
	assert(
		!settledState.interacting,
		"interactive mouse drag should settle after pointer up",
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

async function verifyTouchDragFromGuardedTab(page) {
	await openStory(page);
	const guardedTab = page.getByRole("tab", { name: "发布" });
	const center = await getLocatorCenter(guardedTab);
	const client = await createTouchClient(page);
	assert(
		(await guardedTab.getAttribute("aria-selected")) === "false",
		"the guarded tab should start inactive before the drag begins",
	);

	await dispatchTouchPoint(client, "touchStart", center.x, center.y);
	await dispatchTouchPoint(client, "touchMove", center.x, center.y - 28);
	await page.waitForTimeout(80);

	const midState = await readHeaderState(page);
	assert(
		midState.interacting,
		"touch drag that starts on the guarded tab should still enter header interaction after crossing the tap slop",
	);
	assert(
		midState.progress > 0.05,
		`guarded tab drag should move header progress, got ${midState.progress}`,
	);

	await dispatchTouchPoint(client, "touchEnd", center.x, center.y - 28);
	await page.waitForTimeout(240);
	const settledState = await readHeaderState(page);
	assert(
		!settledState.interacting,
		"guarded tab drag should settle after touch release",
	);
	assert(
		(await guardedTab.getAttribute("aria-selected")) === "false",
		"guarded tab drag should not activate the underlying tab",
	);
}

async function verifyPointerOnlyGuardedTouchDrag(page) {
	await openStory(page);
	await dispatchPointerTouch(
		page,
		"[data-dashboard-mobile-lane-menu-trigger]",
		[{ type: "pointerdown" }, { type: "pointermove", offsetY: -28 }],
	);
	await page.waitForTimeout(80);

	const midState = await readHeaderState(page);
	assert(
		midState.interacting,
		`guarded pointer-only drag should still enter header interaction, got ${JSON.stringify(
			midState,
		)}`,
	);

	await dispatchPointerTouch(
		page,
		"[data-dashboard-mobile-lane-menu-trigger]",
		[{ type: "pointerup", offsetY: -28 }],
	);
	await page.waitForTimeout(240);
	const settledState = await readHeaderState(page);
	assert(
		!settledState.interacting,
		"guarded pointer-only drag should settle after pointer up",
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
		compactSamples.every((sample) => !sample.interacting),
		`wheel down should not enter an interaction state, got ${JSON.stringify(
			compactSamples,
		)}`,
	);
	assert(
		compactSamples.some((sample) => sample.compact),
		`wheel down should eventually compact the header, got ${JSON.stringify(
			compactSamples,
		)}`,
	);
	assert(
		settledCompact.compact &&
			!settledCompact.interacting &&
			settledCompact.progress >= 0.8,
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
		expandedSamples.every((sample) => !sample.interacting),
		`wheel up should not enter an interaction state, got ${JSON.stringify(
			expandedSamples,
		)}`,
	);
	assert(
		expandedSamples.some((sample) => sample.progress <= 0.1),
		`wheel up should eventually reopen the header, got ${JSON.stringify(
			expandedSamples,
		)}`,
	);
	assert(
		settledExpanded.progress <= 0.15 && !settledExpanded.compact,
		`wheel up should settle to expanded, got ${JSON.stringify(
			settledExpanded,
		)}`,
	);
}

async function verifyLaneMenuTouchGuard(page) {
	await openLaneSwitchStory(page);
	await expectTouchGuard("[data-dashboard-mobile-lane-menu-trigger]", page);

	const laneMenuTrigger = page.locator(
		"[data-dashboard-mobile-lane-menu-trigger]",
	);
	const center = await getLocatorCenter(laneMenuTrigger);
	const client = await createTouchClient(page);

	await dispatchTouchPoint(client, "touchStart", center.x, center.y);
	await dispatchTouchPoint(client, "touchMove", center.x + 10, center.y);
	await page.waitForTimeout(80);
	const guardedHorizontalState = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		guardedHorizontalState === "false",
		`lane menu horizontal touchmove should stay ignored, got ${guardedHorizontalState}`,
	);
	await dispatchTouchPoint(client, "touchEnd", center.x + 10, center.y);
	await page.waitForTimeout(80);

	await openLaneSwitchStory(page);
	await expectTouchGuard("[data-dashboard-mobile-lane-menu-trigger]", page);
	const verticalClient = await createTouchClient(page);
	const verticalCenter = await getLocatorCenter(
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await dispatchTouchPoint(
		verticalClient,
		"touchStart",
		verticalCenter.x,
		verticalCenter.y,
	);
	await dispatchTouchPoint(
		verticalClient,
		"touchMove",
		verticalCenter.x,
		verticalCenter.y - 10,
	);
	await page.waitForTimeout(80);
	const guardedMoveState = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		guardedMoveState === "false",
		`lane menu touchmove should stay ignored, got ${guardedMoveState}`,
	);
	await dispatchTouchPoint(
		verticalClient,
		"touchEnd",
		verticalCenter.x,
		verticalCenter.y - 10,
	);
	await page.waitForTimeout(80);

	await openLaneSwitchStory(page);
	const promotedDragClient = await createTouchClient(page);
	const promotedDragCenter = await getLocatorCenter(
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await dispatchTouchPoint(
		promotedDragClient,
		"touchStart",
		promotedDragCenter.x,
		promotedDragCenter.y,
	);
	await dispatchTouchPoint(
		promotedDragClient,
		"touchMove",
		promotedDragCenter.x,
		promotedDragCenter.y - 28,
	);
	await page.waitForTimeout(80);
	const promotedDragState = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		promotedDragState === "true",
		`lane menu drag beyond the tap slop should promote into header interaction, got ${promotedDragState}`,
	);
	await dispatchTouchPoint(
		promotedDragClient,
		"touchEnd",
		promotedDragCenter.x,
		promotedDragCenter.y - 28,
	);
	await page.waitForTimeout(120);
	assert(
		(await page
			.locator("[data-dashboard-mobile-lane-menu-popover]")
			.count()) === 0,
		"lane menu drag promotion should suppress the pending tap/click",
	);
	await tapLocator(
		page,
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await page.waitForSelector("[data-dashboard-mobile-lane-menu-popover]");

	await openLaneSwitchStory(page);
	await expectTouchGuard("[data-dashboard-mobile-lane-menu-trigger]", page);
	await tapLocator(
		page,
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await page.waitForSelector("[data-dashboard-mobile-lane-menu-popover]");

	const touchStates = await readTouchGuardStates(page);
	assert(
		touchStates.length === 1 && touchStates[0] === "false",
		`lane menu tap should keep app shell interaction idle, got ${JSON.stringify(
			touchStates,
		)}`,
	);

	const translatedOption = page
		.locator("[data-dashboard-mobile-lane-menu-popover]")
		.getByRole("menuitemradio", {
			name: "翻译",
		});
	const optionCenter = await getLocatorCenter(translatedOption);
	const optionClient = await createTouchClient(page);
	await dispatchTouchPoint(
		optionClient,
		"touchStart",
		optionCenter.x,
		optionCenter.y,
	);
	await dispatchTouchPoint(
		optionClient,
		"touchMove",
		optionCenter.x + 10,
		optionCenter.y,
	);
	await page.waitForTimeout(80);
	const optionHorizontalState = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		optionHorizontalState === "false",
		`lane option horizontal touchmove should stay ignored, got ${optionHorizontalState}`,
	);
	await dispatchTouchPoint(
		optionClient,
		"touchEnd",
		optionCenter.x + 10,
		optionCenter.y,
	);
	await page.waitForTimeout(80);

	await openLaneSwitchStory(page);
	await expectTouchGuard("[data-dashboard-mobile-lane-menu-trigger]", page);
	await tapLocator(
		page,
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await page.waitForSelector("[data-dashboard-mobile-lane-menu-popover]");
	const optionVerticalClient = await createTouchClient(page);
	const refreshedTranslatedOption = page
		.locator("[data-dashboard-mobile-lane-menu-popover]")
		.getByRole("menuitemradio", {
			name: "翻译",
		});
	const refreshedOptionCenter = await getLocatorCenter(
		refreshedTranslatedOption,
	);
	await dispatchTouchPoint(
		optionVerticalClient,
		"touchStart",
		refreshedOptionCenter.x,
		refreshedOptionCenter.y,
	);
	await dispatchTouchPoint(
		optionVerticalClient,
		"touchMove",
		refreshedOptionCenter.x,
		refreshedOptionCenter.y - 10,
	);
	await page.waitForTimeout(80);
	const optionVerticalState = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		optionVerticalState === "false",
		`lane option vertical touchmove should stay ignored, got ${optionVerticalState}`,
	);
	await dispatchTouchPoint(
		optionVerticalClient,
		"touchEnd",
		refreshedOptionCenter.x,
		refreshedOptionCenter.y - 10,
	);
	await page.waitForTimeout(80);

	await openLaneSwitchStory(page);
	await expectTouchGuard("[data-dashboard-mobile-lane-menu-trigger]", page);
	await tapLocator(
		page,
		page.locator("[data-dashboard-mobile-lane-menu-trigger]"),
	);
	await page.waitForSelector("[data-dashboard-mobile-lane-menu-popover]");
	await tapLocator(
		page,
		page
			.locator("[data-dashboard-mobile-lane-menu-popover]")
			.getByRole("menuitemradio", {
				name: "翻译",
			}),
	);
	await page.waitForTimeout(120);
	await page
		.getByRole("heading", { name: "v2.63.0（稳定版）" })
		.waitFor({ state: "visible" });
	const headerInteracting = await page.evaluate(() => {
		const shell = document.querySelector("[data-app-shell-header-interacting]");
		if (!(shell instanceof HTMLElement)) {
			throw new Error("missing [data-app-shell-header-interacting]");
		}
		return shell.getAttribute("data-app-shell-header-interacting");
	});
	assert(
		headerInteracting === "false",
		`lane switch completion should keep app shell idle, got ${headerInteracting}`,
	);
}

async function verifyUserMenuTouchGuard(page) {
	await openStory(page);
	await expectTouchGuard("[data-dashboard-user-menu] button", page);
	await tapLocator(page, page.getByRole("button", { name: "查看账号信息" }));
	await page.getByRole("dialog", { name: "账号信息" }).waitFor({
		state: "visible",
	});

	const touchStates = await readTouchGuardStates(page);
	assert(
		touchStates.length === 1 && touchStates[0] === "false",
		`user menu tap should keep app shell interaction idle, got ${JSON.stringify(
			touchStates,
		)}`,
	);
}

async function verifyAllTabStickyShell(page) {
	await openAllTabShellStory(page);

	const readViewportBinding = async () =>
		page.evaluate(() => {
			const shell = document.querySelector(
				"[data-app-shell-mobile-chrome='true']",
			);
			const headerState = document.querySelector(
				"[data-dashboard-header-progress]",
			);
			const stickyHeader = document.querySelector(
				"[data-app-shell-header='true']",
			);
			if (
				!(shell instanceof HTMLElement) ||
				!(headerState instanceof HTMLElement) ||
				!(stickyHeader instanceof HTMLElement)
			) {
				throw new Error("missing app shell or dashboard header");
			}

			return {
				boundViewportHeight: Number.parseInt(
					shell.getAttribute("data-app-shell-viewport-height") ?? "0",
					10,
				),
				viewportHeight: Math.round(
					window.visualViewport?.height ?? window.innerHeight,
				),
				headerTop: Math.round(stickyHeader.getBoundingClientRect().top),
				compact:
					headerState.getAttribute("data-dashboard-header-compact") === "true",
			};
		});

	const beforeScroll = await readViewportBinding();
	assert(
		Math.abs(beforeScroll.boundViewportHeight - beforeScroll.viewportHeight) <=
			1,
		`all-tab shell should bind to the current viewport height before scroll, got ${JSON.stringify(
			beforeScroll,
		)}`,
	);
	assert(
		Math.abs(beforeScroll.headerTop) <= 1,
		`all-tab shell header should start pinned to the viewport top, got ${JSON.stringify(
			beforeScroll,
		)}`,
	);

	await page.setViewportSize({ width: 430, height: 860 });
	await page.waitForTimeout(220);
	const afterResize = await readViewportBinding();
	assert(
		Math.abs(afterResize.boundViewportHeight - afterResize.viewportHeight) <= 1,
		`all-tab shell should update its viewport-height binding after a viewport resize, got ${JSON.stringify(
			afterResize,
		)}`,
	);
	assert(
		Math.abs(afterResize.headerTop) <= 1,
		`all-tab shell header should stay pinned to the viewport top after resize, got ${JSON.stringify(
			afterResize,
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
	await verifyMouseDragFromInteractiveControl(page);
	console.log(
		"✓ mouse drag still works when it starts from the lane menu button",
	);
	await verifyTouchDrag(page);
	console.log("✓ touch drag exposes a mid header state before release");
	await verifyTouchDragFromGuardedTab(page);
	console.log(
		"✓ guarded mobile tabs still allow drag after the touch crosses the tap slop",
	);
	await verifyPointerOnlyGuardedTouchDrag(page);
	console.log(
		"✓ guarded controls still promote into a header drag in pointer-only touch environments",
	);
	await verifySlowTouchExpansion(page);
	console.log("✓ slow touch pull stays stable before release");
	await verifyWheelHysteresis(page);
	console.log("✓ wheel input keeps discrete hysteresis and settles cleanly");
	await verifyLaneMenuTouchGuard(page);
	console.log(
		"✓ lane menu tap keeps header gesture idle and still switches to translated lane",
	);
	await verifyUserMenuTouchGuard(page);
	console.log(
		"✓ user menu tap keeps header gesture idle while opening the account popover",
	);
	await verifyAllTabStickyShell(page);
	console.log(
		"✓ all-tab mobile shell keeps the header pinned while tracking the live viewport height",
	);
} finally {
	await browser.close();
}
