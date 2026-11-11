<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import { overflowItem } from "#lib/components/filters/overflow/overflow-bar.svelte.ts";
	import { Button } from "#lib/components/ui/button/index.ts";

	let {
		active,
		label,
		onclick,
		children,
	}: {
		active: boolean;
		label?: string;
		onclick: () => void;
		children: import("svelte").Snippet;
	} = $props();

	// In a bar too narrow for it, this button moves into the "…" menu.
	const overflow = overflowItem((element) => [
		{
			label: label ?? element.textContent?.trim() ?? "",
			detail: active && label === undefined ? t("filters.on") : undefined,
			onselect: onclick,
		},
	]);
</script>

<Button
	variant="secondary"
	aria-label={label}
	data-active={active || undefined}
	{onclick}
	class={{ "bg-white text-popover hover:bg-neutral-200": active }}
	{@attach overflow}
>
	{@render children()}
</Button>
