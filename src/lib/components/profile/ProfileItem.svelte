<script lang="ts">
	import DisplayName from "#lib/components/profile/DisplayName.svelte";
	import ProfileStatusIndicator from "#lib/components/profile/ProfileStatusIndicator.svelte";
	import UserAvatar from "#lib/components/profile/UserAvatar.svelte";
	import * as Avatar from "#lib/components/ui/avatar/index.ts";
	import * as Item from "#lib/components/ui/item/index.ts";
	import { longPressHandlers } from "#lib/util/long-press.ts";

	let {
		avatar,
		title,
		onlineUntil = null,
		active,
		selected,
		link,
		description,
		actions,
		control,
		onToggleSelected,
		onLongPress,
	}: {
		avatar: {
			mediaHash: string | null;
			overlay?: import("svelte").Snippet;
			link?: string;
		};
		title: {
			value: string | null;
			fallback?: string;
			badge?: import("svelte").Snippet;
		};
		onlineUntil?: number | null;
		active?: boolean;
		selected?: boolean;
		link: string;
		description?: import("svelte").Snippet;
		actions?: import("svelte").Snippet;
		control?: import("svelte").Snippet;
		onToggleSelected?: () => void;
		onLongPress?: () => void;
	} = $props();

	const longPress = $derived(
		onLongPress ? longPressHandlers(onLongPress) : {},
	);
	const linkTabindex = $derived(onToggleSelected ? -1 : undefined);
	const accessibleName = $derived(title.value ?? title.fallback ?? "Someone");
</script>

<!-- iOS list row: round avatar, text block with a hairline under it that
     starts where the text does (inset separator), trailing accessories. -->
{#snippet avatarNode()}
	<Item.Media class="relative translate-y-0! py-2.5 ps-4 pe-3">
		<Avatar.Root class="size-13 after:rounded-full">
			<UserAvatar
				mediaHash={avatar.mediaHash}
				class="size-13 rounded-full bg-neutral-700 *:rounded-full"
			/>
		</Avatar.Root>
		{@render avatar.overlay?.()}
	</Item.Media>
{/snippet}
{#snippet contentNode()}
	<div data-slot="profile-item-body" class="body">
		<Item.Content class="min-w-0 flex-1 gap-0.5">
			<Item.Title
				class={[
					"flex w-auto min-w-0 items-center gap-1 truncate text-[16px] leading-tight font-semibold tracking-[-0.01em]",
					{ "text-muted-foreground": !title.value },
				]}
			>
				{@render title.badge?.()}
				<ProfileStatusIndicator {onlineUntil} />
				<DisplayName
					name={title.value}
					fallback={title.fallback}
					class="truncate"
				/>
			</Item.Title>
			{@render description?.()}
		</Item.Content>
		{@render actions?.()}
	</div>
{/snippet}
<Item.Root
	variant={active ? "muted" : "default"}
	data-profile-item
	class={[
		"@container relative flex min-w-24 flex-nowrap items-stretch gap-0 rounded-none p-0 active:bg-white/6 [a]:hover:bg-transparent",
		{
			"border-primary outline-2 -outline-offset-2 outline-primary outline-solid":
				selected,
			"[-webkit-touch-callout:none] **:[-webkit-touch-callout:none]":
				!!onLongPress,
		},
	]}
	{...longPress}
>
	{#if avatar.link}
		<a
			href={avatar.link}
			aria-label="{accessibleName}'s profile"
			class="@max-row:hidden"
			tabindex={linkTabindex}
		>
			{@render avatarNode()}
		</a>
		<a
			href={link}
			class="content @max-row:hidden!"
			tabindex={linkTabindex}
		>
			{@render contentNode()}
		</a>
		<a
			href={link}
			aria-label={accessibleName}
			class="min-w-24 @row:hidden"
			tabindex={linkTabindex}
		>
			{@render avatarNode()}
		</a>
	{:else}
		<a
			href={link}
			class="content overflow-clip"
			tabindex={linkTabindex}
		>
			{@render avatarNode()}
			{@render contentNode()}
		</a>
	{/if}
	{#if control}
		<div class="flex shrink-0 items-center border-b-[0.5px] border-(--ios-separator) ps-3 pe-4">
			{@render control()}
		</div>
	{/if}
	{#if selected}
		<div
			class="pointer-events-none absolute -inset-px z-1 rounded-[inherit] bg-primary/20"
		></div>
	{/if}
	{#if onToggleSelected}
		<button
			type="button"
			class="absolute inset-0 z-2 rounded-[inherit] outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50"
			aria-pressed={selected ?? false}
			aria-label={accessibleName}
			onclick={onToggleSelected}
		></button>
	{/if}
</Item.Root>

<style lang="postcss">
	@reference "$layout";

	.content {
		@apply flex min-w-0 flex-1 items-center self-stretch;
	}

	.body {
		@apply flex min-w-0 flex-1 items-center gap-2 self-stretch py-2.5 pe-4;
		border-bottom: 0.5px solid var(--ios-separator, rgb(84 84 88 / 0.65));
	}
</style>
