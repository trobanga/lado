//! File tree construction from diff data.
//! Builds hierarchical file trees for the UI.

use super::diff::FileChange;
use std::collections::HashMap;

/// A node in the file tree
#[derive(Debug, Clone)]
pub struct FileTreeNode {
    pub name: String,
    pub path: String,
    pub is_folder: bool,
    pub children: Vec<FileTreeNode>,
    pub status: Option<String>,
}

/// Build a hierarchical file tree from a flat list of file changes
pub fn build_file_tree(files: &[FileChange]) -> Vec<FileTreeNode> {
    let mut root: HashMap<String, FileTreeNode> = HashMap::new();

    for file in files {
        let parts: Vec<&str> = file.path.split('/').collect();
        insert_path(&mut root, &parts, &file.path, file.status.as_str(), "");
    }

    // Convert HashMap to sorted Vec
    let mut nodes: Vec<FileTreeNode> = root.into_values().collect();
    compact_tree(&mut nodes);
    sort_tree(&mut nodes);
    nodes
}

fn insert_path(
    nodes: &mut HashMap<String, FileTreeNode>,
    parts: &[&str],
    full_path: &str,
    status: &str,
    prefix: &str,
) {
    if parts.is_empty() {
        return;
    }

    let name = parts[0].to_string();
    let is_file = parts.len() == 1;

    // Build the fully qualified folder path so each folder has a unique identity
    let folder_path = if prefix.is_empty() {
        name.clone()
    } else {
        format!("{prefix}/{name}")
    };

    let node = nodes.entry(name.clone()).or_insert_with(|| FileTreeNode {
        name: name.clone(),
        path: if is_file {
            full_path.to_string()
        } else {
            folder_path.clone()
        },
        is_folder: !is_file,
        children: Vec::new(),
        status: None,
    });

    if is_file {
        node.status = Some(status.to_string());
        node.path = full_path.to_string();
    } else {
        let mut child_map: HashMap<String, FileTreeNode> = node
            .children
            .drain(..)
            .map(|n| (n.name.clone(), n))
            .collect();

        insert_path(&mut child_map, &parts[1..], full_path, status, &folder_path);

        node.children = child_map.into_values().collect();
    }
}

fn sort_tree(nodes: &mut [FileTreeNode]) {
    // Sort: folders first, then alphabetically
    nodes.sort_by(|a, b| {
        match (a.is_folder, b.is_folder) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        }
    });

    for node in nodes {
        sort_tree(&mut node.children);
    }
}

/// Compact single-child folder chains into "first/.../last" display names.
/// This reduces visual clutter when deeply nested folders have only one subfolder.
fn compact_tree(nodes: &mut [FileTreeNode]) {
    for node in nodes.iter_mut() {
        if node.is_folder {
            compact_node(node);
        }
    }
}

/// Recursively compact a single folder node.
/// Merges chains of 3+ single-child folders into "first/.../last".
fn compact_node(node: &mut FileTreeNode) {
    // Compact THIS node's chain first (top-down) to avoid double-compaction
    // when chains span 5+ segments.
    let chain_depth = count_single_child_depth(node);

    if chain_depth >= 2 {
        // Walk down to the leaf folder of the single-child chain.
        let mut leaf = node.children.pop().unwrap();
        while leaf.children.len() == 1 && leaf.children[0].is_folder {
            leaf = leaf.children.pop().unwrap();
        }

        let first = &node.name;
        let last = &leaf.name;
        node.name = format!("{first}/.../{last}");
        node.path = leaf.path;
        node.children = leaf.children;
    }

    // THEN recurse into children (which may themselves have compactable chains).
    for child in node.children.iter_mut() {
        if child.is_folder {
            compact_node(child);
        }
    }
}

/// Count how many single-child folder links descend from `node`.
/// e.g. a -> b -> c -> [files] returns 2 (b and c).
fn count_single_child_depth(node: &FileTreeNode) -> usize {
    if node.children.len() == 1 && node.children[0].is_folder {
        1 + count_single_child_depth(&node.children[0])
    } else {
        0
    }
}

