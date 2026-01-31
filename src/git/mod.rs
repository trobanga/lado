mod diff;
mod file_tree;
mod repository;

pub use diff::{DiffLine, DiffLineType, FileChange};
pub use repository::Repository;

// Re-export for future use
#[allow(unused_imports)]
pub use diff::{DiffData, DiffHunk, FileStatus};
#[allow(unused_imports)]
pub use file_tree::{build_file_tree, flatten_tree, FlatFileEntry, FileTreeNode};
