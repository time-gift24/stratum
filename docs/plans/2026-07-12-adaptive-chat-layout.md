# Adaptive Chat Layout Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the Navbar and chat canvas share one responsive centered width, while keeping the desktop history rail from changing the chat canvas geometry.

**Architecture:** Add a small set of global CSS layout tokens and a reusable content-width utility. Use that utility for the Navbar and Chat shell. Keep history in normal flow below the wide-desktop threshold, then take it out of flow and anchor it beside the centered Chat shell when the viewport has room.

**Tech Stack:** React, TypeScript, Tailwind CSS v4 utilities, CSS `clamp()`/`min()`, Vitest.

---

### Task 1: Add responsive layout tokens

**Files:**
- Modify: `wyse-web/app/app.css`

**Step 1: Add the minimal global tokens**

Define page gutter, shared content width capped at `64rem`, layout gap, history width, and body text size in `:root`. Add a `.wyse-content-width` utility that uses the token and prevents overflow.

**Step 2: Apply the body text token**

Set `body` font size to the responsive body token without changing existing component-specific type classes.

**Step 3: Add the wide-desktop history positioning rule**

At `min-width: 1680px`, add a utility/rule that takes the history rail out of flow and anchors it to the left of the centered Chat shell. Keep the default/mobile rule in normal flow; this threshold leaves enough room for the 1024px chat canvas, 288px history rail, and gutters.

**Step 4: Run formatting/checks**

Run: `cd wyse-web && pnpm exec prettier --check app/app.css`

Expected: PASS.

### Task 2: Make Navbar and Chat share the width token

**Files:**
- Modify: `wyse-web/app/components/site-navbar.tsx`
- Modify: `wyse-web/app/components/chat-workspace.tsx`

**Step 1: Update Navbar container classes**

Replace the hard-coded `max-w-5xl` constraint with the shared content-width utility. Keep the fixed positioning and existing GSAP behavior unchanged.

**Step 2: Update Chat shell classes**

Replace the `max-w-7xl` constraint with the same content-width utility. Use the shared gap token where the shell currently uses the desktop gap.

**Step 3: Isolate the history rail**

Add the history-rail class to the existing history `Card`. Keep the Chat main node and its internal classes unchanged. At wide desktop it should be positioned beside the shell; below that threshold it should remain in normal flow above Chat.

**Step 4: Run the focused component tests**

Run: `cd wyse-web && pnpm test -- app/components/chat-workspace.test.tsx app/components/site-navbar.test.ts`

Expected: PASS.

### Task 3: Lock the layout contract with source-level tests

**Files:**
- Modify: `wyse-web/app/components/chat-workspace.test.tsx`
- Modify: `wyse-web/app/components/site-navbar.test.ts`

**Step 1: Add Navbar token assertion**

Assert that the Navbar source uses the shared content-width utility and no longer uses `max-w-5xl`.

**Step 2: Add Chat/history isolation assertions**

Assert that Chat uses the shared content-width utility, history has the isolation class, and `data-slot="chat-main"` retains its existing layout class.

**Step 3: Run the focused tests**

Run: `cd wyse-web && pnpm test -- app/components/chat-workspace.test.tsx app/components/site-navbar.test.ts`

Expected: PASS.

### Task 4: Verify the frontend change

**Files:**
- No new files.

**Step 1: Format and lint the touched frontend files**

Run: `cd wyse-web && pnpm exec prettier --check app/app.css app/components/site-navbar.tsx app/components/chat-workspace.tsx app/components/chat-workspace.test.tsx app/components/site-navbar.test.ts`

Expected: PASS.

**Step 2: Run the frontend test suite**

Run: `cd wyse-web && pnpm test`

Expected: PASS.

**Step 3: Inspect the final diff**

Run: `git diff --check && git diff --stat`

Expected: no whitespace errors; only the intended frontend layout files are changed.
