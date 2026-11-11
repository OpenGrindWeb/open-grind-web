<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import DotsThreeCircleIcon from "phosphor-svelte/lib/DotsThreeCircleIcon";

	import { Button } from "#lib/components/ui/button/index.ts";
	import * as DropdownMenu from "#lib/components/ui/dropdown-menu/index.ts";
	import type { OverflowBar } from "./overflow-bar.svelte.ts";

	let { bar }: { bar: OverflowBar } = $props();

	const sections = $derived(bar.sections);
</script>

{#if bar.overflowing}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger>
			{#snippet child({ props: { class: className, ...props } })}
				<Button
					variant="secondary"
					size="icon"
					aria-label={t("filters.more")}
					data-slot="overflow-more"
					class={[className, "shrink-0 rounded-full"]}
					{...props}
				>
					<DotsThreeCircleIcon class="size-6" />
				</Button>
			{/snippet}
		</DropdownMenu.Trigger>
		<DropdownMenu.Content
			align="end"
			sideOffset={8}
			data-slot="overflow-menu"
			class="min-w-56"
		>
			{#each sections as entries, index (index)}
				{#if index > 0}
					<DropdownMenu.Separator />
				{/if}
				{#each entries as entry (entry.label)}
					{#if entry.checked === undefined}
						<DropdownMenu.Item onSelect={entry.onselect}>
							<span class="flex-1">{entry.label}</span>
							{#if entry.detail}
								<span class="text-muted-foreground"
									>{entry.detail}</span
								>
							{/if}
						</DropdownMenu.Item>
					{:else}
						<DropdownMenu.CheckboxItem
							checked={entry.checked}
							onCheckedChange={entry.onselect}
							closeOnSelect={false}
						>
							{entry.label}
						</DropdownMenu.CheckboxItem>
					{/if}
				{/each}
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
{/if}
