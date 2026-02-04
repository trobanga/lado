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
        insert_path(&mut root, &parts, &file.path, file.status.as_str());
    }

    // Convert HashMap to sorted Vec
    let mut nodes: Vec<FileTreeNode> = root.into_values().collect();
    sort_tree(&mut nodes);
    nodes
}

fn insert_path(
    nodes: &mut HashMap<String, FileTreeNode>,
    parts: &[&str],
    full_path: &str,
    status: &str,
) {
    if parts.is_empty() {
        return;
    }

    let name = parts[0].to_string();
    let is_file = parts.len() == 1;

    let node = nodes.entry(name.clone()).or_insert_with(|| FileTreeNode {
        name: name.clone(),
        path: if is_file {
            full_path.to_string()
        } else {
            parts[..1].join("/")
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

        insert_path(&mut child_map, &parts[1..], full_path, status);

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
/// Note: folder paths in the tree use just the folder name, not full paths
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

        // Should have 3 folders: src, git (nested under src), tests
        // Note: nested folder paths are just the folder name, not full path
        assert_eq!(paths.len(), 3);
        assert!(paths.contains(&"src".to_string()));
        assert!(paths.contains(&"git".to_string())); // nested folder uses just name
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

        // Collecting under "src" should get src and git (nested folder name)
        let paths_under_src = collect_folder_paths_under(&tree, "src");
        assert_eq!(paths_under_src.len(), 2);
        assert!(paths_under_src.contains(&"src".to_string()));
        assert!(paths_under_src.contains(&"git".to_string()));

        // Collecting under "git" (nested folder) should only get git
        let paths_under_git = collect_folder_paths_under(&tree, "git");
        assert_eq!(paths_under_git.len(), 1);
        assert!(paths_under_git.contains(&"git".to_string()));
    }
}
