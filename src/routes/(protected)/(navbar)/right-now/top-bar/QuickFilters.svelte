<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import FunnelSimpleIcon from "phosphor-svelte/lib/FunnelSimpleIcon";

	import AgeQuickFilter from "#lib/components/filters/age/AgeQuickFilter.svelte";
	import PositionQuickFilter from "#lib/components/filters/position/PositionQuickFilter.svelte";
	import QuickFilterButton from "#lib/components/filters/QuickFilterButton.svelte";
	import type { RightNowFeedFiltersState } from "../right-now-feed-filters-state.svelte";
	import HostingQuickFilter from "./HostingQuickFilter.svelte";

	let { filters }: { filters: RightNowFeedFiltersState } = $props();

	const sortLabels = {
		NEWEST: "filters.newest",
		DISTANCE: "filters.distance",
	} as const;

	let openFilters = $state({ age: false, position: false, hosting: false });

	const { sort, ageEnabled, positionEnabled, hostingEnabled } = $derived(
		filters.current,
	);
	const sortLabel = $derived(t(sortLabels[sort]));
</script>

<QuickFilterButton
	active
	label={t("filters.sort", { sort: sortLabel })}
	onclick={() =>
		filters.set({ sort: sort === "NEWEST" ? "DISTANCE" : "NEWEST" })}
>
	<FunnelSimpleIcon />
	{sortLabel}
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
<QuickFilterButton
	active={hostingEnabled}
	onclick={() => (openFilters.hosting = true)}
>
	{t("filters.hosting")}
</QuickFilterButton>

<AgeQuickFilter bind:open={openFilters.age} {filters} />
<PositionQuickFilter bind:open={openFilters.position} {filters} />
<HostingQuickFilter bind:open={openFilters.hosting} {filters} />
