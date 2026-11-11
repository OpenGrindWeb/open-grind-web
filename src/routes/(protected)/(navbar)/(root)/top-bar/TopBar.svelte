<script lang="ts">
	import CommandCenterTrigger from "#lib/components/command-center/CommandCenterTrigger.svelte";
	import QuickFiltersBar from "#lib/components/filters/QuickFiltersBar.svelte";
	import MeButton from "#lib/components/shared/MeButton.svelte";
	import GridFilters from "../GridFilters.svelte";
	import LocationChange from "../LocationChange.svelte";
	import QuickFilters from "./QuickFilters.svelte";

	let { ourProfileId }: { ourProfileId: number } = $props();

	let openFilters = $state({ all: false, age: false, position: false });
</script>

<QuickFiltersBar>
	<MeButton {ourProfileId} />
	<LocationChange />
	<QuickFilters bind:openFilters />
	<CommandCenterTrigger />
</QuickFiltersBar>
<GridFilters bind:open={openFilters.all} />
