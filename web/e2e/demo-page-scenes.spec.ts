import { expect, test } from "@playwright/test";

test.describe("mock-only page demo scenes", () => {
	test("admin dashboard and repo governance scenes are deep-linkable", async ({
		page,
	}) => {
		await page.goto("/admin/?demo=admin-dashboard-overview&d_controls=hidden");
		await expect(
			page.locator('[data-admin-dashboard-shell="true"]'),
		).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("运营总览")).toBeVisible();

		await page.goto("/admin/repos?demo=admin-repos-overview&d_controls=hidden");
		await expect(page.getByText("仓库刷新治理")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("octo-demo/release-lab")).toBeVisible();
		await expect(page.getByText("octo-demo/docs-hub")).toBeVisible();
	});

	test("bind github and announcement detail scenes use deterministic payloads", async ({
		page,
	}) => {
		await page.goto(
			"/bind/github?demo=bind-github-pending&linuxdo=connected&passkey=created&d_controls=hidden",
		);
		await expect(page.getByText("可以继续绑定 GitHub")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText("Passkey 已暂存")).toBeVisible();

		await page.goto(
			"/octo-demo/release-lab/discussions/42?demo=announcement-detail&from=feed&d_controls=hidden",
		);
		await expect(page.getByRole("tab", { name: "润色" })).toHaveAttribute(
			"aria-selected",
			"true",
			{ timeout: 15_000 },
		);
		await expect(
			page.getByRole("heading", { name: "公告：站内阅读流已对齐" }),
		).toBeVisible();
	});

	test("focused public release demo retains the complete timeline", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 1000 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.16.0?demo=public-release-ready&d_controls=hidden",
		);

		await expect(page.getByTestId("public-release-directory")).toBeVisible({
			timeout: 15_000,
		});
		const directory = page.getByTestId("public-release-directory-scroll");
		for (let attempt = 0; attempt < 24; attempt += 1) {
			await directory.evaluate((element) => {
				element.scrollTop = element.scrollHeight;
			});
			if (
				(await page
					.getByTestId("public-release-directory-virtual-list")
					.getAttribute("data-release-count")) === "100"
			) {
				break;
			}
			await page.waitForTimeout(120);
		}
		await expect(
			page.getByTestId("public-release-directory-virtual-list"),
		).toHaveAttribute("data-release-count", "100", { timeout: 15_000 });
		await expect(
			page.getByTestId("public-release-virtual-list"),
		).toHaveAttribute("data-release-count", "100", { timeout: 15_000 });
		await expect(page.getByTestId("public-release-directory-scroll")).toHaveCSS(
			"overflow-y",
			"auto",
		);
	});

	test("focused public release demo keeps far discrete targets in the first response", async ({
		page,
	}) => {
		const responsePromise = page.waitForResponse(
			(response) =>
				response
					.url()
					.includes("/api/public/repos/octo-demo/release-lab/releases") &&
				!response.url().includes("/content"),
		);
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v1.50.0?demo=public-release-ready&highlight=id%3A291058027&highlight=id%3A291057928&d_controls=hidden",
		);
		const response = (await responsePromise).json() as Promise<{
			items: Array<{ release_id: string }>;
		}>;
		const data = await response;
		const ids = new Set(data.items.map((item) => item.release_id));
		expect(ids.has("291058027")).toBe(true);
		expect(ids.has("291057928")).toBe(true);
		expect(data.items.length).toBeLessThanOrEqual(30);
	});

	test("highlight navigation moves keyboard focus to the selected release card", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v1.50.0?demo=public-release-ready&highlight=id%3A291058027&highlight=id%3A291057928&d_controls=hidden",
		);

		const navigation = page.getByTestId("public-release-highlight-navigation");
		await expect(navigation).toBeVisible({ timeout: 15_000 });
		await navigation.getByTitle("下一条高亮记录").click();
		await expect(page.locator("[data-release-id='291057928']")).toBeFocused({
			timeout: 8_000,
		});
	});

	test("focused public release demo does not leave estimated gaps between cards", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 1000 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v1.50.0?demo=public-release-ready&d_controls=hidden",
		);

		const detail = page.getByTestId("public-release-detail-scroll");
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await expect
			.poll(
				() =>
					page.evaluate(() => {
						const rows = [
							...document.querySelectorAll<HTMLElement>(
								"[data-testid='public-release-virtual-list'] > [data-index]",
							),
						]
							.map((element) => {
								const rect = element.getBoundingClientRect();
								return {
									hasRelease: Boolean(
										element.querySelector("[data-release-id]"),
									),
									top: rect.top,
									bottom: rect.bottom,
								};
							})
							.filter((row) => row.hasRelease);
						if (rows.length < 3) return null;
						return Math.max(
							...rows
								.slice(1)
								.map((row, index) => row.top - rows[index]!.bottom),
						);
					}),
				{ timeout: 5_000 },
			)
			.toBeLessThanOrEqual(24);
	});

	test("switching public release lanes keeps the focused card in view", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 720 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v1.50.0?demo=public-release-ready&d_controls=hidden",
		);

		const detail = page.getByTestId("public-release-detail-scroll");
		const laneSelector = page.locator("[data-feed-page-lane-selector='true']");
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await expect(
			page.locator("[data-current-release='true'][data-release-id]"),
		).toHaveCount(1, { timeout: 15_000 });

		const targetId = await page
			.locator("[data-current-release='true'][data-release-id]")
			.getAttribute("data-release-id");
		expect(targetId).toBeTruthy();
		const readTargetState = () =>
			page.evaluate((id) => {
				const scroll = document.querySelector<HTMLElement>(
					"[data-testid='public-release-detail-scroll']",
				);
				const target = id
					? document.querySelector<HTMLElement>(
							`[data-release-id='${CSS.escape(id)}']`,
						)
					: null;
				if (!scroll || !target) return null;
				const view = scroll.getBoundingClientRect();
				const rect = target.getBoundingClientRect();
				return {
					visible: rect.bottom > view.top && rect.top < view.bottom,
					top: rect.top,
					bottom: rect.bottom,
					viewTop: view.top,
					viewBottom: view.bottom,
					scrollTop: scroll.scrollTop,
					scrollHeight: scroll.scrollHeight,
					height: rect.height,
				};
			}, targetId);
		await expect.poll(readTargetState, { timeout: 5_000 }).toMatchObject({
			visible: true,
		});
		await page.waitForTimeout(5_500);
		await expect.poll(readTargetState, { timeout: 1_000 }).toMatchObject({
			visible: true,
		});

		for (const lane of ["original", "translated", "smart"]) {
			const beforeLane = await readTargetState();
			await laneSelector.locator(`[data-feed-page-lane='${lane}']`).click();
			await expect(
				laneSelector.locator(`[data-feed-page-lane='${lane}']`),
			).toHaveAttribute("aria-pressed", "true");
			await page.waitForTimeout(700);
			const state = await readTargetState();
			expect(
				state?.visible,
				`${lane}: before=${JSON.stringify(beforeLane)} after=${JSON.stringify(state)}`,
			).toBe(true);
		}
	});

	test("switching public release lanes after the initial seek does not revive a stale scroll guard", async ({
		page,
	}) => {
		test.setTimeout(60_000);
		await page.setViewportSize({ width: 1440, height: 720 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v1.40.0?demo=public-release-ready&d_controls=hidden",
		);

		const detail = page.getByTestId("public-release-detail-scroll");
		const laneSelector = page.locator("[data-feed-page-lane-selector='true']");
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await expect(
			page.locator("[data-current-release='true'][data-release-id]"),
		).toHaveCount(1, { timeout: 15_000 });
		const targetId = await page
			.locator("[data-current-release='true'][data-release-id]")
			.getAttribute("data-release-id");
		expect(targetId).toBeTruthy();

		const readTargetState = () =>
			page.evaluate((id) => {
				const scroll = document.querySelector<HTMLElement>(
					"[data-testid='public-release-detail-scroll']",
				);
				const target = id
					? document.querySelector<HTMLElement>(
							`[data-release-id='${CSS.escape(id)}']`,
						)
					: null;
				const current = document.querySelector<HTMLElement>(
					"[data-current-release='true'][data-release-id]",
				);
				if (!scroll || !target) return null;
				const view = scroll.getBoundingClientRect();
				const rect = target.getBoundingClientRect();
				return {
					currentId: current?.dataset.releaseId ?? null,
					visible: rect.bottom > view.top && rect.top < view.bottom,
					scrollTop: scroll.scrollTop,
				};
			}, targetId);

		await page.waitForTimeout(5_500);
		await expect.poll(readTargetState, { timeout: 2_000 }).toMatchObject({
			currentId: targetId,
			visible: true,
		});

		for (const lane of ["original", "translated", "smart"]) {
			await page.evaluate(() => {
				const scroll = document.querySelector<HTMLElement>(
					"[data-testid='public-release-detail-scroll']",
				);
				if (!scroll) throw new Error("detail scroll missing");
				const samples: number[] = [];
				(
					window as unknown as { __publicReleaseLaneScrollSamples?: number[] }
				).__publicReleaseLaneScrollSamples = samples;
				const startedAt = performance.now();
				const sample = (time: number) => {
					samples.push(scroll.scrollTop);
					if (time - startedAt < 3_200) requestAnimationFrame(sample);
				};
				requestAnimationFrame(sample);
			});
			await laneSelector.locator(`[data-feed-page-lane='${lane}']`).click();
			await expect(
				laneSelector.locator(`[data-feed-page-lane='${lane}']`),
			).toHaveAttribute("aria-pressed", "true");
			await page.waitForTimeout(3_400);
			await expect.poll(readTargetState, { timeout: 2_000 }).toMatchObject({
				currentId: targetId,
				visible: true,
			});
			const samples = await page.evaluate(
				() =>
					(
						window as unknown as {
							__publicReleaseLaneScrollSamples?: number[];
						}
					).__publicReleaseLaneScrollSamples ?? [],
			);
			const settled = samples.slice(-24);
			if (settled.length > 1) {
				expect(
					Math.max(...settled) - Math.min(...settled),
					`${lane}: scroll position kept oscillating: ${JSON.stringify(settled)}`,
				).toBeLessThanOrEqual(1);
			}
		}
	});

	test("focused public release demo keeps alternating directory targets fully visible", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 1317 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const detail = page.getByTestId("public-release-detail-scroll");
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await expect(
			page.getByTestId("public-release-directory-virtual-list"),
		).toHaveAttribute("data-release-count", "100", { timeout: 15_000 });

		const targetIds = ["291058026", "291058011"];
		for (const [index, releaseId] of [
			...targetIds,
			...targetIds,
			...targetIds,
		].entries()) {
			const link = directory.locator(
				`[data-release-directory-id="${releaseId}"]`,
			);
			await link.click();
			await expect(link).toHaveAttribute("aria-current", "page");
			await expect
				.poll(
					async () =>
						page.evaluate((id) => {
							const scroll = document.querySelector<HTMLElement>(
								"[data-testid='public-release-detail-scroll']",
							);
							const card = document.querySelector<HTMLElement>(
								`[data-release-id="${id}"]`,
							);
							if (!scroll || !card) return false;
							const view = scroll.getBoundingClientRect();
							const rect = card.getBoundingClientRect();
							return rect.top >= view.top && rect.bottom <= view.bottom + 1;
						}, releaseId),
					{ timeout: 4_500 },
				)
				.toBe(true);
			await expect(page).toHaveURL(
				new RegExp(
					String.raw`/releases/tag/v2\.${index % 2 === 0 ? "30" : "15"}\.0(?:\?.*)?$`,
				),
			);
		}
	});

	test("focused public release demo does not jiggle detail on a visible directory click", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1152, height: 1080 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const detail = page.getByTestId("public-release-detail-scroll");
		const target = directory.locator("[data-release-directory-id='291058026']");
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await expect(target).toBeVisible({ timeout: 15_000 });
		await page.waitForTimeout(1_200);
		const before = await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			const card = document.querySelector<HTMLElement>(
				"[data-release-id='291058026']",
			);
			if (!scroll || !card) return null;
			const view = scroll.getBoundingClientRect();
			const rect = card.getBoundingClientRect();
			return {
				scrollTop: scroll.scrollTop,
				fullyVisible: rect.top >= view.top && rect.bottom <= view.bottom + 1,
			};
		});
		expect(before?.fullyVisible).toBe(true);
		await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			if (!scroll) throw new Error("detail scroll missing");
			const samples: number[] = [];
			(
				window as unknown as { __publicReleaseScrollSamples?: number[] }
			).__publicReleaseScrollSamples = samples;
			scroll.addEventListener("scroll", () => samples.push(scroll.scrollTop), {
				passive: true,
			});
		});
		await target.click();
		await expect(page).toHaveURL(/\/releases\/tag\/v2\.30\.0(?:\?.*)?$/);
		await page.waitForTimeout(1_000);
		const after = await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			const samples =
				(window as unknown as { __publicReleaseScrollSamples?: number[] })
					.__publicReleaseScrollSamples ?? [];
			return {
				scrollTop: scroll?.scrollTop ?? null,
				changes: samples.filter(
					(value, index) => index === 0 || value !== samples[index - 1],
				),
			};
		});
		expect(after.scrollTop).toBe(before?.scrollTop ?? null);
		expect(after.changes).toEqual([]);
	});

	test("focused public release hover propagates during initial focus", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1152, height: 1080 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const targetCard = page.getByTestId("public-release-item-291058026");
		const targetDirectoryItem = directory.locator(
			"[data-release-directory-id='291058026']",
		);
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(targetCard).toBeVisible({ timeout: 15_000 });
		const box = await targetCard.boundingBox();
		expect(box).not.toBeNull();
		await page.mouse.move(box!.x + box!.width / 2, box!.y + box!.height / 2);
		await expect(targetDirectoryItem).toHaveAttribute("aria-current", "page", {
			timeout: 1_500,
		});
	});

	test("focused public release demo keeps a visible detail target still during a directory click", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 720 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const detail = page.getByTestId("public-release-detail-scroll");
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await page.waitForTimeout(1_200);
		await detail.hover();
		await page.mouse.wheel(0, 350);
		await page.waitForTimeout(500);

		let target: string | null = null;
		for (let attempt = 0; attempt < 6 && !target; attempt += 1) {
			target = await detail.evaluate(() => {
				const scroll = document.querySelector<HTMLElement>(
					"[data-testid='public-release-detail-scroll']",
				);
				if (!scroll) return null;
				const viewport = scroll.getBoundingClientRect();
				const candidates = [
					...scroll.querySelectorAll<HTMLElement>("[data-release-id]"),
				].filter((element) => {
					const rect = element.getBoundingClientRect();
					return rect.top >= viewport.top && rect.bottom <= viewport.bottom + 1;
				});
				return candidates.at(-1)?.dataset.releaseId ?? null;
			});
			if (!target) {
				await page.mouse.wheel(0, 120);
				await page.waitForTimeout(150);
			}
		}
		if (!target) throw new Error("expected a fully visible detail target");
		const directoryTarget = directory.locator(
			`[data-release-directory-id="${target}"]`,
		);
		await expect(directoryTarget).toBeVisible();
		const before = await detail.evaluate((scroll) => scroll.scrollTop);
		await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			if (!scroll) throw new Error("detail scroll missing");
			const samples: Array<{ time: number; top: number }> = [];
			(
				window as unknown as {
					__publicReleaseScrollFrameSamples?: Array<{
						time: number;
						top: number;
					}>;
				}
			).__publicReleaseScrollFrameSamples = samples;
			const startedAt = performance.now();
			const sample = (time: number) => {
				samples.push({ time: time - startedAt, top: scroll.scrollTop });
				if (time - startedAt < 3_200) requestAnimationFrame(sample);
			};
			requestAnimationFrame(sample);
		});
		await directoryTarget.click();
		await page.waitForTimeout(3_400);
		const samples = await page.evaluate(() => {
			return (
				(
					window as unknown as {
						__publicReleaseScrollFrameSamples?: Array<{
							time: number;
							top: number;
						}>;
					}
				).__publicReleaseScrollFrameSamples ?? []
			);
		});
		const min = Math.min(...samples.map((sample) => sample.top));
		const max = Math.max(...samples.map((sample) => sample.top));
		expect(
			max - min,
			`detail scroll changed from ${before}; target=${target}`,
		).toBeLessThanOrEqual(1);
	});

	test("focused public release demo reveals an offscreen target without rebound", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 720 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const detail = page.getByTestId("public-release-detail-scroll");
		const targetId = "291058016";
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await page.waitForTimeout(1_200);
		await directory.hover();
		await page.mouse.wheel(0, 1_000);
		const directoryTarget = directory.locator(
			`[data-release-directory-id="${targetId}"]`,
		);
		await expect(directoryTarget).toBeVisible({ timeout: 5_000 });
		const before = await detail.evaluate((scroll) => scroll.scrollTop);
		await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			if (!scroll) throw new Error("detail scroll missing");
			const samples: Array<{ time: number; top: number }> = [];
			(
				window as unknown as {
					__publicReleaseScrollFrameSamples?: Array<{
						time: number;
						top: number;
					}>;
				}
			).__publicReleaseScrollFrameSamples = samples;
			const startedAt = performance.now();
			const sample = (time: number) => {
				samples.push({ time: time - startedAt, top: scroll.scrollTop });
				if (time - startedAt < 3_200) requestAnimationFrame(sample);
			};
			requestAnimationFrame(sample);
		});
		await directoryTarget.click();
		await expect(page).toHaveURL(/\/releases\/tag\/v2\.20\.0(?:\?.*)?$/);
		await page.waitForTimeout(3_400);
		await expect
			.poll(() =>
				page.evaluate((id) => {
					const scroll = document.querySelector<HTMLElement>(
						"[data-testid='public-release-detail-scroll']",
					);
					const target = scroll?.querySelector<HTMLElement>(
						`[data-release-id="${id}"]`,
					);
					if (!scroll || !target) return false;
					const view = scroll.getBoundingClientRect();
					const rect = target.getBoundingClientRect();
					return rect.top >= view.top && rect.bottom <= view.bottom + 1;
				}, targetId),
			)
			.toBe(true);
		const samples = await page.evaluate(() => {
			return (
				(
					window as unknown as {
						__publicReleaseScrollFrameSamples?: Array<{
							time: number;
							top: number;
						}>;
					}
				).__publicReleaseScrollFrameSamples ?? []
			);
		});
		const changes = samples
			.map(
				(sample, index) => sample.top - (samples[index - 1]?.top ?? sample.top),
			)
			.filter((delta) => Math.abs(delta) > 2);
		const firstForward = changes.findIndex((delta) => delta > 2);
		const rebound =
			firstForward >= 0 &&
			changes.slice(firstForward + 1).some((delta) => delta < -2);
		expect(
			rebound,
			`detail scroll reversed after moving forward: before=${before}, changes=${JSON.stringify(changes)}`,
		).toBe(false);
	});

	test("focused public release demo keeps a far target on one continuous seek", async ({
		page,
	}) => {
		await page.setViewportSize({ width: 1440, height: 720 });
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v2.31.0?demo=public-release-ready&d_controls=hidden",
		);

		const directory = page.getByTestId("public-release-directory-scroll");
		const detail = page.getByTestId("public-release-detail-scroll");
		const targetId = "291057928";
		await expect(directory).toBeVisible({ timeout: 15_000 });
		await expect(detail).toBeVisible({ timeout: 15_000 });
		await directory.evaluate((element) => {
			element.scrollTop = element.scrollHeight;
		});
		const directoryTarget = directory.locator(
			`[data-release-directory-id="${targetId}"]`,
		);
		await expect(directoryTarget).toBeVisible({ timeout: 5_000 });
		await page.evaluate(() => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			if (!scroll) throw new Error("detail scroll missing");
			const samples: Array<{ time: number; top: number }> = [];
			(
				window as unknown as {
					__publicReleaseScrollFrameSamples?: Array<{
						time: number;
						top: number;
					}>;
				}
			).__publicReleaseScrollFrameSamples = samples;
			const startedAt = performance.now();
			const sample = (time: number) => {
				samples.push({ time: time - startedAt, top: scroll.scrollTop });
				if (time - startedAt < 3_200) requestAnimationFrame(sample);
			};
			requestAnimationFrame(sample);
		});
		await directoryTarget.click();
		await expect(page).toHaveURL(/\/releases\/tag\/v1\.32\.0(?:\?.*)?$/);
		await page.waitForTimeout(3_400);
		const finalState = await page.evaluate((id) => {
			const scroll = document.querySelector<HTMLElement>(
				"[data-testid='public-release-detail-scroll']",
			);
			const target = scroll?.querySelector<HTMLElement>(
				`[data-release-id="${id}"]`,
			);
			if (!scroll || !target)
				return {
					visible: false,
					mounted: Boolean(target),
					scrollTop: scroll?.scrollTop ?? null,
					scrollHeight: scroll?.scrollHeight ?? null,
					clientHeight: scroll?.clientHeight ?? null,
					mountedIds: Array.from(
						scroll?.querySelectorAll<HTMLElement>("[data-release-id]") ?? [],
					).map((element) => element.dataset.releaseId),
				};
			const view = scroll.getBoundingClientRect();
			const rect = target.getBoundingClientRect();
			return {
				visible: rect.top >= view.top && rect.bottom <= view.bottom + 1,
				mounted: true,
				scrollTop: scroll.scrollTop,
				scrollHeight: scroll.scrollHeight,
				clientHeight: scroll.clientHeight,
				top: rect.top,
				bottom: rect.bottom,
				viewTop: view.top,
				viewBottom: view.bottom,
			};
		}, targetId);
		expect(finalState.visible, JSON.stringify(finalState)).toBe(true);
		const samples = await page.evaluate(
			() =>
				(
					window as unknown as {
						__publicReleaseScrollFrameSamples?: Array<{
							time: number;
							top: number;
						}>;
					}
				).__publicReleaseScrollFrameSamples ?? [],
		);
		const changes = samples
			.map(
				(sample, index) => sample.top - (samples[index - 1]?.top ?? sample.top),
			)
			.filter((delta) => Math.abs(delta) > 2);
		const firstForward = changes.findIndex((delta) => delta > 2);
		const rebound =
			firstForward >= 0 &&
			changes.slice(firstForward + 1).some((delta) => delta < -2);
		expect(
			rebound,
			`far detail seek reversed: changes=${JSON.stringify(changes)}`,
		).toBe(false);
	});

	test("unknown public release focus returns the documented not-found error", async ({
		page,
	}) => {
		await page.goto(
			"/public/octo-demo/release-lab/releases/tag/v9.99.0?demo=public-release-ready&d_controls=hidden",
		);
		await expect(
			page.getByText(
				"release_not_found_or_not_cached: release not found or not cached",
			),
		).toBeVisible({ timeout: 15_000 });
	});

	test("not found and app boot scenes remain explicit", async ({ page }) => {
		await page.goto("/demo-missing-route?demo=not-found&d_controls=hidden");
		await expect(page.locator("[data-not-found-surface]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(
			page.getByRole("heading", { name: "页面不存在" }),
		).toBeVisible();

		await page.goto("/?demo=app-boot&d_controls=hidden");
		await expect(page.locator("[data-app-boot]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(
			page.getByText("应用正在完成初始化，请稍候片刻。"),
		).toBeVisible();
	});

	test("app shell version states are addressable without a real service worker", async ({
		page,
	}) => {
		await page.goto(
			"/focus/repo/octo-demo/release-lab?demo=app-shell&d_shell=update-install&d_controls=hidden",
		);
		await expect(page.locator("[data-version-update-notice]")).toBeVisible({
			timeout: 15_000,
		});
		await expect(page.getByText(/新版本|更新/).first()).toBeVisible();
		await expect(page.locator("[data-pwa-install-action]")).toBeVisible();

		await page.goto(
			"/focus/repo/octo-demo/release-lab?demo=app-shell&d_shell=resource-update&d_controls=hidden",
		);
		await expect(page.locator("[data-version-update-message]")).toHaveText(
			"应用资源更新已准备好，刷新后完成切换",
		);
		await expect(page.getByRole("button", { name: "刷新" })).toBeVisible();

		await page.goto(
			"/focus/repo/octo-demo/release-lab?demo=app-shell&d_shell=unknown&d_controls=hidden",
		);
		await expect(page.getByText("Version unknown")).toBeVisible({
			timeout: 15_000,
		});
	});
});
