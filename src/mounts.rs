pub(crate) use crate::platform::filesystem::FilesystemSnapshot as MountSnapshot;

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
