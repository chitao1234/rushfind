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
            b"Usage: rfd [global options] [path ...] [expression]\n\n\
Defaults:\n\
  path defaults to . and expression defaults to -print\n\
  adjacent primaries imply -a; -a binds tighter than -o; use , to sequence actions\n\n\
Global options:\n\
  -P -H -L                      symlink handling before start paths (default: -P)\n\
  --help                        show this help text\n\
  --version, -version           show build/version information\n\
  -Olevel                       GNU-compatible parser option; no optimizer effect yet\n\
  -D opts                       lightweight debug diagnostics; use -D help\n\n\
Compatibility options:\n\
  -files0-from FILE|-           read NUL-delimited start paths from FILE or stdin\n\
  -follow                       GNU positional compatibility option; enables logical traversal\n\
  -warn, -nowarn                control warnings for unknown -D debug options\n\
  -noleaf                       accepted for GNU compatibility; no traversal effect today\n\
  -ignore_readdir_race\n\
  -noignore_readdir_race        accepted for GNU compatibility; no runtime effect today\n\n\
Common tests:\n\
  -name/-iname PATTERN          match basenames\n\
  -path/-ipath PATTERN          match whole displayed paths\n\
  -wholename/-iwholename        GNU path aliases for -path/-ipath\n\
  -regex/-iregex PATTERN        whole-path regex match\n\
  -regextype TYPE               emacs, posix-extended, posix-basic, rust, pcre2\n\
  -type/-xtype LIST             file type list such as f,d,l; D is recognized but unsupported\n\
  -context LABEL                recognized for GNU compatibility; unsupported in this build\n\
  -readable -writable -executable\n\
  -uid/-gid N  -user/-group NAME  -nouser -nogroup\n\
  Windows: -owner NAME  -owner-sid SID  -group-sid SID\n\
  -perm MODE  -flags EXPR  -reparse-type TYPE\n\
  -size N  -empty  -used N\n\
  -atime/-ctime/-mtime N  -amin/-cmin/-mmin N\n\
  -newer FILE  -anewer FILE  -cnewer FILE  -newerXY REF  -daystart\n\
  -samefile FILE  -inum N  -links N  -lname/-ilname PATTERN\n\
  -maxdepth N  -mindepth N  -depth  -prune  -xdev/-mount  -fstype TYPE\n\
  -true  -false\n\n\
Actions:\n\
  -print  -print0  -printf FORMAT  -ls\n\
  -fprint FILE  -fprint0 FILE  -fprintf FILE FORMAT  -fls FILE\n\
  -exec ... ;  -exec ... +  -execdir ... ;  -execdir ... +\n\
  -ok ... ;  -okdir ... ;  -delete  -quit\n\n\
Environment:\n\
  RUSHFIND_WORKERS=N            default: host parallelism capped at 4\n\
                                1 keeps ordered mode; values >1 enable relaxed parallel mode\n\
  RUSHFIND_WARNINGS=off         suppress startup warnings\n\n\
Notes:\n\
  -delete implies depth-first removal semantics\n\
  -ok ... + and -okdir ... + are not supported\n\
  Platform-specific predicates may fail during planning with explicit diagnostics\n\
  See rfd(1) for the full reference.\n",
        )
        .map_err(|error| failed_to_write("stdout", error))
}

pub(crate) fn write_debug_help<W: Write>(writer: &mut W) -> Result<(), Diagnostic> {
    writer
        .write_all(
            b"Debug categories accepted by rfd -D:\n\
  exec   opt   rates   search   stat   time   tree   all   help\n\n\
Requested categories currently emit lightweight rushfind diagnostics rather than\n\
GNU findutils' detailed tracing stream.\n",
        )
        .map_err(|error| failed_to_write("stdout", error))
}

#[cfg(test)]
mod tests {
    use super::{BuildVersion, format_version_line, write_debug_help, write_help};

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

    #[test]
    fn help_text_exposes_human_facing_sections() {
        let mut output = Vec::new();
        write_help(&mut output).unwrap();
        let rendered = String::from_utf8(output).unwrap();

        assert!(rendered.contains("Usage: rfd [global options] [path ...] [expression]"));
        assert!(rendered.contains("Compatibility options:"));
        assert!(rendered.contains("Common tests:"));
        assert!(rendered.contains("Actions:"));
        assert!(rendered.contains("Environment:"));
        assert!(rendered.contains("See rfd(1) for the full reference."));
    }

    #[test]
    fn debug_help_mentions_categories_and_lightweight_tracing() {
        let mut output = Vec::new();
        write_debug_help(&mut output).unwrap();
        let rendered = String::from_utf8(output).unwrap();

        assert!(rendered.contains("Debug categories accepted by rfd -D:"));
        assert!(rendered.contains("exec   opt   rates   search"));
        assert!(rendered.contains("lightweight rushfind diagnostics"));
    }
}
