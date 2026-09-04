import { summarizePlaywrightReport } from "./summarize-playwright-results";

function assertEqual(actual: unknown, expected: unknown, label: string): void {
	if (actual !== expected)
		throw new Error(`${label}: expected ${expected}, got ${actual}`);
}

const clean = summarizePlaywrightReport({
	suites: [
		{
			specs: [
				{
					tests: [
						{ expectedStatus: "passed", results: [{ status: "passed" }] },
					],
				},
				{ tests: [{ expectedStatus: "skipped", results: [] }] },
			],
		},
	],
});
assertEqual(clean.total_tests, 2, "clean total");
assertEqual(clean.passed_tests, 1, "clean passed");
assertEqual(clean.skipped_tests, 1, "clean skipped");
assertEqual(clean.failed_tests, 0, "clean failed");
assertEqual(clean.flaky_tests, 0, "clean flaky");
assertEqual(clean.retry_count, 0, "clean retries");

const flaky = summarizePlaywrightReport({
	suites: [
		{
			specs: [
				{
					tests: [
						{
							expectedStatus: "passed",
							results: [{ status: "failed" }, { status: "passed" }],
						},
					],
				},
			],
		},
	],
});
assertEqual(flaky.total_tests, 1, "flaky total");
assertEqual(flaky.passed_tests, 1, "flaky passed");
assertEqual(flaky.failed_tests, 0, "flaky failed");
assertEqual(flaky.flaky_tests, 1, "flaky count");
assertEqual(flaky.retry_count, 1, "flaky retries");

const finalFailure = summarizePlaywrightReport({
	suites: [
		{
			suites: [
				{
					specs: [
						{
							tests: [
								{
									expectedStatus: "passed",
									results: [{ status: "failed" }, { status: "failed" }],
								},
							],
						},
					],
				},
			],
		},
	],
});
assertEqual(finalFailure.total_tests, 1, "failure total");
assertEqual(finalFailure.passed_tests, 0, "failure passed");
assertEqual(finalFailure.failed_tests, 1, "failure failed");
assertEqual(finalFailure.flaky_tests, 0, "failure flaky");
assertEqual(finalFailure.retry_count, 1, "failure retries");

console.log("test-summarize-playwright-results: all checks passed");
