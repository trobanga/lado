use crate::git::FileChange;
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

impl From<&FileChange> for FileEntryModel {
    fn from(file: &FileChange) -> Self {
        let name = file
            .path
            .rsplit('/')
            .next()
            .unwrap_or(&file.path)
            .to_string();

        Self {
            name,
            path: file.path.clone(),
            depth: 0,
            is_folder: false,
            is_expanded: true,
            status: file.status.as_str().to_string(),
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
