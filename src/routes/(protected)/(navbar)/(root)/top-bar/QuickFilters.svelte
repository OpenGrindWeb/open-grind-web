<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import SlidersHorizontalIcon from "phosphor-svelte/lib/SlidersHorizontalIcon";

	import { genderFilterFlag } from "#lib/api/feature-flags-state.svelte.ts";
	import AgeQuickFilter from "#lib/components/filters/age/AgeQuickFilter.svelte";
	import PositionQuickFilter from "#lib/components/filters/position/PositionQuickFilter.svelte";
	import { overflowItem } from "#lib/components/filters/overflow/overflow-bar.svelte.ts";
	import QuickFilterButton from "#lib/components/filters/QuickFilterButton.svelte";
	import { buttonVariants } from "#lib/components/ui/button/index.ts";
	import * as ToggleGroup from "#lib/components/ui/toggle-group/index.ts";
	import { gridState } from "#lib/grid/grid-state.svelte.ts";
	import { sentFilterKeysWithoutQuickFilter } from "./quick-filters";

	let {
		openFilters = $bindable(),
	}: { openFilters: { all: boolean; age: boolean; position: boolean } } =
		$props();

	const TOGGLE_FILTER_KEYS = ["isOnline", "isRightNow", "isFresh"] as const;
	const TOGGLE_LABELS = {
		isOnline: "filters.online",
		isRightNow: "filters.rightNow",
		isFresh: "filters.fresh",
	} as const;

	// In a bar too narrow for it, the toggle group moves into the "…" menu
	// as one checkable line per toggle.
	const toggleOverflow = overflowItem(() =>
		TOGGLE_FILTER_KEYS.map((key) => ({
			label: t(TOGGLE_LABELS[key]),
			checked: filters[key],
			onselect: () => {
				const next = {
					isOnline: filters.isOnline,
					isRightNow: filters.isRightNow,
					isFresh: filters.isFresh,
				};
				next[key] = !next[key];
				gridState.filters.set(next);
			},
		})),
	);

	const filters = $derived(gridState.filters.current);
	const { ageEnabled, positionEnabled } = $derived(filters);
	const sheetOnlyFiltersSent = $derived(
		sentFilterKeysWithoutQuickFilter({
			filters,
			genderFilterFlag: genderFilterFlag(),
		}).length > 0,
	);
</script>

<QuickFilterButton
	active={sheetOnlyFiltersSent}
	label={t("filters.allFilters")}
	onclick={() => (openFilters.all = true)}
>
	<SlidersHorizontalIcon />
</QuickFilterButton>
<QuickFilterButton active={ageEnabled} onclick={() => (openFilters.age = true)}>
	{t("filters.age")}
</QuickFilterButton>
<QuickFilterButton
	active={positionEnabled}
	onclick={() => (openFilters.position = true)}
>
	{t("filters.position")}
</QuickFilterButton>
<ToggleGroup.Root
	type="multiple"
	variant="default"
	bind:value={
		() => TOGGLE_FILTER_KEYS.filter((key) => filters[key]),
		(values: (typeof TOGGLE_FILTER_KEYS)[number][]) => {
			gridState.filters.set({
				isOnline: values.includes("isOnline"),
				isRightNow: values.includes("isRightNow"),
				isFresh: values.includes("isFresh"),
			});
		}
	}
	size="sm"
	class="h-9"
	{@attach toggleOverflow}
>
	<ToggleGroup.Item
		value="isOnline"
		class={buttonVariants({ variant: "secondary" })}
	>
		{t("filters.online")}
	</ToggleGroup.Item>
	<ToggleGroup.Item
		value="isRightNow"
		class={buttonVariants({ variant: "secondary" })}
	>
		{t("filters.rightNow")}
	</ToggleGroup.Item>
	<ToggleGroup.Item
		value="isFresh"
		class={buttonVariants({ variant: "secondary" })}
	>
		{t("filters.fresh")}
	</ToggleGroup.Item>
</ToggleGroup.Root>

<AgeQuickFilter bind:open={openFilters.age} filters={gridState.filters} />
<PositionQuickFilter
	bind:open={openFilters.position}
	filters={gridState.filters}
/>
