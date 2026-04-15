import fs from "node:fs";
import path from "node:path";

const CARGO_PACKAGE_VERSION_PATTERN =
	/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m;

function normalizeVersionCandidate(
	value: string | null | undefined,
): string | null {
	const normalized = value?.trim();
	return normalized ? normalized : null;
}

export function extractCargoPackageVersion(cargoToml: string): string | null {
	const cargoVersionMatch = cargoToml.match(CARGO_PACKAGE_VERSION_PATTERN);
	return normalizeVersionCandidate(cargoVersionMatch?.[1]);
}

export function readCargoPackageVersion(repoRoot: string): string | null {
	const cargoTomlPath = path.resolve(repoRoot, "Cargo.toml");

	try {
		return extractCargoPackageVersion(fs.readFileSync(cargoTomlPath, "utf8"));
	} catch {
		return null;
	}
}

export function resolveEmbeddedAppVersion(
	envValue: string | null | undefined,
	cargoVersion: string | null | undefined,
): string {
	return (
		normalizeVersionCandidate(envValue) ??
		normalizeVersionCandidate(cargoVersion) ??
		"unknown"
	);
}
