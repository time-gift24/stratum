import { describe, expect, it } from "vitest"

import {
  formReducer,
  initialFormState,
} from "@/features/studio-management/form-state"

describe("Studio form refresh", () => {
  it("keeps a dirty draft when a background refresh completes", () => {
    const loaded = initialFormState({ name: "server-v1" }, '"v1"')
    const dirty = formReducer(loaded, {
      type: "edit",
      draft: { name: "local draft" },
    })

    const refreshed = formReducer(dirty, {
      type: "refresh",
      value: { name: "server-v2" },
      etag: '"v2"',
    })

    expect(refreshed).toBe(dirty)
  })

  it("updates an untouched form from a background refresh", () => {
    const loaded = initialFormState({ name: "server-v1" }, '"v1"')

    const refreshed = formReducer(loaded, {
      type: "refresh",
      value: { name: "server-v2" },
      etag: '"v2"',
    })

    expect(refreshed.draft).toEqual({ name: "server-v2" })
    expect(refreshed.etag).toBe('"v2"')
  })

  it("only replaces a dirty draft after an explicit reload", () => {
    const loaded = initialFormState({ name: "server-v1" }, '"v1"')
    const dirty = formReducer(loaded, {
      type: "edit",
      draft: { name: "local draft" },
    })

    const reloaded = formReducer(dirty, {
      type: "reload",
      value: { name: "server-v2" },
      etag: '"v2"',
    })

    expect(reloaded.phase).toBe("loaded")
    expect(reloaded.draft.name).toBe("server-v2")
    expect(reloaded.etag).toBe('"v2"')
  })

  it("returns to loaded after a draft is fully restored", () => {
    const loaded = initialFormState(
      { name: "server-v1", parameters: { temperature: 0.2, nested: true } },
      '"v1"'
    )
    const dirty = formReducer(loaded, {
      type: "edit",
      draft: {
        name: "local draft",
        parameters: { temperature: 0.2, nested: true },
      },
    })

    const restored = formReducer(dirty, {
      type: "edit",
      draft: {
        name: "server-v1",
        parameters: { nested: true, temperature: 0.2 },
      },
    })

    expect(restored.phase).toBe("loaded")
    expect(restored.dirty).toBe(false)
    expect(restored.draft).toEqual(restored.acknowledged)
  })

  it("can mark an invalid editor buffer dirty before it changes the draft", () => {
    const loaded = initialFormState({ parameters: {} }, '"v1"')

    const invalidBuffer = formReducer(loaded, {
      type: "edit",
      draft: loaded.draft,
      forceDirty: true,
    })

    expect(invalidBuffer.phase).toBe("dirty")
    expect(invalidBuffer.dirty).toBe(true)
  })

  it("does not invent unsaved changes when deleting a clean resource fails", () => {
    const loaded = initialFormState({ name: "server-v1" }, '"v1"')

    const blocked = formReducer(loaded, {
      type: "blocked",
      message: "resource is referenced",
      blockers: [{ resource_type: "agent_definition", name: "researcher" }],
    })
    const conflict = formReducer(loaded, {
      type: "conflict",
      message: "revision changed",
    })

    expect(blocked.phase).toBe("invalid")
    expect(blocked.dirty).toBe(false)
    expect(conflict.phase).toBe("conflict")
    expect(conflict.dirty).toBe(false)
  })
})
