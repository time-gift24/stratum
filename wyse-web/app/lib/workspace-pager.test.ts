import assert from "node:assert/strict"
import test from "node:test"

import { getPagerMotionPolicy, resolveSlideIndex } from "./workspace-pager"

test("clamps slide selections to the available slide range", () => {
  assert.equal(resolveSlideIndex(1, -1, 3, false), 0)
  assert.equal(resolveSlideIndex(1, 9, 3, false), 2)
  assert.equal(resolveSlideIndex(1, 2, 3, false), 2)
})

test("keeps the current slide while a transition is locked", () => {
  assert.equal(resolveSlideIndex(1, 2, 3, true), 1)
})

test("uses instant movement when reduced motion is requested", () => {
  assert.equal(getPagerMotionPolicy(false), "animate")
  assert.equal(getPagerMotionPolicy(true), "instant")
})
