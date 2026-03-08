"use strict";

const t = require("tap");

t.test("passes basic math", (assert) => {
  assert.equal(1 + 1, 2);
  assert.end();
});

t.test("fails intentionally", (assert) => {
  assert.equal(2 * 2, 5, "intentional failure");
  assert.end();
});

t.test("skipped test", { skip: true }, () => {});
