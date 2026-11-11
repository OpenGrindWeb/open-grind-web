<script lang="ts">
	import { page } from "$app/state";
	import ChatCircleIcon from "phosphor-svelte/lib/ChatCircleIcon";
	import DotsNineIcon from "phosphor-svelte/lib/DotsNineIcon";
	import DropIcon from "phosphor-svelte/lib/DropIcon";
	import FireIcon from "phosphor-svelte/lib/FireIcon";
	import { untrack } from "svelte";

	import { getOrCreateConversationsState } from "#lib/chat/conversations-context.svelte.ts";
	import ProgressiveBlur from "#lib/components/shared/ProgressiveBlur.svelte";
	import { Badge } from "#lib/components/ui/badge/index.ts";
	import { tabsListVariants } from "#lib/components/ui/tabs/index.ts";
	import { getTapsState } from "#lib/interest/taps-state.svelte.ts";
	import { bottomChrome } from "#lib/util/screen-chrome.svelte.ts";
	import { tabNavigation } from "./tab-navigation.ts";

	let { ourProfileId }: { ourProfileId: number } = $props();

	const conversations = untrack(() =>
		getOrCreateConversationsState(ourProfileId),
	);
	const hasUnread = $derived(conversations.hasUnread);

	const taps = untrack(() => getTapsState(ourProfileId));
	const hasUnseenTaps = $derived(taps.hasUnseen);

	// One highlight behind the active tab, which slides to the next one
	// instead of jumping. It follows the active link's box.
	let links: HTMLDivElement | undefined = $state();
	let indicator = $state<{ left: number; width: number } | null>(null);
	function placeIndicator() {
		const active = [...(links?.children ?? [])].find(
			(child): child is HTMLAnchorElement =>
				child instanceof HTMLAnchorElement &&
				child.dataset.active === "true",
		);
		indicator = active
			? { left: active.offsetLeft, width: active.offsetWidth }
			: null;
	}
	$effect(() => {
		void page.route.id;
		placeIndicator();
	});
	$effect(() => {
		if (!links) return;
		const resize = new ResizeObserver(placeIndicator);
		resize.observe(links);
		return () => resize.disconnect();
	});
</script>

{#snippet label(text: string)}
	<span class="label" data-label={text}>{text}</span>
{/snippet}

<ProgressiveBlur
	direction="bottomToTop"
	tag="nav"
	aria-label="Main"
	class="fixed bottom-0 z-50 w-full pt-2 pb-fixed-nav"
	bgClass="bg-linear-to-t from-background to-transparent"
	contentClass="flex gap-2 overflow-auto no-scrollbar px-1 *:first:ms-auto *:last:me-auto"
	contentScrollIntent="x"
	{@attach bottomChrome}
>
	<div
		bind:this={links}
		class={[
			tabsListVariants({ variant: "default" }),
			"links relative shrink-0 [&>a>svg]:size-5!",
		]}
	>
		{#if indicator}
			<span
				aria-hidden="true"
				data-slot="tab-indicator"
				class="tab-indicator"
				style:transform="translateX({indicator.left}px)"
				style:width="{indicator.width}px"
			></span>
		{/if}
		<a
			href="/"
			data-active={page.route.id === "/(protected)/(navbar)/(root)"}
			onclick={tabNavigation({ href: "/" })}
		>
			<DotsNineIcon weight="fill" />
			{@render label("Browse")}
		</a>
		<a
			href="/right-now"
			data-active={page.route.id === "/(protected)/(navbar)/right-now"}
			onclick={tabNavigation({ href: "/right-now" })}
		>
			<DropIcon weight="fill" />
			{@render label("Right Now")}
		</a>
		<a
			href="/interest"
			data-active={page.route.id?.startsWith(
				"/(protected)/(navbar)/interest",
			)}
			onclick={tabNavigation({
				href: "/interest",
				landsOn: "/interest/taps",
			})}
		>
			<FireIcon weight="fill" />
			{@render label("Interest")}
			{#if hasUnseenTaps}
				<Badge
					class="absolute inset-e-2 top-1 size-2.5 rounded-full p-0"
				/>
			{/if}
		</a>
		<a
			href="/chat"
			data-active={page.route.id === "/(protected)/chat"}
			onclick={tabNavigation({ href: "/chat" })}
		>
			<ChatCircleIcon weight="fill" />
			{@render label("Inbox")}
			{#if hasUnread}
				<Badge
					class="absolute inset-e-2 top-1 size-2.5 rounded-full p-0"
				/>
			{/if}
		</a>
	</div>
</ProgressiveBlur>

<style lang="postcss">
	@reference "$layout";

	.links a {
		@apply relative inline-flex h-[calc(100%-1px)] flex-1 flex-col items-center justify-center gap-0.5 rounded-full border border-transparent! px-3 py-1 text-xs whitespace-nowrap text-foreground/60 transition-colors group-data-vertical/tabs:rounded-2xl group-data-vertical/tabs:px-3 group-data-vertical/tabs:py-1.5 hover:bg-input/20 focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-1 focus-visible:outline-ring disabled:pointer-events-none disabled:opacity-50 motion-reduce:transition-none dark:text-muted-foreground dark:hover:bg-input/20 data-active:font-medium data-active:text-foreground dark:data-active:border-input dark:data-active:text-accent [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4;
	}

	.label {
		@apply flex flex-col items-center;
	}

	.links a {
		z-index: 1;
	}

	.tab-indicator {
		position: absolute;
		top: 0.25rem;
		bottom: 0.25rem;
		left: 0;
		border-radius: 9999px;
		background: rgb(255 255 255 / 0.12);
		pointer-events: none;
		/* lets a tab switch's view transition slide it between snapshots */
		view-transition-name: og-tab-indicator;
		transition:
			transform 0.42s cubic-bezier(0.34, 1.3, 0.64, 1),
			width 0.42s cubic-bezier(0.34, 1.3, 0.64, 1);
	}

	/* during a view transition the transition itself moves it */
	:global(:root[data-tab-transition]) .tab-indicator {
		transition: none;
	}

	@media (prefers-reduced-motion: reduce) {
		.tab-indicator {
			transition: none;
		}
	}

	.label::after {
		@apply invisible h-0 overflow-hidden font-medium select-none;
		content: attr(data-label);
	}
</style>
