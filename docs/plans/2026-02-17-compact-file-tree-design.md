# Compact Deeply Nested Directory Paths in File Tree

**Issue**: lado-0it
**Date**: 2026-02-17

## Problem

Deeply nested directory paths (common in Java: `src/main/java/com/example/service/impl/`) waste horizontal space and make the file tree hard to scan.

## Solution

Post-process the hierarchical `FileTreeNode` tree after building, merging single-child folder chains into one node with a compacted display name like `src/.../service`.

## Algorithm

1. **Merge single-child chains**: Walk the tree bottom-up. When a folder has exactly one child that is also a folder, merge them. Repeat until chain ends. Only compact chains of 3+ segments (compacting `a/b` to `a/.../b` adds no value).

2. **Disambiguate siblings**: After compaction, check sibling folders for display name collisions. Progressively reveal more segments from the start until unique.

## Display Format

- `first/.../last` for chains of 3+ segments
- Full path preserved in `node.path` for expand/collapse state keying

## Examples

```
a/c/c/b/foo/bar/file.txt  →  a/.../bar/file.txt
b/c/c/b/foo/bar/file.txt  →  b/.../bar/file.txt
```

## Data Flow

```
build_file_tree() → compact_tree() → sort_tree() → flatten_tree_with_state()
```

## Impact

- `FileTreeNode.name`: compacted display name
- `FileTreeNode.path`: full real path (unchanged semantics)
- No changes to flattening, UI, or expand/collapse logic
