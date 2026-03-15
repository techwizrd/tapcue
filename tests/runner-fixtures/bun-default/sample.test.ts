import { expect, test } from "bun:test";

test("passes", () => {
  expect(1 + 1).toBe(2);
});

test("fails", () => {
  expect(2 * 2).toBe(5);
});
