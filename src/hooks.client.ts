import type { ClientInit, HandleClientError } from "@sveltejs/kit/hooks";

import { enableAutoRefreshAtTop } from "#lib/components/feedback/refresh/auto-refresh.ts";
import { ws } from "#lib/ws.svelte.ts";

export const init: ClientInit = () => {
	enableAutoRefreshAtTop();
	ws.connect();
};

export const handleError: HandleClientError = ({ error, event }) => {
	console.error("Error during request to", event.url.pathname, ":", error);
};
