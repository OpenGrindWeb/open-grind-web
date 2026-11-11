<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import StarIcon from "phosphor-svelte/lib/StarIcon";

	import OverflowMenu from "#lib/components/filters/overflow/OverflowMenu.svelte";
	import {
		overflowItem,
		provideOverflowBar,
	} from "#lib/components/filters/overflow/overflow-bar.svelte.ts";
	import QuickFilterButton from "#lib/components/filters/QuickFilterButton.svelte";
	import ProgressiveBlur from "#lib/components/shared/ProgressiveBlur.svelte";
	import { buttonVariants } from "#lib/components/ui/button/index.ts";
	import * as ToggleGroup from "#lib/components/ui/toggle-group/index.ts";
	import { topChrome } from "#lib/util/screen-chrome.svelte.ts";
	import type { ConversationFilters } from "#lib/chat/conversation-filters.svelte.ts";
	import type { ConversationFilterValues } from "#lib/model/messaging/conversation-filters.ts";
	import DistanceQuickFilter from "./DistanceQuickFilter.svelte";
	import PositionQuickFilter from "./PositionQuickFilter.svelte";

	let {
		filters,
		onchange,
		inert = false,
		locked = false,
	}: {
		filters: ConversationFilters;
		onchange: (values: Partial<ConversationFilterValues>) => void;
		inert?: boolean;
		locked?: boolean;
	} = $props();

	const LABELLED_TOGGLES = [
		{ key: "unread", label: "filters.unread" },
		{ key: "online", label: "filters.online" },
		{ key: "rightNow", label: "filters.rightNow" },
	] as const;

	const values = $derived(filters.value);

	let open = $state({ distance: false, position: false });

	// Filters that don't fit move into a "…" menu instead of scrolling.
	const bar = provideOverflowBar();
	const favoritesOverflow = overflowItem(() => [
		{
			label: t("filters.favoritesOnly"),
			checked: values.favorites,
			onselect: () => onchange({ favorites: !values.favorites }),
		},
	]);
	const togglesOverflow = overflowItem(() =>
		LABELLED_TOGGLES.map(({ key, label }) => ({
			label: t(label),
			checked: values[key],
			onselect: () =>
				onchange({ [key]: !values[key] } as Partial<ConversationFilterValues>),
		})),
	);
</script>

<ProgressiveBlur
	direction="topToBottom"
	data-fixed-header
	class="absolute inset-x-0 top-0 z-10"
	bgClass="bg-linear-to-b from-background to-transparent"
	contentClass="pt-3"
	{inert}
	{@attach !inert && topChrome}
>
	<div
		data-slot="quick-filters-row"
		class="flex items-center gap-1.5 overflow-hidden px-3 pb-2"
		inert={locked}
		{@attach bar.row}
	>
		<ToggleGroup.Root
			type="multiple"
			variant="default"
			size="sm"
			class="h-9"
			{@attach favoritesOverflow}
			bind:value={
				() => (values.favorites ? ["favorites"] : []),
				(active: string[]) =>
					onchange({ favorites: active.includes("favorites") })
			}
		>
			<ToggleGroup.Item
				value="favorites"
				aria-label={t("filters.favoritesOnly")}
				class={buttonVariants({ variant: "secondary" })}
			>
				<StarIcon weight={values.favorites ? "fill" : "bold"} />
			</ToggleGroup.Item>
		</ToggleGroup.Root>
		<ToggleGroup.Root
			type="multiple"
			variant="default"
			size="sm"
			class="h-9"
			{@attach togglesOverflow}
			bind:value={
				() =>
					LABELLED_TOGGLES.filter(({ key }) => values[key]).map(
						({ key }) => key,
					),
				(active: string[]) =>
					onchange({
						unread: active.includes("unread"),
						online: active.includes("online"),
						rightNow: active.includes("rightNow"),
					})
			}
		>
			{#each LABELLED_TOGGLES as { key, label } (key)}
				<ToggleGroup.Item
					value={key}
					class={buttonVariants({ variant: "secondary" })}
				>
					{t(label)}
				</ToggleGroup.Item>
			{/each}
		</ToggleGroup.Root>
		<QuickFilterButton
			active={values.distanceMetres !== null}
			onclick={() => (open.distance = true)}
		>
			{t("filters.distance")}
		</QuickFilterButton>
		<QuickFilterButton
			active={values.positions.length > 0}
			onclick={() => (open.position = true)}
		>
			{t("filters.position")}
		</QuickFilterButton>
		<OverflowMenu {bar} />
	</div>
</ProgressiveBlur>

<DistanceQuickFilter
	bind:open={open.distance}
	distanceMetres={values.distanceMetres}
	onapply={(distanceMetres) => onchange({ distanceMetres })}
/>
<PositionQuickFilter
	bind:open={open.position}
	positions={values.positions}
	onapply={(positions) => onchange({ positions })}
/>
