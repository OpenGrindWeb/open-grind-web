import { describe, expect, it } from "vitest";

import { tabDirection, tabIndexOf } from "./tab-transition.ts";

describe("tab transitions", () => {
	it("knows each tab's root screen and nothing below the others", () => {
		expect(tabIndexOf("/")).toBe(0);
		expect(tabIndexOf("/right-now")).toBe(1);
		expect(tabIndexOf("/interest/taps")).toBe(2);
		expect(tabIndexOf("/chat")).toBe(3);
		expect(tabIndexOf("/settings")).toBe(4);
		expect(tabIndexOf("/chat/100001:123")).toBeNull();
		expect(tabIndexOf("/settings/profile")).toBeNull();
		expect(tabIndexOf("/profile/42")).toBeNull();
	});

	it("slides toward the tab picked, and only between two tabs", () => {
		expect(tabDirection("/", "/chat")).toBe("forward");
		expect(tabDirection("/chat", "/")).toBe("back");
		expect(tabDirection("/interest/taps", "/interest/views")).toBeNull();
		expect(tabDirection("/chat", "/chat/100001:123")).toBeNull();
		expect(tabDirection(undefined, "/")).toBeNull();
	});
});
