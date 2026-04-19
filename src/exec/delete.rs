use crate::diagnostics::Diagnostic;
use std::fs;
use std::path::Path;

pub(crate) fn delete_path(path: &Path) -> Result<bool, Diagnostic> {
    let file_type = fs::symlink_metadata(path)
        .map_err(|error| Diagnostic::new(format!("{}: {error}", path.display()), 1))?
        .file_type();

    let result = if file_type.is_dir() {
        fs::remove_dir(path)
    } else {
        fs::remove_file(path)
    };

    match result {
        Ok(()) => Ok(true),
        Err(error) => Err(Diagnostic::new(format!("{}: {error}", path.display()), 1)),
    }
}