/// Flatten the file tree with explicit expanded state for each folder
/// The expanded_state map uses folder paths as keys, with true = expanded (default), false = collapsed
pub fn flatten_tree_with_state(
    nodes: &[FileTreeNode],
    depth: i32,
    expanded_state: &std::collections::HashMap<String, bool>,
) -> Vec<FlatFileEntry> {
    let mut result = Vec::new();

    for node in nodes {
        // For folders, check if they're expanded (default to true if not in map)
        let is_expanded = if node.is_folder {
            *expanded_state.get(&node.path).unwrap_or(&true)
        } else {
            true
        };

        result.push(FlatFileEntry {
            name: node.name.clone(),
            path: node.path.clone(),
            depth,
            is_folder: node.is_folder,
            is_expanded,
            status: node.status.clone().unwrap_or_else(|| "modified".to_string()),
            comment_count: 0,
            viewed: false,
        });

        // Only recurse into children if the folder is expanded
        if node.is_folder && is_expanded {
            result.extend(flatten_tree_with_state(&node.children, depth + 1, expanded_state));
        }
    }

    result
}

/// A flattened file entry for the UI
#[derive(Debug, Clone)]
pub struct FlatFileEntry {
    pub name: String,
    pub path: String,
    pub depth: i32,
    pub is_folder: bool,
    pub is_expanded: bool,
    pub status: String,
    pub comment_count: i32,
    pub viewed: bool,
}

/// Collect all folder paths from a file tree (for bulk expand/collapse operations)
pub fn collect_folder_paths(nodes: &[FileTreeNode]) -> Vec<String> {
    let mut paths = Vec::new();
    for node in nodes {
        if node.is_folder {
            paths.push(node.path.clone());
            paths.extend(collect_folder_paths(&node.children));
        }
    }
    paths
}

