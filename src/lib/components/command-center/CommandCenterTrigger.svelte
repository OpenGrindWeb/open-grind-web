<script>
	import { t } from "#lib/i18n/index.svelte.ts";
	import TerminalIcon from "phosphor-svelte/lib/TerminalIcon";

	import { overflowItem } from "#lib/components/filters/overflow/overflow-bar.svelte.ts";
	import { Button } from "#lib/components/ui/button/index.ts";
	import * as Kbd from "#lib/components/ui/kbd/index.ts";
	import { modifierKeyLabel } from "#lib/platform/keybindings.ts";
	import { isMobilePlatform } from "#lib/platform/os.ts";
	import { commandCenterState } from "./command-center-state.svelte";

	const isMobile = isMobilePlatform();
	const modifierKey = modifierKeyLabel();

	// In a bar too narrow for it, this moves into the "…" menu.
	const overflow = overflowItem(() => [
		{
			label: t("filters.commandCenter"),
			onselect: () => (commandCenterState.open = true),
		},
	]);
</script>

<Button
	variant="outline"
	onclick={() => (commandCenterState.open = true)}
	class={{ "pe-1.5": !isMobile }}
	{@attach overflow}
>
	<TerminalIcon />
	{#if !isMobile}
		<Kbd.Root>{modifierKey} + K</Kbd.Root>
	{/if}
</Button>
