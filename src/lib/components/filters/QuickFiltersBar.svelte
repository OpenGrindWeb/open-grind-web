<script lang="ts">
	import OverflowMenu from "#lib/components/filters/overflow/OverflowMenu.svelte";
	import { provideOverflowBar } from "#lib/components/filters/overflow/overflow-bar.svelte.ts";
	import ProgressiveBlur from "#lib/components/shared/ProgressiveBlur.svelte";
	import { topChrome } from "#lib/util/screen-chrome.svelte.ts";

	let { children }: { children: import("svelte").Snippet } = $props();

	// Filters that don't fit move into a "…" menu instead of scrolling.
	const bar = provideOverflowBar();
</script>

<ProgressiveBlur
	data-fixed-header
	class="fixed top-0 left-0 z-10 w-full"
	bgClass="bg-linear-to-b from-background to-transparent"
	contentClass="flex flex-col pt-fixed-header"
	direction="topToBottom"
	{@attach topChrome}
>
	<div
		data-slot="quick-filters-row"
		class="flex items-center gap-1.5 overflow-hidden px-3 pb-2"
		{@attach bar.row}
	>
		{@render children()}
		<OverflowMenu {bar} />
	</div>
</ProgressiveBlur>
