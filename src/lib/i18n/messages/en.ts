/**
 * Source catalog. Every other language is typed against these keys, so a
 * missing translation fails the type check instead of showing a raw key.
 */
export const en = {
	"nav.browse": "Browse",
	"nav.rightNow": "Right Now",
	"nav.interest": "Interest",
	"nav.inbox": "Inbox",
	"nav.me": "Me",

	"filters.allFilters": "All filters",
	"filters.age": "Age",
	"filters.position": "Position",
	"filters.online": "Online",
	"filters.rightNow": "Right now",
	"filters.fresh": "Fresh",
	"filters.changeLocation": "Change location",
	"filters.commandCenter": "Command center",
	"filters.more": "More filters",
	"filters.on": "On",
	"filters.sort": "Sort: {sort}",
	"filters.newest": "Newest",
	"filters.distance": "Distance",
	"filters.hosting": "Hosting",
	"filters.unread": "Unread",
	"filters.favoritesOnly": "Favorites only",

	"refresh.button": "Refresh",

	"rightNow.join": "Join",
	"rightNow.joinFree": "Join · Free",
	"rightNow.joinCount": "Join · {count} free",

	"inbox.you": "You:",
	"inbox.draft": "Draft:",
	"inbox.previewUnavailable": "Preview not available",
	"inbox.pin": "Pin",
	"inbox.unpin": "Unpin",
	"inbox.mute": "Mute",
	"inbox.unmute": "Unmute",
	"inbox.delete": "Delete",

	"interest.views": "Views",
	"interest.taps": "Taps",

	"chat.back": "Back to chats",
	"chat.distanceUnknown": "Distance unknown",
	"chat.placeholder": "Say something...",

	"preview.gaymoji": "Gaymoji",
	"preview.gif": "GIF",
	"preview.location": "Location",
	"preview.repliedToPhoto": "Replied to a photo",
	"preview.reactedToAlbum": "Reacted to an album",
	"preview.album": "Album",
	"preview.expiringImage": "Expiring image",
	"preview.expiringVideo": "Expiring video",
	"preview.photo": "Photo",

	"settings.display": "Display",
	"settings.language": "Language",
	"settings.languageDescription":
		"Choose the language of the interface. Screens not translated yet stay in English.",
} as const;

export type MessageKey = keyof typeof en;
