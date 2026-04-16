use crate::diagnostics::Diagnostic;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::{OsStr, OsString};
use std::fs;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MountSnapshot {
    types_by_mount_id: BTreeMap<u64, OsString>,
    known_types: BTreeSet<OsString>,
}

impl MountSnapshot {
    pub(crate) fn load_proc_self_mountinfo() -> Result<Self, Diagnostic> {
        let mountinfo = fs::read_to_string("/proc/self/mountinfo")
            .map_err(|error| Diagnostic::new(format!("/proc/self/mountinfo: {error}"), 1))?;
        Self::from_mountinfo(&mountinfo)
    }

    pub(crate) fn from_mountinfo(mountinfo: &str) -> Result<Self, Diagnostic> {
        let mut snapshot = Self::default();

        for line in mountinfo.lines().filter(|line| !line.trim().is_empty()) {
            let (left, right) = line
                .split_once(" - ")
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?;
            let mount_id = left
                .split_whitespace()
                .next()
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?
                .parse::<u64>()
                .map_err(|_| Diagnostic::new(format!("invalid mount ID in `{line}`"), 1))?;
            let file_system_type = right
                .split_whitespace()
                .next()
                .ok_or_else(|| Diagnostic::new(format!("invalid mountinfo line `{line}`"), 1))?;

            let type_name = OsString::from(file_system_type);
            snapshot.known_types.insert(type_name.clone());
            snapshot.types_by_mount_id.insert(mount_id, type_name);
        }

        Ok(snapshot)
    }

    pub(crate) fn knows_type(&self, type_name: &OsStr) -> bool {
        self.known_types.contains(type_name)
    }

    pub(crate) fn type_for_mount_id(&self, mount_id: u64) -> Option<&OsStr> {
        self.types_by_mount_id
            .get(&mount_id)
            .map(|type_name| type_name.as_os_str())
    }
}

#[cfg(test)]
mod tests {
    use super::MountSnapshot;
    use std::ffi::OsStr;

    #[test]
    fn parses_mountinfo_ids_and_type_names() {
        let snapshot = MountSnapshot::from_mountinfo(concat!(
            "23 1 8:1 / / rw - ext4 /dev/root rw\n",
            "24 23 0:21 / /run rw - tmpfs tmpfs rw,size=65536k\n",
            "25 23 0:44 / /ssh rw - fuse.sshfs sshfs rw\n",
        ))
        .unwrap();

        assert_eq!(snapshot.type_for_mount_id(23), Some(OsStr::new("ext4")));
        assert_eq!(snapshot.type_for_mount_id(24), Some(OsStr::new("tmpfs")));
        assert_eq!(
            snapshot.type_for_mount_id(25),
            Some(OsStr::new("fuse.sshfs"))
        );
        assert!(snapshot.knows_type(OsStr::new("tmpfs")));
        assert!(!snapshot.knows_type(OsStr::new("btrfs")));
    }

    #[test]
    fn rejects_mountinfo_lines_without_separator_or_type() {
        let error =
            MountSnapshot::from_mountinfo("23 1 8:1 / / rw ext4 /dev/root rw\n").unwrap_err();

        assert!(error.message.contains("invalid mountinfo line"));
    }
}
