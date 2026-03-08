import { describe, expect, it } from "vitest";

describe("vitest fixture", () => {
  it("passes", () => {
    expect([1, 2, 3].length).toBe(3);
  });

  it("fails", () => {
    expect("tapcue").toBe("tap-cue");
  });

  it.skip("skipped", () => {
    expect(true).toBe(false);
  });
});
