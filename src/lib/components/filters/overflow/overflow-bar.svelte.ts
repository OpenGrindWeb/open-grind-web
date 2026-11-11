import { getContext, setContext, untrack } from "svelte";
import type { Attachment } from "svelte/attachments";

/** One line of the "…" menu. */
export type OverflowEntry = {
	label: string;
	/** Defined for toggles: the menu shows a checkmark when true. */
	checked?: boolean;
	/** Secondary text on the trailing side, e.g. "On" for an active filter. */
	detail?: string;
	onselect: () => void;
};

type Registered = {
	element: HTMLElement;
	entries: (element: HTMLElement) => OverflowEntry[];
};

const CONTEXT = Symbol("overflow-bar");
const MORE_BUTTON_PX = 36;

/**
 * A row of quick filters that never scrolls: the items that don't fit are
 * hidden and offered in a "…" menu instead, in the same order.
 *
 * Items join the bar themselves with `overflowItem()` while they render
 * inside it, so each one describes its own menu entries.
 */
export class OverflowBar {
	// Raw: the entries are only swapped whole, and must stay unproxied so a
	// registration can be found again to remove it.
	#registered = $state.raw<Registered[]>([]);
	#hidden = $state.raw<HTMLElement[]>([]);
	#row: HTMLElement | null = null;
	#gap = 0;
	#pending: ReturnType<typeof setTimeout> | undefined;
	#resize: ResizeObserver | null = null;

	get overflowing(): boolean {
		return this.#hidden.length > 0;
	}

	/** Menu sections, one per hidden item, in bar order. */
	get sections(): OverflowEntry[][] {
		const hidden = new Set(this.#hidden);
		return this.#ordered()
			.filter(({ element }) => hidden.has(element))
			.map(({ element, entries }) => entries(element));
	}

	row: Attachment<HTMLElement> = (row) => {
		this.#row = row;
		this.#gap = parseFloat(getComputedStyle(row).columnGap) || 0;
		const resize = new ResizeObserver(() => this.schedule());
		this.#resize = resize;
		resize.observe(row);
		// Attachments run in effects: reading the list here must not make
		// this one depend on it, or each registration would re-run it.
		untrack(() => {
			for (const { element } of this.#registered) resize.observe(element);
		});
		this.schedule();
		return () => {
			clearTimeout(this.#pending);
			this.#resize?.disconnect();
			this.#resize = null;
			this.#row = null;
		};
	};

	register(registered: Registered): () => void {
		untrack(() => {
			this.#registered = [...this.#registered, registered];
		});
		this.#resize?.observe(registered.element);
		this.schedule();
		return () => {
			untrack(() => {
				this.#registered = this.#registered.filter(
					(item) => item !== registered,
				);
			});
			this.#resize?.unobserve(registered.element);
			registered.element.style.removeProperty("display");
			this.schedule();
		};
	}

	/** Batches measurements; a timeout rather than a frame so it also runs
	 * while the page isn't painting (a backgrounded tab). */
	schedule(): void {
		clearTimeout(this.#pending);
		this.#pending = setTimeout(() => this.#measure(), 0);
	}

	#ordered(): Registered[] {
		return [...this.#registered].sort((a, b) =>
			a.element.compareDocumentPosition(b.element) &
			Node.DOCUMENT_POSITION_FOLLOWING
				? -1
				: 1,
		);
	}

	#measure(): void {
		const row = this.#row;
		if (!row) return;
		const items = this.#ordered();
		for (const { element } of items) element.style.removeProperty("display");
		row.scrollLeft = 0;

		if (row.scrollWidth <= row.clientWidth + 1) {
			if (this.#hidden.length) this.#hidden = [];
			return;
		}

		const box = row.getBoundingClientRect();
		const paddingEnd = parseFloat(getComputedStyle(row).paddingInlineEnd) || 0;
		const limit = box.right - paddingEnd - MORE_BUTTON_PX - this.#gap;
		const hidden: HTMLElement[] = [];
		for (const { element } of items) {
			if (hidden.length || element.getBoundingClientRect().right > limit)
				hidden.push(element);
		}
		for (const element of hidden) element.style.display = "none";
		const changed =
			hidden.length !== this.#hidden.length ||
			hidden.some((element, index) => element !== this.#hidden[index]);
		if (changed) this.#hidden = hidden;
	}
}

export function provideOverflowBar(): OverflowBar {
	return setContext(CONTEXT, new OverflowBar());
}

/**
 * Lets a component join the overflow bar it renders in, if any. Call it
 * during component init; attach the result to the element that should
 * move into the menu: `{@attach overflowItem((element) => [...])}`.
 */
export function overflowItem(
	entries: (element: HTMLElement) => OverflowEntry[],
): Attachment<HTMLElement> | undefined {
	const bar = getContext<OverflowBar | undefined>(CONTEXT);
	if (!bar) return undefined;
	return (element) => bar.register({ element, entries });
}
