import "./message-lightbox.css";

import { createContext } from "svelte";
import type PhotoSwipeLightbox from "photoswipe/lightbox";

import {
	applyPhotoSwipeBackGesture,
	applyPhotoSwipeErrorUi,
	applyPhotoSwipeThumbDimensions,
	applyPhotoSwipeViewportSync,
	CLOSE_SVG,
} from "#lib/util/photoswipe/index.ts";

const PHOTOSWIPE_PLACEHOLDER_WIDTH_PX = 250;

function setThumbRadii(lightbox: PhotoSwipeLightbox) {
	const slide = lightbox.pswp?.currSlide;
	const thumb = slide?.data.element?.querySelector("img");
	if (!slide || !(thumb instanceof HTMLImageElement)) return;

	const thumbWidth = thumb.getBoundingClientRect().width;
	const displayedWidth = slide.width * slide.zoomLevels.initial;
	if (thumbWidth === 0 || displayedWidth === 0) return;

	const style = getComputedStyle(thumb);
	const corners = [
		style.borderTopLeftRadius,
		style.borderTopRightRadius,
		style.borderBottomRightRadius,
		style.borderBottomLeftRadius,
	].map(parseFloat);

	const radiiUndoingScale = (scale: number) =>
		corners.map((corner) => `${corner / scale}px`).join(" ");

	const root = document.documentElement.style;
	root.setProperty(
		"--pswp-thumb-radius",
		radiiUndoingScale(thumbWidth / displayedWidth),
	);
	root.setProperty(
		"--pswp-placeholder-radius",
		radiiUndoingScale(thumbWidth / PHOTOSWIPE_PLACEHOLDER_WIDTH_PX),
	);
}

function clearThumbRadii() {
	document.documentElement.style.removeProperty("--pswp-thumb-radius");
	document.documentElement.style.removeProperty("--pswp-placeholder-radius");
}

function setThumbsVisibility({
	gallery,
	visibility,
}: {
	gallery: HTMLElement;
	visibility: "hidden" | "visible";
}) {
	gallery.querySelectorAll(".item").forEach((item) => {
		if (item instanceof HTMLElement) item.style.visibility = visibility;
	});
}

function scaleThumbSize({
	lightbox,
	thumbScale,
}: {
	lightbox: PhotoSwipeLightbox;
	thumbScale: number;
}) {
	lightbox.addFilter("itemData", (itemData) => ({
		...itemData,
		width: (itemData.width ?? 0) * thumbScale,
		height: (itemData.height ?? 0) * thumbScale,
	}));
}

/** The current slide's own thumb, for a gallery spanning many messages. */
function setSlideThumbVisibility({
	lightbox,
	visibility,
}: {
	lightbox: PhotoSwipeLightbox;
	visibility: "hidden" | "visible";
}) {
	const element = lightbox.pswp?.currSlide?.data.element;
	if (element instanceof HTMLElement) element.style.visibility = visibility;
}

function configure({
	lightbox,
	gallery,
	thumbScale,
	spansMessages = false,
}: {
	lightbox: PhotoSwipeLightbox;
	gallery: HTMLElement;
	thumbScale: number;
	spansMessages?: boolean;
}) {
	// A gallery over the whole conversation only hides the thumb the
	// opening/closing animation flies to; hiding them all would blank the chat.
	const hideThumbs = () =>
		spansMessages
			? setSlideThumbVisibility({ lightbox, visibility: "hidden" })
			: setThumbsVisibility({ gallery, visibility: "hidden" });
	applyPhotoSwipeErrorUi(lightbox);
	applyPhotoSwipeThumbDimensions(lightbox);
	if (thumbScale !== 1) scaleThumbSize({ lightbox, thumbScale });
	applyPhotoSwipeViewportSync(lightbox);
	applyPhotoSwipeBackGesture(lightbox);

	lightbox.on("openingAnimationStart", () => {
		setThumbRadii(lightbox);
		lightbox.pswp?.element?.classList.add("pswp--radius-opening");
		hideThumbs();
	});
	lightbox.on("openingAnimationEnd", () => {
		lightbox.pswp?.element?.classList.remove("pswp--radius-opening");
		clearThumbRadii();
	});
	lightbox.on("closingAnimationStart", () => {
		setThumbRadii(lightbox);
		lightbox.pswp?.element?.classList.add("pswp--radius-closing");
		hideThumbs();
	});
	lightbox.on("closingAnimationEnd", clearThumbRadii);
	if (spansMessages) {
		// the thumb hidden for the opening is the slide we started on
		lightbox.on("change", () =>
			setThumbsVisibility({ gallery, visibility: "visible" }),
		);
	}
	lightbox.on("destroy", () =>
		setThumbsVisibility({ gallery, visibility: "visible" }),
	);
}

/**
 * Set by a conversation that shows its photos in one lightbox, so a photo
 * opens among all the others and swipes to them (like the official app)
 * instead of each message opening a lightbox of its own.
 */
export const [hasConversationPhotoGallery, setConversationPhotoGallery] =
	createContext<boolean>();

export const CONVERSATION_PHOTO = "a[data-conversation-photo][href]";

/** One lightbox over every photo message in a conversation, in order. */
export function attachConversationPhotoGallery({
	gallery,
}: {
	gallery: HTMLElement;
}): () => void {
	return attachLightbox({
		gallery,
		children: CONVERSATION_PHOTO,
		thumbScale: 1,
		spansMessages: true,
	});
}

export function attachGalleryLightbox({
	gallery,
	thumbScale = 1,
}: {
	gallery: HTMLElement;
	thumbScale?: number;
}): () => void {
	return attachLightbox({ gallery, children: "a[href]", thumbScale });
}

function attachLightbox({
	gallery,
	children,
	thumbScale,
	spansMessages = false,
}: {
	gallery: HTMLElement;
	children: string;
	thumbScale: number;
	spansMessages?: boolean;
}): () => void {
	let disposed = false;
	let lightbox: PhotoSwipeLightbox | undefined;
	import("photoswipe/lightbox")
		.then(({ default: PhotoSwipeLightbox }) => {
			if (disposed) return;
			lightbox = new PhotoSwipeLightbox({
				gallery,
				children,
				pswpModule: () => import("photoswipe"),
				mainClass: "pswp--buttons-visible",
				showAnimationDuration: 500,
				hideAnimationDuration: 500,
				closeSVG: CLOSE_SVG,
			});
			configure({ lightbox, gallery, thumbScale, spansMessages });
			lightbox.init();
		})
		.catch((error) => console.error(error));
	return () => {
		disposed = true;
		lightbox?.destroy();
	};
}
