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
            b"Usage: rfd [-H] [-L] [-P] [-Olevel] [-D debugopts] [path...] [expression]\n\n\
Expression operators follow GNU find precedence. Supported compatibility options include:\n\
  --help --version -version\n\
  -files0-from FILE -noleaf -warn -nowarn\n\
  -ignore_readdir_race -noignore_readdir_race\n\n\
Debug options accepted by -D: exec,opt,rates,search,stat,time,tree,all,help\n",
        )
        .map_err(|error| failed_to_write("stdout", error))
}

pub(crate) fn write_debug_help<W: Write>(writer: &mut W) -> Result<(), Diagnostic> {
    writer
        .write_all(
            b"Debug options accepted by rfd -D:\n\
  exec opt rates search stat time tree all help\n\n\
Detailed GNU find debug tracing is not implemented; requested categories emit lightweight internal diagnostics.\n",
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
