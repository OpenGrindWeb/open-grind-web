import type { MessageKey } from "#lib/i18n/messages/en.ts";

export const INTEREST_TABS: { href: string; label: string; key: MessageKey }[] = [
	{ href: "/interest/views", label: "Views", key: "interest.views" },
	{ href: "/interest/taps", label: "Taps", key: "interest.taps" },
];

export function interestTabIndex(pathname: string): number {
	return Math.max(
		0,
		INTEREST_TABS.findIndex(({ href }) => href === pathname),
	);
}
