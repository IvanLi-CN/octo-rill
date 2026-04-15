import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import {
	readCargoPackageVersion,
	resolveEmbeddedAppVersion,
} from "../config/embeddedVersion";

function withTempRepo(
	setup: (repoRoot: string) => void,
	assertions: (repoRoot: string) => void,
): void {
	const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), "embedded-version-"));

	try {
		setup(repoRoot);
		assertions(repoRoot);
	} finally {
		fs.rmSync(repoRoot, { force: true, recursive: true });
	}
}

withTempRepo(
	(repoRoot) => {
		fs.writeFileSync(
			path.join(repoRoot, "Cargo.toml"),
			`[package]
name = "octo-rill"
version = "1.2.3"
`,
		);
	},
	(repoRoot) => {
		assert.equal(
			resolveEmbeddedAppVersion(" 9.9.9 ", readCargoPackageVersion(repoRoot)),
			"9.9.9",
		);
	},
);

withTempRepo(
	(repoRoot) => {
		fs.writeFileSync(
			path.join(repoRoot, "Cargo.toml"),
			`[package]
name = "octo-rill"
version = "2.3.4"
`,
		);
	},
	(repoRoot) => {
		assert.equal(
			resolveEmbeddedAppVersion("", readCargoPackageVersion(repoRoot)),
			"2.3.4",
		);
	},
);

withTempRepo(
	() => {},
	(repoRoot) => {
		assert.doesNotThrow(() => readCargoPackageVersion(repoRoot));
		assert.equal(
			resolveEmbeddedAppVersion(undefined, readCargoPackageVersion(repoRoot)),
			"unknown",
		);
	},
);

console.log("embedded version contract tests passed");
