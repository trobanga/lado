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

/// Flatten the file tree for display in a ListView
pub fn flatten_tree(nodes: &[FileTreeNode], depth: i32) -> Vec<FlatFileEntry> {
    let mut result = Vec::new();

    for node in nodes {
        result.push(FlatFileEntry {
            name: node.name.clone(),
            path: node.path.clone(),
            depth,
            is_folder: node.is_folder,
            is_expanded: true,
            status: node.status.clone().unwrap_or_else(|| "modified".to_string()),
        });

        if node.is_folder {
            result.extend(flatten_tree(&node.children, depth + 1));
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
}
