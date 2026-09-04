import { mkdir, readFile, writeFile } from "node:fs/promises";
import { dirname } from "node:path";

export interface PlaywrightSummary {
	schema_version: 1;
	total_tests: number;
	passed_tests: number;
	failed_tests: number;
	skipped_tests: number;
	flaky_tests: number;
	retry_count: number;
	collection_error?: string;
}

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
	return typeof value === "object" && value !== null;
}

function testEntries(report: unknown): UnknownRecord[] {
	const entries: UnknownRecord[] = [];

	function visitSuite(value: unknown): void {
		if (!isRecord(value)) return;
		const specs = Array.isArray(value.specs) ? value.specs : [];
		for (const spec of specs) {
			if (!isRecord(spec)) continue;
			const tests = Array.isArray(spec.tests) ? spec.tests : [];
			for (const test of tests) {
				if (isRecord(test)) entries.push(test);
			}
		}
		const nestedSuites = Array.isArray(value.suites) ? value.suites : [];
		for (const suite of nestedSuites) visitSuite(suite);
	}

	const suites =
		isRecord(report) && Array.isArray(report.suites) ? report.suites : [];
	for (const suite of suites) visitSuite(suite);
	return entries;
}

export function summarizePlaywrightReport(report: unknown): PlaywrightSummary {
	const summary: PlaywrightSummary = {
		schema_version: 1,
		total_tests: 0,
		passed_tests: 0,
		failed_tests: 0,
		skipped_tests: 0,
		flaky_tests: 0,
		retry_count: 0,
	};

	for (const test of testEntries(report)) {
		const results = Array.isArray(test.results)
			? test.results.filter(isRecord)
			: [];
		const finalResult = results.at(-1);
		const finalStatus = finalResult?.status;
		const expectedStatus = test.expectedStatus;
		const skipped =
			finalStatus === "skipped" ||
			(results.length === 0 && expectedStatus === "skipped");

		summary.total_tests += 1;
		summary.retry_count += Math.max(0, results.length - 1);
		if (skipped) {
			summary.skipped_tests += 1;
			continue;
		}
		if (finalStatus === "passed") {
			summary.passed_tests += 1;
			if (results.length > 1) summary.flaky_tests += 1;
			continue;
		}
		summary.failed_tests += 1;
	}

	return summary;
}

function markdownSummary(summary: PlaywrightSummary): string {
	const error = summary.collection_error
		? `\n\n> Report collection error: ${summary.collection_error}`
		: "";
	return [
		"## Frontend E2E results",
		"",
		"| Metric | Count |",
		"| --- | ---: |",
		`| Total tests | ${summary.total_tests} |`,
		`| Final passed | ${summary.passed_tests} |`,
		`| Final failed | ${summary.failed_tests} |`,
		`| Skipped | ${summary.skipped_tests} |`,
		`| Flaky | ${summary.flaky_tests} |`,
		`| Retries | ${summary.retry_count} |`,
		`${error}`,
		"",
	].join("\n");
}

export async function writePlaywrightSummary(
	inputPath: string,
	outputPath: string,
	stepSummaryPath = process.env.GITHUB_STEP_SUMMARY,
): Promise<PlaywrightSummary> {
	let summary: PlaywrightSummary;
	try {
		const report = JSON.parse(await readFile(inputPath, "utf8")) as unknown;
		summary = summarizePlaywrightReport(report);
	} catch (error) {
		summary = {
			schema_version: 1,
			total_tests: 0,
			passed_tests: 0,
			failed_tests: 0,
			skipped_tests: 0,
			flaky_tests: 0,
			retry_count: 0,
			collection_error: error instanceof Error ? error.message : String(error),
		};
	}

	await mkdir(dirname(outputPath), { recursive: true });
	await writeFile(outputPath, `${JSON.stringify(summary, null, 2)}\n`, "utf8");
	if (stepSummaryPath) {
		await mkdir(dirname(stepSummaryPath), { recursive: true });
		await writeFile(stepSummaryPath, markdownSummary(summary), {
			encoding: "utf8",
			flag: "a",
		});
	}
	return summary;
}

async function main(): Promise<void> {
	const inputPath = process.argv[2] ?? "test-results/playwright-results.json";
	const outputPath = process.argv[3] ?? "test-results/playwright-summary.json";
	const summary = await writePlaywrightSummary(inputPath, outputPath);
	if (summary.collection_error) {
		console.error(
			`Playwright report summary failed: ${summary.collection_error}`,
		);
		process.exitCode = 1;
	}
}

if (import.meta.main) await main();
