/** The tab-bar screens, in bar order (Me last). */
const TABS: ((pathname: string) => boolean)[] = [
	(path) => path === "/",
	(path) => path === "/right-now",
	(path) => path === "/interest" || path.startsWith("/interest/"),
	(path) => path === "/chat",
	(path) => path === "/settings",
];

/** Index of the tab a pathname is the root screen of, or null. */
export function tabIndexOf(pathname: string | undefined): number | null {
	if (pathname === undefined) return null;
	const index = TABS.findIndex((isTab) => isTab(pathname));
	return index === -1 ? null : index;
}

export type TabDirection = "forward" | "back";

/**
 * Which way a navigation moves along the tab bar, or null when it isn't a
 * switch between two different tabs (a push, a tab's own sub-page, ...).
 */
export function tabDirection(
	from: string | undefined,
	to: string | undefined,
): TabDirection | null {
	const start = tabIndexOf(from);
	const end = tabIndexOf(to);
	if (start === null || end === null || start === end) return null;
	return end > start ? "forward" : "back";
}
