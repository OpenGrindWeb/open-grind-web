<script lang="ts">
	import * as Item from "#lib/components/ui/item/index.ts";
	import * as ToggleGroup from "#lib/components/ui/toggle-group/index.ts";
	import {
		LOCALE_NAMES,
		LOCALES,
		locale,
		setLocale,
		t,
		type Locale,
	} from "#lib/i18n/index.svelte.ts";
</script>

<Item.Root variant="outline" class="gap-3 p-4">
	<Item.Content class="gap-1">
		<Item.Title>{t("settings.language")}</Item.Title>
		<Item.Description>{t("settings.languageDescription")}</Item.Description>
	</Item.Content>
	<ToggleGroup.Root
		type="single"
		variant="outline"
		class="w-full"
		bind:value={
			() => locale(),
			(next: string) => {
				if (next) setLocale(next as Locale);
			}
		}
	>
		{#each LOCALES as code (code)}
			<ToggleGroup.Item value={code} lang={code} class="flex-1 justify-center">
				{LOCALE_NAMES[code]}
			</ToggleGroup.Item>
		{/each}
	</ToggleGroup.Root>
</Item.Root>
