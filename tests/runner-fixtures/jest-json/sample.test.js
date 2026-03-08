test("jest pass", () => {
  expect(1 + 1).toBe(2);
});

test("jest fail", () => {
  expect("tapcue").toBe("tap-cue");
});

test.skip("jest skipped", () => {
  expect(true).toBe(false);
});
