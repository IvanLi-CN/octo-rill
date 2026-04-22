import type { Page } from "@playwright/test";

export async function installPasskeyBrowserMock(page: Page) {
	await page.addInitScript(() => {
		class FakePublicKeyCredential {
			id: string;
			type: PublicKeyCredentialType;
			rawId: ArrayBuffer;
			response:
				| AuthenticatorAttestationResponse
				| AuthenticatorAssertionResponse;

			constructor(payload: {
				id: string;
				rawId: ArrayBuffer;
				response:
					| AuthenticatorAttestationResponse
					| AuthenticatorAssertionResponse;
			}) {
				this.id = payload.id;
				this.type = "public-key";
				this.rawId = payload.rawId;
				this.response = payload.response;
			}

			getClientExtensionResults() {
				return {};
			}

			static async isConditionalMediationAvailable() {
				return true;
			}
		}

		const bytes = (...values: number[]) => Uint8Array.from(values).buffer;

		const createCredential = () =>
			new FakePublicKeyCredential({
				id: "passkey-registration-test",
				rawId: bytes(1, 2, 3, 4),
				response: {
					attestationObject: bytes(11, 12, 13, 14),
					clientDataJSON: bytes(21, 22, 23, 24),
					getTransports: () => ["internal"],
				} as AuthenticatorAttestationResponse,
			});

		const getCredential = () =>
			new FakePublicKeyCredential({
				id: "passkey-authentication-test",
				rawId: bytes(5, 6, 7, 8),
				response: {
					authenticatorData: bytes(31, 32, 33, 34),
					clientDataJSON: bytes(41, 42, 43, 44),
					signature: bytes(51, 52, 53, 54),
					userHandle: bytes(61, 62, 63, 64),
				} as AuthenticatorAssertionResponse,
			});

		Object.defineProperty(window, "PublicKeyCredential", {
			configurable: true,
			writable: true,
			value: FakePublicKeyCredential,
		});

		const existingCredentials = navigator.credentials ?? {};
		Object.defineProperty(navigator, "credentials", {
			configurable: true,
			value: {
				...existingCredentials,
				create: async () => createCredential(),
				get: async () => getCredential(),
			},
		});
	});
}

export async function installPasskeyUnsupportedBrowser(page: Page) {
	await page.addInitScript(() => {
		Object.defineProperty(window, "PublicKeyCredential", {
			configurable: true,
			writable: true,
			value: undefined,
		});

		const existingCredentials = navigator.credentials ?? {};
		Object.defineProperty(navigator, "credentials", {
			configurable: true,
			value: {
				...existingCredentials,
				create: undefined,
				get: undefined,
			},
		});
	});
}
