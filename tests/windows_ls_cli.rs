#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::{escape_ls_rendered_path, file_owner_name, ownership_probe_available};
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

#[test]
fn ls_uses_windows_owner_only_record_shape() {
    if !ownership_probe_available() {
        eprintln!("skipping Windows ls CLI test: security descriptor query unavailable");
        return;
    }

    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();
    let owner = file_owner_name(&file);

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(file.as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-ls".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    let line = String::from_utf8(output.stdout)
        .unwrap()
        .trim_end_matches('\n')
        .trim_end_matches('\r')
        .to_owned();

    let rewritten = line.replacen(&owner, "<OWNER>", 1);
    let columns = rewritten.split_whitespace().collect::<Vec<_>>();
    assert!(columns.len() >= 9, "{rewritten:?}");
    assert!(columns[0].parse::<u64>().is_ok(), "{rewritten:?}");
    assert!(columns[1].parse::<u64>().is_ok(), "{rewritten:?}");
    assert_eq!(columns[2].len(), 8, "{rewritten:?}");
    assert!(columns[3].parse::<u64>().is_ok(), "{rewritten:?}");
    assert_eq!(columns[4], "<OWNER>", "{rewritten:?}");
    assert!(columns[5].parse::<u64>().is_ok(), "{rewritten:?}");
    assert!(
        rewritten.ends_with(&escape_ls_rendered_path(&file.display().to_string())),
        "{rewritten:?}"
    );

    let alloc_needle = format!(" {} ", columns[1]);
    let alloc_start = line.find(&alloc_needle).unwrap() + 1;
    let fileid_field = &line[..alloc_start - 1];
    assert!(fileid_field.len() >= 18, "{line:?}");
    assert_eq!(fileid_field.trim(), columns[0], "{line:?}");
}
