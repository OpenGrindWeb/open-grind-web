<script lang="ts">
	import { onNavigate } from "$app/navigation";
	import { prefersReducedMotion } from "svelte/motion";

	import { tabDirection } from "./tab-transition.ts";

	const START_TIMEOUT_MS = 250;

	// Switching tabs slides the screen sideways, toward the tab picked, while
	// the tab bar stays put. Uses same-document view transitions (Safari 18+);
	// without them, or with reduced motion, the switch stays instant.
	onNavigate((navigation) => {
		const direction = tabDirection(
			navigation.from?.url.pathname,
			navigation.to?.url.pathname,
		);
		if (direction === null || prefersReducedMotion.current) return;
		if (typeof document.startViewTransition !== "function") return;

		const root = document.documentElement;
		root.dataset.tabTransition = direction;
		return new Promise<void>((resolve) => {
			let started = false;
			const transition = document.startViewTransition(async () => {
				started = true;
				resolve();
				await navigation.complete;
			});
			// A page that isn't painting (backgrounded) never starts the
			// transition: give up on it rather than hold the navigation.
			setTimeout(() => {
				if (started) return;
				transition.skipTransition();
				resolve();
			}, START_TIMEOUT_MS);
			void transition.finished.finally(() => {
				delete root.dataset.tabTransition;
			});
		});
	});
</script>

<style>
	:global(nav[aria-label="Main"]) {
		view-transition-name: og-tab-bar;
	}

	:global(::view-transition-group(og-tab-bar)) {
		animation: none;
	}

	/* the active tab's highlight slides from the old tab to the new one */
	:global(::view-transition-group(og-tab-indicator)) {
		animation-duration: 0.42s;
		animation-timing-function: cubic-bezier(0.34, 1.3, 0.64, 1);
	}

	:global(:root[data-tab-transition]::view-transition-old(root)),
	:global(:root[data-tab-transition]::view-transition-new(root)) {
		animation-duration: 0.34s;
		animation-timing-function: cubic-bezier(0.32, 0.72, 0, 1);
		animation-fill-mode: both;
		mix-blend-mode: normal;
	}

	:global(:root[data-tab-transition="forward"]::view-transition-old(root)) {
		animation-name: og-tab-out-left;
	}
	:global(:root[data-tab-transition="forward"]::view-transition-new(root)) {
		animation-name: og-tab-in-right;
	}
	:global(:root[data-tab-transition="back"]::view-transition-old(root)) {
		animation-name: og-tab-out-right;
	}
	:global(:root[data-tab-transition="back"]::view-transition-new(root)) {
		animation-name: og-tab-in-left;
	}

	@keyframes -global-og-tab-out-left {
		to {
			transform: translateX(-28%);
			opacity: 0;
		}
	}
	@keyframes -global-og-tab-in-right {
		from {
			transform: translateX(28%);
			opacity: 0;
		}
	}
	@keyframes -global-og-tab-out-right {
		to {
			transform: translateX(28%);
			opacity: 0;
		}
	}
	@keyframes -global-og-tab-in-left {
		from {
			transform: translateX(-28%);
			opacity: 0;
		}
	}
</style>
