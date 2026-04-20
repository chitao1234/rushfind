use crate::diagnostics::Diagnostic;
use crate::platform::filesystem;
use crate::time::Timestamp;
use std::path::Path;

pub fn read_birth_time(path: &Path, follow: bool) -> Result<Option<Timestamp>, Diagnostic> {
    filesystem::read_birth_time(path, follow)
}
