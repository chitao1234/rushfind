use findoxide::entry::EntryContext;
use findoxide::file_output::{FileOutputTerminator, render_file_print_bytes};
use findoxide::output::render_output_bytes;
use findoxide::planner::OutputAction;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

fn entry_for(raw: &[u8]) -> EntryContext {
    EntryContext::new(PathBuf::from(OsString::from_vec(raw.to_vec())), 0, true)
}

#[test]
fn print_family_renderers_preserve_non_utf8_path_bytes() {
    let entry = entry_for(b"./bad-\xff.txt");

    assert_eq!(
        render_output_bytes(OutputAction::Print, &entry),
        b"./bad-\xff.txt\n"
    );
    assert_eq!(
        render_output_bytes(OutputAction::Print0, &entry),
        b"./bad-\xff.txt\0"
    );
}

#[test]
fn file_print_family_renderers_preserve_non_utf8_path_bytes() {
    let entry = entry_for(b"./bad-\xfe.bin");

    assert_eq!(
        render_file_print_bytes(&entry, FileOutputTerminator::Newline),
        b"./bad-\xfe.bin\n"
    );
    assert_eq!(
        render_file_print_bytes(&entry, FileOutputTerminator::Nul),
        b"./bad-\xfe.bin\0"
    );
}
