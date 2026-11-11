import { cleanup, render, screen, waitFor } from "@testing-library/svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import Harness from "./OverflowBarHarness.test.svelte";

const ROW = '[data-slot="quick-filters-row"]';

// jsdom has no layout: give the row and each filter a width so the bar can
// measure what fits. Filters are 100px wide, laid out left to right.
function layOut({ rowWidth }: { rowWidth: number }) {
	const box = (left: number, width: number) =>
		({ left, right: left + width, width, top: 0, bottom: 36, height: 36 }) as DOMRect;
	vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(
		function (this: HTMLElement) {
			if (this.matches(ROW)) return box(0, rowWidth);
			const siblings = [...(this.parentElement?.children ?? [])];
			return box(siblings.indexOf(this) * 100, 100);
		},
	);
	vi.spyOn(HTMLElement.prototype, "scrollWidth", "get").mockImplementation(
		function (this: HTMLElement) {
			return this.matches(ROW) ? this.children.length * 100 : 0;
		},
	);
	vi.spyOn(HTMLElement.prototype, "clientWidth", "get").mockImplementation(
		function (this: HTMLElement) {
			return this.matches(ROW) ? rowWidth : 0;
		},
	);
}

afterEach(() => {
	cleanup();
	vi.restoreAllMocks();
});

describe("quick filters bar", () => {
	it("renders its filters and no menu while they fit", async () => {
		layOut({ rowWidth: 1000 });
		render(Harness, { onage: vi.fn() });

		await new Promise((resolve) => setTimeout(resolve, 10));
		expect(screen.getByRole("button", { name: "Age" })).toBeTruthy();
		expect(screen.getByRole("button", { name: "All filters" })).toBeTruthy();
		expect(screen.queryByRole("button", { name: "More filters" })).toBeNull();
	});

	it("moves the filters that don't fit behind a More filters button", async () => {
		layOut({ rowWidth: 150 });
		render(Harness, { onage: vi.fn() });

		await waitFor(() =>
			expect(screen.getByRole("button", { name: "More filters" })).toBeTruthy(),
		);
		expect(screen.getByRole("button", { name: "Age" }).style.display).toBe("");
		const allFilters = document.querySelector<HTMLElement>(
			'[aria-label="All filters"]',
		);
		expect(allFilters?.style.display).toBe("none");
	});
});
