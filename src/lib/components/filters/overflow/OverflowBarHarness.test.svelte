<script lang="ts">
	import QuickFilterButton from "#lib/components/filters/QuickFilterButton.svelte";
	import QuickFiltersBar from "#lib/components/filters/QuickFiltersBar.svelte";

	let { onage }: { onage: () => void } = $props();
</script>

<QuickFiltersBar>
	<QuickFilterButton active onclick={onage}>Age</QuickFilterButton>
	<QuickFilterButton active={false} label="All filters" onclick={() => {}}>
		x
	</QuickFilterButton>
</QuickFiltersBar>
