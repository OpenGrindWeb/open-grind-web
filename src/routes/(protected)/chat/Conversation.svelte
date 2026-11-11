<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import { page } from "$app/state";
	import BellIcon from "phosphor-svelte/lib/BellIcon";
	import BellSimpleSlashIcon from "phosphor-svelte/lib/BellSimpleSlashIcon";
	import CaretRightIcon from "phosphor-svelte/lib/CaretRightIcon";
	import PushPinIcon from "phosphor-svelte/lib/PushPinIcon";
	import PushPinSlashIcon from "phosphor-svelte/lib/PushPinSlashIcon";
	import TrashIcon from "phosphor-svelte/lib/TrashIcon";

	import { getConversations } from "#lib/chat/conversations-context.svelte.ts";
	import FavoriteStar from "#lib/components/profile/FavoriteStar.svelte";
	import ProfileItem from "#lib/components/profile/ProfileItem.svelte";
	import RelativeTimeDynamic from "#lib/components/shared/RelativeTimeDynamic.svelte";
	import SelectionCheck from "#lib/components/shared/SelectionCheck.svelte";
	import { Badge } from "#lib/components/ui/badge/index.ts";
	import * as ContextMenu from "#lib/components/ui/context-menu/index.ts";
	import * as Item from "#lib/components/ui/item/index.ts";
	import { playHaptic } from "#lib/haptics.ts";
	import { previewLabel } from "#lib/model/messaging/message-preview.ts";
	import { firedByTouch } from "#lib/platform/touch-origin.ts";
	import type { Conversation } from "#lib/model/messaging/conversations.ts";
	import type { SelectionSet } from "#lib/util/selection.svelte.ts";

	let {
		conversation,
		selection = null,
		onEnterSelection,
		onRequestDelete,
	}: {
		conversation: Conversation;
		selection?: SelectionSet<string> | null;
		onEnterSelection?: () => void;
		onRequestDelete?: () => void;
	} = $props();

	const conversations = getConversations();

	const preview = $derived(conversation.data.preview);
	const participant = $derived(conversation.data.participants[0]);
	const previewText = $derived(previewLabel(preview));
	// The last message is ours: prefix it, like most messengers do.
	const previewIsOurs = $derived(
		preview?.senderId != null &&
			preview.senderId === conversations.ourProfileId,
	);
	const conversationId = $derived(conversation.data.conversationId);
	const draft = $derived(conversations.drafts.get(conversationId));

	const active = $derived(page.params.conversationId === conversationId);
	const isSelected = $derived(selection?.has(conversationId) ?? false);

	let contextMenuUsed = $state(false);
	let pressedByTouch = false;

	function togglePinned() {
		void conversations.setPinned({
			conversationIds: [conversationId],
			pinned: !conversation.data.pinned,
		});
	}

	function toggleMuted() {
		void conversations.setMuted({
			conversationIds: [conversationId],
			muted: !conversation.data.muted,
		});
	}

	function toggleSelected() {
		selection?.toggle(conversationId);
	}
</script>

{#snippet titleBadges()}
	{#if conversation.data.favorite}
		<FavoriteStar />
	{/if}
	{#if conversation.data.muted}
		<BellSimpleSlashIcon
			weight="fill"
			class="size-4 shrink-0 text-muted-foreground"
		/>
	{/if}
{/snippet}

{#snippet selectedOverlay()}
	<div class="absolute inset-2 flex items-center justify-center">
		<SelectionCheck />
	</div>
{/snippet}

{#snippet row()}
	<ProfileItem
		{active}
		avatar={{
			mediaHash: participant?.primaryMediaHash ?? null,
			link: participant ? `/profile/${participant.profileId}` : undefined,
			overlay: isSelected ? selectedOverlay : undefined,
		}}
		title={{ value: conversation.data.name, badge: titleBadges }}
		onlineUntil={conversation.data.onlineUntil ?? participant?.onlineUntil}
		link="/chat/{conversationId}"
		selected={isSelected}
		onToggleSelected={selection ? toggleSelected : undefined}
		onLongPress={onEnterSelection}
	>
		{#snippet description()}
			<Item.Description
				class={[
					"line-clamp-2 text-[14px] leading-snug wrap-anywhere",
					{
						"font-medium text-white":
							draft === "" &&
							conversation.data.unreadCount > 0 &&
							!conversation.data.muted,
					},
				]}
			>
				{#if draft !== ""}
					<span
						data-slot="conversation-draft-prefix"
						class="font-bold text-primary">{t("inbox.draft")}&nbsp;</span
					>{draft}
				{:else if previewText !== null}
					{#if previewIsOurs}
						<span
							data-slot="conversation-preview-ours"
							class="font-normal text-muted-foreground">{t("inbox.you")}&nbsp;</span
						>
					{/if}{previewText}
				{:else}
					<span
						class="font-normal tracking-tight text-muted-foreground italic"
					>
						{t("inbox.previewUnavailable")}
					</span>
				{/if}
			</Item.Description>
		{/snippet}
		{#snippet actions()}
			<Item.Actions
				class="flex min-w-0 flex-col items-end gap-1.5 self-start pt-0.5"
			>
				<span
					class="flex max-w-full items-center gap-1 text-[13px] text-muted-foreground"
				>
					{#if conversation.data.pinned}
						<PushPinIcon weight="fill" class="size-4 shrink-0" />
					{/if}
					<span class="truncate text-right">
						<RelativeTimeDynamic
							date={conversation.data.lastActivityTimestamp}
						/>
					</span>
					<CaretRightIcon
						weight="bold"
						class="size-3.5 shrink-0 opacity-50"
					/>
				</span>
				{#if conversation.data.unreadCount > 0}
					<Badge
						variant={conversation.data.muted
							? "secondary"
							: "default"}
						class="px-count-badge @max-row:hidden"
					>
						{conversation.data.unreadCount}
					</Badge>
				{/if}
			</Item.Actions>
		{/snippet}
	</ProfileItem>
{/snippet}

{#if onEnterSelection}
	{@render row()}
{:else}
	<ContextMenu.Root
		onOpenChange={(open) => {
			if (!open) return;
			contextMenuUsed = true;
			if (pressedByTouch) playHaptic("longPress");
		}}
	>
		<ContextMenu.Trigger
			class="rounded-2xl"
			onpointerdown={(event) => (pressedByTouch = firedByTouch(event))}
			oncontextmenu={(event) => (pressedByTouch = firedByTouch(event))}
		>
			{@render row()}
		</ContextMenu.Trigger>
		{#if contextMenuUsed}
			<ContextMenu.Content class="w-48">
				<ContextMenu.Item onSelect={togglePinned}>
					{#if conversation.data.pinned}
						<PushPinSlashIcon weight="fill" class="size-5" />
						{t("inbox.unpin")}
					{:else}
						<PushPinIcon weight="fill" class="size-5" />
						{t("inbox.pin")}
					{/if}
				</ContextMenu.Item>
				<ContextMenu.Item onSelect={toggleMuted}>
					{#if conversation.data.muted}
						<BellIcon weight="fill" class="size-5" />
						{t("inbox.unmute")}
					{:else}
						<BellSimpleSlashIcon weight="fill" class="size-5" />
						{t("inbox.mute")}
					{/if}
				</ContextMenu.Item>
				<ContextMenu.Separator />
				<ContextMenu.Item
					variant="destructive"
					onSelect={onRequestDelete}
				>
					<TrashIcon class="size-5" />
					{t("inbox.delete")}
				</ContextMenu.Item>
			</ContextMenu.Content>
		{/if}
	</ContextMenu.Root>
{/if}
