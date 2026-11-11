/**
 * Where a list can't be pulled (a mouse wheel or the keyboard reached its
 * top), upstream offers a Refresh button. With `atTop` on, the list refreshes
 * right away instead, once per visit to the top. Touch keeps pull-to-refresh.
 *
 * Off by default (and so in component tests); the app turns it on at start.
 */
export const autoRefresh = { atTop: false };

export function enableAutoRefreshAtTop(): void {
	autoRefresh.atTop = true;
}
