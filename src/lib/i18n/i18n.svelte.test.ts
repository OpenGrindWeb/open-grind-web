import { afterEach, describe, expect, it } from "vitest";

import { LOCALES, locale, setLocale, t } from "./index.svelte.ts";
import { de } from "./messages/de.ts";
import { en, type MessageKey } from "./messages/en.ts";
import { es } from "./messages/es.ts";
import { fr } from "./messages/fr.ts";

afterEach(() => setLocale("en"));

describe("i18n", () => {
	it("starts in English when the device says English", () => {
		expect(locale()).toBe("en");
		expect(t("nav.inbox")).toBe("Inbox");
	});

	it("switches language and remembers it", () => {
		setLocale("fr");
		expect(t("nav.inbox")).toBe("Messages");
		expect(document.documentElement.lang).toBe("fr");
		expect(localStorage.getItem("open-grind:locale")).toBe("fr");
	});

	it("fills placeholders and leaves unknown ones alone", () => {
		setLocale("de");
		expect(t("rightNow.joinCount", { count: 2 })).toBe("Beitreten · 2 gratis");
		expect(t("filters.sort")).toBe("Sortierung: {sort}");
	});

	it("translates every key in every language, keeping placeholders", () => {
		const placeholders = (text: string) =>
			[...text.matchAll(/\{(\w+)\}/g)].map((match) => match[1]).sort();
		for (const catalog of [fr, es, de]) {
			for (const key of Object.keys(en) as MessageKey[]) {
				expect(catalog[key].trim(), key).not.toBe("");
				expect(placeholders(catalog[key]), key).toEqual(placeholders(en[key]));
			}
		}
		expect(LOCALES).toEqual(["en", "fr", "es", "de"]);
	});
});
