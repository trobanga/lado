mod diff;
mod file_tree;
mod repository;

pub use diff::{CommentData, DiffData, DiffLine, DiffLineType};
pub use file_tree::{
    build_file_tree, flatten_tree_with_state, FileTreeNode, FlatFileEntry,
};
pub use repository::Repository;