/// Collect folder paths under a specific path (for recursive expand on a subtree)
/// Folder paths are fully qualified (e.g., "src/git" not just "git")
pub fn collect_folder_paths_under(nodes: &[FileTreeNode], target_path: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for node in nodes {
        if node.is_folder {
            if node.path == target_path {
                // Found the target, collect it and all descendants
                paths.push(node.path.clone());
                paths.extend(collect_folder_paths(&node.children));
            } else {
                // Recurse into children to find the target
                paths.extend(collect_folder_paths_under(&node.children, target_path));
            }
        }
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::diff::FileStatus;

    #[test]
    fn test_build_file_tree() {
        let files = vec![
            FileChange {
                path: "src/main.rs".to_string(),
                status: FileStatus::Modified,
                additions: 10,
                deletions: 5,
            },
            FileChange {
                path: "src/lib.rs".to_string(),
                status: FileStatus::Added,
                additions: 20,
                deletions: 0,
            },
            FileChange {
                path: "README.md".to_string(),
                status: FileStatus::Modified,
                additions: 2,
                deletions: 1,
            },
        ];

        let tree = build_file_tree(&files);

        // Should have 2 top-level nodes: src folder and README.md
        assert_eq!(tree.len(), 2);

        // First should be src (folder)
        assert!(tree[0].is_folder);
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[0].children.len(), 2);
    }

    #[test]
    fn test_flatten_tree_respects_expanded_state() {
        let files = vec![
            FileChange {
                path: "src/main.rs".to_string(),
                status: FileStatus::Modified,
                additions: 10,
                deletions: 5,
            },
            FileChange {
                path: "src/lib.rs".to_string(),
                status: FileStatus::Added,
                additions: 20,
                deletions: 0,
            },
            FileChange {
                path: "README.md".to_string(),
                status: FileStatus::Modified,
                additions: 2,
                deletions: 1,
            },
        ];

        let tree = build_file_tree(&files);

        // When all expanded, should have 4 entries: src folder, main.rs, lib.rs, README.md
        let mut expanded_state = std::collections::HashMap::new();
        let flat_expanded = flatten_tree_with_state(&tree, 0, &expanded_state);
        assert_eq!(flat_expanded.len(), 4);
        assert_eq!(flat_expanded[0].name, "src");
        assert!(flat_expanded[0].is_folder);
        assert!(flat_expanded[0].is_expanded);

        // When src is collapsed, should have 2 entries: src folder, README.md
        expanded_state.insert("src".to_string(), false);
        let flat_collapsed = flatten_tree_with_state(&tree, 0, &expanded_state);
        assert_eq!(flat_collapsed.len(), 2);
        assert_eq!(flat_collapsed[0].name, "src");
        assert!(!flat_collapsed[0].is_expanded);
        assert_eq!(flat_collapsed[1].name, "README.md");
    }

    #[test]
    fn test_collect_folder_paths() {
        let files = vec![
            FileChange {
                path: "src/main.rs".to_string(),
                status: FileStatus::Modified,
                additions: 10,
                deletions: 5,
            },
            FileChange {
                path: "src/git/diff.rs".to_string(),
                status: FileStatus::Added,
                additions: 20,
                deletions: 0,
            },
            FileChange {
                path: "tests/test.rs".to_string(),
                status: FileStatus::Modified,
                additions: 5,
                deletions: 2,
            },
        ];

        let tree = build_file_tree(&files);
        let paths = collect_folder_paths(&tree);

        // Should have 3 folders: src, src/git (nested under src), tests
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&"src".to_string()));
        assert!(paths.contains(&"src/git".to_string()));
        assert!(paths.contains(&"tests".to_string()));
    }

    #[test]
    fn test_collect_folder_paths_under() {
        let files = vec![
            FileChange {
                path: "src/git/diff.rs".to_string(),
                status: FileStatus::Modified,
                additions: 10,
                deletions: 5,
            },
            FileChange {
                path: "src/git/repo.rs".to_string(),
                status: FileStatus::Added,
                additions: 20,
                deletions: 0,
            },
            FileChange {
                path: "src/app.rs".to_string(),
                status: FileStatus::Modified,
                additions: 5,
                deletions: 2,
            },
        ];

        let tree = build_file_tree(&files);

        // Collecting under "src" should get src and src/git
        let paths_under_src = collect_folder_paths_under(&tree, "src");
        assert_eq!(paths_under_src.len(), 2);
        assert!(paths_under_src.contains(&"src".to_string()));
        assert!(paths_under_src.contains(&"src/git".to_string()));

        // Collecting under "src/git" should only get src/git
        let paths_under_git = collect_folder_paths_under(&tree, "src/git");
        assert_eq!(paths_under_git.len(), 1);
        assert!(paths_under_git.contains(&"src/git".to_string()));
    }

    #[test]
    fn test_compact_single_child_chain() {
        // A chain of 3+ single-child folders should be compacted into "a/.../c"
        let files = vec![FileChange {
            path: "a/b/c/file.txt".to_string(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
        }];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a/.../c");
        assert!(tree[0].is_folder);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "file.txt");
        assert!(!tree[0].children[0].is_folder);
    }

    #[test]
    fn test_no_compact_two_segments() {
        // Only 2 folder segments — should NOT be compacted
        let files = vec![FileChange {
            path: "a/b/file.txt".to_string(),
            status: FileStatus::Added,
            additions: 5,
            deletions: 0,
        }];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a");
        assert!(tree[0].is_folder);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "b");
        assert!(tree[0].children[0].is_folder);
        assert_eq!(tree[0].children[0].children.len(), 1);
        assert_eq!(tree[0].children[0].children[0].name, "file.txt");
    }

    #[test]
    fn test_no_compact_when_multiple_children() {
        // Folder "a" has two children — should NOT be compacted
        let files = vec![
            FileChange {
                path: "a/b/file1.txt".to_string(),
                status: FileStatus::Modified,
                additions: 3,
                deletions: 1,
            },
            FileChange {
                path: "a/c/file2.txt".to_string(),
                status: FileStatus::Added,
                additions: 7,
                deletions: 0,
            },
        ];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a");
        assert!(tree[0].is_folder);
        assert_eq!(tree[0].children.len(), 2);
        // Children should not be compacted — both "b" and "c" are direct children
        let child_names: Vec<&str> = tree[0].children.iter().map(|c| c.name.as_str()).collect();
        assert!(child_names.contains(&"b"));
        assert!(child_names.contains(&"c"));
    }

    #[test]
    fn test_compact_deep_chain_single_ellipsis() {
        // 5 folder segments should produce single ellipsis, not double
        let files = vec![FileChange {
            path: "a/b/c/d/e/file.txt".to_string(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
        }];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "a/.../e");
        assert!(tree[0].is_folder);
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "file.txt");
    }

    #[test]
    fn test_compact_disambiguates_siblings() {
        // User's exact example: a/c/c/b/foo/bar and b/c/c/b/foo/bar
        // Should produce "a/.../bar" and "b/.../bar" — distinct first segments
        let files = vec![
            FileChange {
                path: "a/c/c/b/foo/bar/file1.txt".to_string(),
                status: FileStatus::Modified,
                additions: 1,
                deletions: 0,
            },
            FileChange {
                path: "b/c/c/b/foo/bar/file2.txt".to_string(),
                status: FileStatus::Added,
                additions: 1,
                deletions: 0,
            },
        ];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 2);
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"a/.../bar"));
        assert!(names.contains(&"b/.../bar"));
    }

    #[test]
    fn test_compact_no_disambiguation_needed() {
        // Two distinct chains — different leaf names, no collision possible
        let files = vec![
            FileChange {
                path: "a/b/c/d/file1.txt".to_string(),
                status: FileStatus::Modified,
                additions: 1,
                deletions: 0,
            },
            FileChange {
                path: "x/y/z/w/file2.txt".to_string(),
                status: FileStatus::Added,
                additions: 1,
                deletions: 0,
            },
        ];

        let tree = build_file_tree(&files);

        assert_eq!(tree.len(), 2);
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert!(names.contains(&"a/.../d"));
        assert!(names.contains(&"x/.../w"));
    }

    #[test]
    fn test_compact_nested_chains_under_common_parent() {
        // Common parent "src" with two children that each have compactable chains
        let files = vec![
            FileChange {
                path: "src/a/b/c/file1.txt".to_string(),
                status: FileStatus::Modified,
                additions: 1,
                deletions: 0,
            },
            FileChange {
                path: "src/x/y/z/file2.txt".to_string(),
                status: FileStatus::Added,
                additions: 1,
                deletions: 0,
            },
        ];

        let tree = build_file_tree(&files);

        // "src" has 2 children, so it doesn't compact
        assert_eq!(tree.len(), 1);
        assert_eq!(tree[0].name, "src");
        assert_eq!(tree[0].children.len(), 2);
        // Each child compacts independently
        let child_names: Vec<&str> = tree[0].children.iter().map(|n| n.name.as_str()).collect();
        assert!(child_names.contains(&"a/.../c"));
        assert!(child_names.contains(&"x/.../z"));
    }

    #[test]
    fn test_compact_tree_with_flatten_and_expand_state() {
        let files = vec![
            FileChange {
                path: "src/main/java/com/example/Service.java".to_string(),
                status: FileStatus::Modified,
                additions: 10,
                deletions: 5,
            },
        ];

        let tree = build_file_tree(&files);

        // Should compact to one folder node with compacted name
        assert_eq!(tree.len(), 1);
        assert!(tree[0].name.contains("..."));
        assert_eq!(tree[0].children.len(), 1);
        assert_eq!(tree[0].children[0].name, "Service.java");

        // Flatten with default expanded state — should show compacted folder + file
        let expanded_state = std::collections::HashMap::new();
        let flat = flatten_tree_with_state(&tree, 0, &expanded_state);
        assert_eq!(flat.len(), 2);
        assert!(flat[0].name.contains("..."));
        assert!(flat[0].is_folder);
        assert!(flat[0].is_expanded);
        assert_eq!(flat[0].depth, 0);
        assert_eq!(flat[1].name, "Service.java");
        assert_eq!(flat[1].depth, 1);

        // Collapse the compacted folder using its path — should only show the folder
        let mut collapsed_state = std::collections::HashMap::new();
        collapsed_state.insert(tree[0].path.clone(), false);
        let flat_collapsed = flatten_tree_with_state(&tree, 0, &collapsed_state);
        assert_eq!(flat_collapsed.len(), 1);
        assert!(!flat_collapsed[0].is_expanded);
    }

    #[test]
    fn test_flat_entry_comment_count_default() {
        let files = vec![FileChange {
            path: "src/main.rs".to_string(),
            status: FileStatus::Modified,
            additions: 1,
            deletions: 0,
        }];

        let tree = build_file_tree(&files);
        let flat = flatten_tree_with_state(&tree, 0, &std::collections::HashMap::new());

        for entry in &flat {
            assert_eq!(
                entry.comment_count, 0,
                "entry '{}' should have 0 comments",
                entry.name
            );
        }
    }
}
