<script lang="ts">
	import DisplayName from "#lib/components/profile/DisplayName.svelte";
	import DistanceFormatted from "#lib/components/profile/DistanceFormatted.svelte";
	import ProfileStatusIndicator from "#lib/components/profile/ProfileStatusIndicator.svelte";
	import UserAvatar from "#lib/components/profile/UserAvatar.svelte";
	import * as Avatar from "#lib/components/ui/avatar/index.ts";
	import * as Card from "#lib/components/ui/card/index.ts";
	import type { ConversationProfile } from "../conversation-state.svelte";

	let { profile }: { profile: ConversationProfile } = $props();
</script>

<!-- iOS Messages header: avatar centred above the name -->
<a
	href="/profile/{profile.profileId}"
	class="flex min-w-0 flex-1 justify-center py-1.5"
>
	<Card.Header class="flex w-full min-w-0 flex-col items-center gap-1 px-0">
		<Avatar.Root class="size-9 after:rounded-full">
			<UserAvatar
				mediaHash={profile.mediaHash ?? null}
				class="size-full *:rounded-full"
				size="lg"
			/>
		</Avatar.Root>
		<div
			class="flex max-w-full min-w-0 items-baseline gap-1.5 text-[12px] leading-none"
		>
			<Card.Title
				class={[
					"flex min-w-0 items-center gap-1 text-[12px] font-medium",
					{ "text-muted-foreground": !profile.name },
				]}
			>
				<ProfileStatusIndicator onlineUntil={profile.onlineUntil} />
				<DisplayName name={profile.name} class="truncate" />
			</Card.Title>
			{#if profile.distance === null}
				<Card.Description class="truncate text-[11px]"
					>Distance unknown</Card.Description
				>
			{:else}
				<Card.Description class="truncate text-[11px]">
					<DistanceFormatted distance={profile.distance} />
				</Card.Description>
			{/if}
		</div>
	</Card.Header>
</a>
