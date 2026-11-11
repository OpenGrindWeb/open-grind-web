<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import CaretLeftIcon from "phosphor-svelte/lib/CaretLeftIcon";

	import BackLink from "#lib/components/navigation/BackLink.svelte";
	import ProgressiveBlur from "#lib/components/shared/ProgressiveBlur.svelte";
	import { Skeleton } from "#lib/components/ui/skeleton/index.ts";
	import { topChrome } from "#lib/util/screen-chrome.svelte.ts";
	import { getConversationState } from "../conversation-state.svelte";
	import ConversationNavBarProfile from "./ConversationNavBarProfile.svelte";

	const conversationState = $derived(getConversationState()());
</script>

<ProgressiveBlur
	direction="topToBottom"
	class="absolute z-10 h-19 w-full shrink-0"
	bgClass="bg-linear-to-b max-split:from-background split:from-card to-transparent"
	contentClass="flex items-center h-full"
	tag="nav"
	aria-label="Conversation"
	{@attach topChrome}
>
	<BackLink
		href="/chat"
		label={t("chat.back")}
		class="flex h-full w-14 shrink-0 items-center justify-center text-primary"
	>
		<CaretLeftIcon size={28} weight="bold" />
	</BackLink>
	{#if conversationState.profile !== null}
		<ConversationNavBarProfile profile={conversationState.profile} />
	{:else if conversationState.error}
		<span class="flex-1">Failed to load conversation</span>
	{:else}
		<div class="flex flex-1 flex-col items-center justify-center gap-1">
			<Skeleton class="size-9 rounded-full" />
			<Skeleton class="h-3 w-20 rounded-md" />
		</div>
	{/if}
	<!-- balances the back button so the profile stays centred, iOS style -->
	<span class="w-14 shrink-0" aria-hidden="true"></span>
</ProgressiveBlur>
