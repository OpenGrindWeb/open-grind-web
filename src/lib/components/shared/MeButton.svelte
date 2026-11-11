<script lang="ts">
	import { t } from "#lib/i18n/index.svelte.ts";
	import { untrack } from "svelte";

	import { getProfile, onProfileEdit } from "#lib/api/users/profiles.ts";
	import BrokenUserAvatar from "#lib/components/profile/BrokenUserAvatar.svelte";
	import UserAvatar from "#lib/components/profile/UserAvatar.svelte";
	import { tabNavigation } from "./tab-navigation.ts";

	// Our own photo, linking to Me (settings). Lives in Browse's top bar.
	let { ourProfileId }: { ourProfileId: number } = $props();

	let myProfilePhotos = $state(
		untrack(() =>
			getProfile(ourProfileId).then((profile) => profile.medias),
		),
	);

	$effect(() =>
		onProfileEdit(({ profileId, patch }) => {
			if (profileId !== ourProfileId || patch.medias === undefined)
				return;
			myProfilePhotos = Promise.resolve(patch.medias);
		}),
	);
</script>

<a
	href="/settings"
	aria-label={t("nav.me")}
	data-slot="me-button"
	onclick={tabNavigation({ href: "/settings" })}
	class="flex size-9 shrink-0 rounded-full border border-border bg-muted p-0.5 transition-opacity active:opacity-60 motion-reduce:transition-none"
>
	{#await myProfilePhotos then photos}
		{@const mainPhoto = photos[0] as { mediaHash: string } | undefined}
		<UserAvatar
			mediaHash={mainPhoto?.mediaHash ?? null}
			class="size-full *:rounded-full"
			size="md"
		/>
	{:catch}
		<BrokenUserAvatar />
	{/await}
</a>
