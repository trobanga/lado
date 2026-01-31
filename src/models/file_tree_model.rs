use crate::git::FlatFileEntry;
use crate::FileEntry;

/// Model for a file entry in the UI
pub struct FileEntryModel {
    pub name: String,
    pub path: String,
    pub depth: i32,
    pub is_folder: bool,
    pub is_expanded: bool,
    pub status: String,
}

impl From<&FlatFileEntry> for FileEntryModel {
    fn from(entry: &FlatFileEntry) -> Self {
        Self {
            name: entry.name.clone(),
            path: entry.path.clone(),
            depth: entry.depth,
            is_folder: entry.is_folder,
            is_expanded: entry.is_expanded,
            status: entry.status.clone(),
        }
    }
}

impl From<FileEntryModel> for FileEntry {
    fn from(model: FileEntryModel) -> Self {
        Self {
            name: model.name.into(),
            path: model.path.into(),
            depth: model.depth,
            is_folder: model.is_folder,
            is_expanded: model.is_expanded,
            status: model.status.into(),
        }
    }
}
