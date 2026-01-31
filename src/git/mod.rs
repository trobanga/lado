mod diff;
mod file_tree;
mod repository;

pub use diff::{DiffData, DiffLine, DiffLineType};
pub use file_tree::{build_file_tree, flatten_tree, FlatFileEntry};
pub use repository::Repository;
