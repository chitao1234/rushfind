use crate::diagnostics::{Diagnostic, failed_to_write};
use std::io::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildVersion<'a> {
    pub(crate) name: &'a str,
    pub(crate) version: &'a str,
    pub(crate) commit: &'a str,
    pub(crate) target: &'a str,
}

pub(crate) fn current_build_version() -> BuildVersion<'static> {
    BuildVersion {
        name: "rushfind",
        version: env!("RUSHFIND_BUILD_VERSION"),
        commit: env!("RUSHFIND_BUILD_GIT_HASH"),
        target: env!("RUSHFIND_BUILD_TARGET"),
    }
}

pub(crate) fn format_version_line(build: BuildVersion<'_>) -> String {
    let commit = if build.commit.is_empty() {
        "unknown"
    } else {
        build.commit
    };

    format!(
        "{} {} (commit {}, target {})",
        build.name, build.version, commit, build.target
    )
}

pub(crate) fn write_version_line<W: Write>(writer: &mut W) -> Result<(), Diagnostic> {
    writeln!(writer, "{}", format_version_line(current_build_version()))
        .map_err(|error| failed_to_write("stdout", error))
}

pub(crate) fn write_help<W: Write>(writer: &mut W) -> Result<(), Diagnostic> {
    writer
        .write_all(
            b"Usage: rfd [OPTIONS] [PATH...] [EXPRESSION]\n\n\
Find files by walking PATHs and evaluating EXPRESSION for each entry.\n\
Default PATH is . and default EXPRESSION is -print.\n\n\
Options:\n\
  -H, -L, -P                 control symlink following before PATHs\n\
  -OLEVEL                    accept GNU optimizer levels for compatibility\n\
  -D LIST                    enable lightweight debug diagnostics; use -D help\n\
  --help                     show this help and exit\n\
  --version, -version         show version and exit\n\n\
Expressions:\n\
  Tests:      -name PATTERN, -path PATTERN, -type LIST, -size N, -mtime N, ...\n\
  Actions:    -print, -print0, -printf FORMAT, -exec COMMAND ;, -delete, -quit\n\
  Operators:  ( EXPR ), ! EXPR, EXPR -a EXPR, EXPR -o EXPR, EXPR , EXPR\n\
  Controls:   -maxdepth N, -mindepth N, -depth, -prune, -xdev, -follow\n\n\
Compatibility options:\n\
  -files0-from FILE, -noleaf, -warn, -nowarn,\n\
  -ignore_readdir_race, -noignore_readdir_race\n\n\
Examples:\n\
  rfd src -name '*.rs' -print\n\
  rfd . -type f -size +1M -print0\n\
  rfd . -name target -prune -o -type f -print\n\n\
See rfd(1) for the full command reference.\n",
        )
        .map_err(|error| failed_to_write("stdout", error))
}

pub(crate) fn write_debug_help<W: Write>(writer: &mut W) -> Result<(), Diagnostic> {
    writer
        .write_all(
            b"Debug diagnostics for rfd -D LIST:\n\n\
Valid names:\n\
  exec,opt,rates,search,stat,time,tree,all,help\n\n\
Debug options:\n\
  Lightweight internal diagnostics are emitted for the requested categories.\n\
  Full GNU findutils debug tracing is not implemented.\n\n\
Example:\n\
  rfd -D search . -maxdepth 1 -print\n",
        )
        .map_err(|error| failed_to_write("stdout", error))
}

#[cfg(test)]
mod tests {
    use super::{BuildVersion, format_version_line};

    #[test]
    fn format_version_line_renders_the_expected_shape() {
        let rendered = format_version_line(BuildVersion {
            name: "rushfind",
            version: "0.1.1-dev",
            commit: "abc1234",
            target: "x86_64-unknown-linux-gnu",
        });

        assert_eq!(
            rendered,
            "rushfind 0.1.1-dev (commit abc1234, target x86_64-unknown-linux-gnu)"
        );
    }

    #[test]
    fn format_version_line_falls_back_to_unknown_for_empty_commit() {
        let rendered = format_version_line(BuildVersion {
            name: "rushfind",
            version: "0.1.1-dev",
            commit: "",
            target: "x86_64-pc-windows-gnu",
        });

        assert_eq!(
            rendered,
            "rushfind 0.1.1-dev (commit unknown, target x86_64-pc-windows-gnu)"
        );
    }
}
