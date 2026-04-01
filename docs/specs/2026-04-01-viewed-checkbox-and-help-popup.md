# Viewed Checkbox & Help Popup

**Date:** 2026-04-01
**Status:** Approved

## Overview

Two features inspired by GitHub's PR review UX:
1. A per-file "viewed" checkbox that persists across sessions and skips viewed files during J/K navigation
2. A centered help overlay triggered by `?` showing all keybindings

---

## Feature 1: Viewed Checkbox

### Data Model

Add `viewed: bool` field to the file entry pipeline:
- `FileEntry` in `structs.slint`
- `FlatFileEntry` in `git/file_tree.rs`
- `FileEntryModel` in `models/file_tree_model.rs`

### Persistence

- Store viewed state in `~/.config/lado/viewed_state.json`
- Structure: map of diff target key → (file path → content hash)
- Diff target key: string representation of the current diff target (branch name, commit, PR number)
- Content hash: hash of the diff lines for that file (use a fast hash, e.g., a simple hash of the concatenated diff line content)
- On launch: load state, compare hashes. If hash matches → `viewed = true`. If mismatch or missing → `viewed = false`
- On toggle: update map, persist to disk

### UI Changes (file_tree.slint)

- Add a checkmark icon to each `TreeItem`, positioned after the `StatusBadge` and before the filename
- When `viewed == false`: show an empty checkbox outline (or no icon)
- When `viewed == true`: show a filled checkmark, dim the filename text (use `theme.text-muted`)
- Folders do not show a checkbox

### Keyboard

- `v` toggles viewed state on the currently focused file (no effect on folders)
- `J` (next file) skips files where `viewed == true`
- `K` (previous file) skips files where `viewed == true`
- If all remaining files are viewed, J/K do nothing (don't wrap or get stuck)

### Callbacks

- New `toggle-viewed(index: int)` callback: Slint → Rust
- Rust side: toggle viewed state in memory, update hash map, persist, update UI model
- Modify existing `find-next-file(current-idx: int, direction: int) -> int` to skip viewed files

### Edge Cases

- If a file is viewed, then the diff target changes (e.g., new commits pushed), hashes won't match → auto-unviewed
- Toggling viewed on a folder: no-op
- All files viewed: J/K navigation stops (returns current index)

---

## Feature 2: Help Popup

### UI (new component: help_overlay.slint)

- Semi-transparent black backdrop covering entire window
- Centered white/dark card (themed) with rounded corners
- Header: "lado — Keyboard Shortcuts" (or similar)
- Body: two-column layout (key | description), grouped by category
- Scrollable via Flickable if content overflows
- Close button (X) in top-right corner of card

### Keybinding Categories

1. **Navigation**: j/k (scroll), J/K (next/prev file), [/] (prev/next commit)
2. **View**: u (unified), s (side-by-side)
3. **File Tree**: e (toggle folder), E (expand all), c (collapse all), C (expand recursive), v (toggle viewed)
4. **Other**: F11 (fullscreen), Enter (select file), ? (this help)

### Integration (main.slint)

- Add `help-visible: bool` property to MainWindow
- In main FocusScope: `?` toggles `help-visible`
- When `help-visible == true`: block all other keyboard input except `?` and `Escape` (same pattern as settings panel)
- `?` and `Escape` both close the overlay
- Clicking the backdrop also closes the overlay

### Content Source

- Keybinding labels read from `AppSettings` so they reflect user customizations
- Descriptions are static strings

---

## Files to Create

- `ui/components/help_overlay.slint` — new help overlay component

## Files to Modify

- `ui/structs.slint` — add `viewed: bool` to FileEntry
- `ui/components/file_tree.slint` — add checkbox rendering, dimmed style for viewed files
- `ui/main.slint` — add help-visible property, ? key handler, help overlay, v key handler, modify J/K to skip viewed
- `src/git/file_tree.rs` — add `viewed: bool` to FlatFileEntry
- `src/models/file_tree_model.rs` — add viewed field to conversion
- `src/app.rs` — viewed state management, persistence, new callbacks, modify find_next_file
- `src/config.rs` — viewed state persistence (or new module)
