import { isTauri } from "@tauri-apps/api/core";
import { type Platform, platform } from "@tauri-apps/plugin-os";

export const platformNames = {
	android: "Android",
	linux: "Linux",
	macos: "macOS",
	windows: "Windows",
} as const satisfies Partial<Record<Platform, string>>;

export function currentPlatform(): Platform | "web" {
	return isTauri() ? platform() : "web";
}

export function isMobilePlatform(): boolean {
	return isTauri() && ["android", "ios"].includes(platform());
}

export function isDesktopPlatform(): boolean {
	return isTauri() && ["linux", "macos", "windows"].includes(platform());
}

export function isAndroidPlatform(): boolean {
	return isTauri() && platform() === "android";
}

export function isLinuxPlatform(): boolean {
	return isTauri() && platform() === "linux";
}

export function isMacosPlatform(): boolean {
	return isTauri() && platform() === "macos";
}

/** Served by og-server as a web app (its bridge stands in for Tauri). */
export function isWebBuild(): boolean {
	return (
		typeof window !== "undefined" &&
		(window as { __OG_WEB__?: boolean }).__OG_WEB__ === true
	);
}
