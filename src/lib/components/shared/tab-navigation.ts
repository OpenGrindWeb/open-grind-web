import { page } from "$app/state";

import { returnTo } from "#lib/util/history.ts";
import { isWithin } from "#lib/util/pathname.ts";
import { isPlainClick } from "#lib/util/plain-click.ts";

/**
 * Click handler for a link to a top-level screen (a tab, or Me): a plain
 * click on the screen already shown does nothing, and a click from inside
 * that screen's own pages goes back to it instead of stacking a new entry.
 */
export function tabNavigation({
	href,
	landsOn = href,
}: {
	href: string;
	landsOn?: string;
}) {
	return (event: MouseEvent) => {
		if (!isPlainClick(event)) return;

		const current = page.url.pathname;
		if (current === landsOn) {
			event.preventDefault();
			return;
		}
		if (!isWithin({ pathname: current, root: href })) return;

		event.preventDefault();
		returnTo(landsOn);
	};
}
