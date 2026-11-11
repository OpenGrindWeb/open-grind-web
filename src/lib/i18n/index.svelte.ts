import { de } from "./messages/de.ts";
import { en, type MessageKey } from "./messages/en.ts";
import { es } from "./messages/es.ts";
import { fr } from "./messages/fr.ts";

/**
 * Interface language. English is the source and the fallback: a screen not
 * translated yet keeps its inline English strings, and every other language
 * must define every key (the catalogs are typed against `en`).
 */
export const LOCALES = ["en", "fr", "es", "de"] as const;
export type Locale = (typeof LOCALES)[number];

export const LOCALE_NAMES: Record<Locale, string> = {
	en: "English",
	fr: "Français",
	es: "Español",
	de: "Deutsch",
};

const CATALOGS: Record<Locale, Record<MessageKey, string>> = { en, fr, es, de };
const STORAGE_KEY = "open-grind:locale";

function isLocale(value: unknown): value is Locale {
	return LOCALES.includes(value as Locale);
}

function stored(): Locale | null {
	try {
		const value = localStorage.getItem(STORAGE_KEY);
		return isLocale(value) ? value : null;
	} catch {
		return null;
	}
}

function fromDevice(): Locale {
	const languages =
		typeof navigator === "undefined" ? [] : (navigator.languages ?? []);
	for (const tag of languages) {
		const language = tag.split("-")[0]?.toLowerCase();
		if (isLocale(language)) return language;
	}
	return "en";
}

const initial: Locale = stored() ?? fromDevice();
let current = $state<Locale>(initial);

function applyToDocument(locale: Locale) {
	if (typeof document !== "undefined") document.documentElement.lang = locale;
}
applyToDocument(initial);

export function locale(): Locale {
	return current;
}

export function setLocale(next: Locale): void {
	current = next;
	applyToDocument(next);
	try {
		localStorage.setItem(STORAGE_KEY, next);
	} catch {
		// private mode: the choice lasts for this session only
	}
}

/**
 * The text for `key` in the current language, with `{name}` placeholders
 * filled from `params`. Reactive: reading it in a template re-renders on a
 * language change.
 */
export function t(
	key: MessageKey,
	params?: Record<string, string | number>,
): string {
	const text = CATALOGS[current][key] ?? en[key];
	if (!params) return text;
	return text.replace(/\{(\w+)\}/g, (match, name: string) =>
		name in params ? String(params[name]) : match,
	);
}
