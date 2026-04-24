import { ApiError } from "@/api";
import type {
	PasskeyCreationOptionsJSON,
	PasskeyCredentialDescriptorJSON,
	PasskeyRequestOptionsJSON,
} from "@/api";

type PasskeyMediation = "conditional" | "required" | "optional" | "silent";

function base64UrlToBuffer(value: string): ArrayBuffer {
	const normalized = value.replace(/-/g, "+").replace(/_/g, "/");
	const padded = normalized.padEnd(
		normalized.length + ((4 - (normalized.length % 4)) % 4),
		"=",
	);
	const binary = atob(padded);
	const bytes = new Uint8Array(binary.length);
	for (let index = 0; index < binary.length; index += 1) {
		bytes[index] = binary.charCodeAt(index);
	}
	return bytes.buffer;
}

function bufferToBase64Url(value: ArrayBuffer | ArrayBufferView): string {
	const bytes =
		value instanceof ArrayBuffer
			? new Uint8Array(value)
			: new Uint8Array(value.buffer, value.byteOffset, value.byteLength);
	let binary = "";
	for (const byte of bytes) {
		binary += String.fromCharCode(byte);
	}
	return btoa(binary)
		.replace(/\+/g, "-")
		.replace(/\//g, "_")
		.replace(/=+$/g, "");
}

function mapDescriptor(
	descriptor: PasskeyCredentialDescriptorJSON,
): PublicKeyCredentialDescriptor {
	return {
		type: descriptor.type,
		id: base64UrlToBuffer(descriptor.id),
		transports: descriptor.transports,
	};
}

function creationOptionsFromJson(
	options: PasskeyCreationOptionsJSON,
): CredentialCreationOptions {
	return {
		publicKey: {
			...options.publicKey,
			challenge: base64UrlToBuffer(options.publicKey.challenge),
			user: {
				...options.publicKey.user,
				id: base64UrlToBuffer(options.publicKey.user.id),
			},
			excludeCredentials:
				options.publicKey.excludeCredentials?.map(mapDescriptor),
		},
	};
}

function requestOptionsFromJson(
	options: PasskeyRequestOptionsJSON,
	mediation: PasskeyMediation,
): CredentialRequestOptions {
	return {
		publicKey: {
			...options.publicKey,
			challenge: base64UrlToBuffer(options.publicKey.challenge),
			allowCredentials: options.publicKey.allowCredentials.map(mapDescriptor),
		},
		mediation: mediation as CredentialMediationRequirement,
	};
}

function serializeRegistrationCredential(credential: PublicKeyCredential) {
	const response = credential.response as AuthenticatorAttestationResponse & {
		getTransports?: () => AuthenticatorTransport[];
	};
	return {
		id: credential.id,
		rawId: bufferToBase64Url(credential.rawId),
		type: credential.type,
		response: {
			attestationObject: bufferToBase64Url(response.attestationObject),
			clientDataJSON: bufferToBase64Url(response.clientDataJSON),
			transports: response.getTransports?.(),
		},
		clientExtensionResults: credential.getClientExtensionResults(),
	};
}

function serializeAuthenticationCredential(credential: PublicKeyCredential) {
	const response = credential.response as AuthenticatorAssertionResponse;
	return {
		id: credential.id,
		rawId: bufferToBase64Url(credential.rawId),
		type: credential.type,
		response: {
			authenticatorData: bufferToBase64Url(response.authenticatorData),
			clientDataJSON: bufferToBase64Url(response.clientDataJSON),
			signature: bufferToBase64Url(response.signature),
			userHandle: response.userHandle
				? bufferToBase64Url(response.userHandle)
				: null,
		},
		clientExtensionResults: credential.getClientExtensionResults(),
	};
}

export function browserSupportsPasskeys(): boolean {
	return (
		typeof window !== "undefined" &&
		typeof window.PublicKeyCredential !== "undefined" &&
		typeof navigator.credentials?.create === "function" &&
		typeof navigator.credentials?.get === "function"
	);
}

export async function browserSupportsConditionalMediation(): Promise<boolean> {
	if (!browserSupportsPasskeys()) return false;
	const publicKeyCredentialCtor =
		PublicKeyCredential as typeof PublicKeyCredential & {
			isConditionalMediationAvailable?: () => Promise<boolean>;
		};
	if (
		typeof publicKeyCredentialCtor.isConditionalMediationAvailable !==
		"function"
	) {
		return false;
	}
	try {
		return await publicKeyCredentialCtor.isConditionalMediationAvailable();
	} catch {
		return false;
	}
}

export async function createPasskeyCredential(
	options: PasskeyCreationOptionsJSON,
): Promise<unknown> {
	const credential = await navigator.credentials.create(
		creationOptionsFromJson(options),
	);
	if (!(credential instanceof PublicKeyCredential)) {
		throw new Error("浏览器没有返回可用的 Passkey 注册结果。");
	}
	return serializeRegistrationCredential(credential);
}

export async function getPasskeyCredential(
	options: PasskeyRequestOptionsJSON,
	mediation: PasskeyMediation = "required",
): Promise<unknown> {
	const credential = await navigator.credentials.get(
		requestOptionsFromJson(options, mediation),
	);
	if (!(credential instanceof PublicKeyCredential)) {
		throw new Error("浏览器没有返回可用的 Passkey 登录结果。");
	}
	return serializeAuthenticationCredential(credential);
}

export function normalizePasskeyErrorMessage(error: unknown): string {
	if (error instanceof ApiError) {
		switch (error.code) {
			case "passkey_registration_expired":
			case "passkey_authentication_expired":
				return "这次 Passkey 操作已经过期，请重新试一次。";
			case "passkey_registration_missing":
			case "passkey_authentication_missing":
				return "Passkey 状态已经失效，请重新开始。";
			case "passkey_already_bound":
				return "这把 Passkey 已经绑定到其他账号，请改用已有账号登录。";
			case "passkey_retry_required":
				return "当前账号需要在设置页里重新添加这把 Passkey。";
			case "passkey_github_required":
				return "当前账号还没有 GitHub 绑定，先完成 GitHub 绑定后才能使用 Passkey。";
			case "passkey_not_found":
			case "passkey_user_not_found":
			case "passkey_authentication_failed":
				return "没有找到可用的 Passkey 登录结果，请确认你选择了正确的设备或账号。";
			case "passkey_registration_failed":
				return "Passkey 注册没有完成，请在浏览器里重试一次。";
		}
		return error.message;
	}

	if (error instanceof DOMException) {
		switch (error.name) {
			case "AbortError":
				return "Passkey 操作已取消。";
			case "NotAllowedError":
				return "你取消了 Passkey 操作，或浏览器没有允许这次验证。";
			case "InvalidStateError":
				return "这把 Passkey 似乎已经存在，换一个设备或改用已有账号登录试试。";
			case "SecurityError":
				return "当前页面不满足浏览器对 Passkey 的安全上下文要求。";
			default:
				return error.message || "Passkey 操作失败，请稍后再试。";
		}
	}

	if (error instanceof Error) {
		return error.message;
	}
	return "Passkey 操作失败，请稍后再试。";
}
