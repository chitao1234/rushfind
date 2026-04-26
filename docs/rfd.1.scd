rfd(1) "rushfind" "User Commands"

# NAME

rfd - rushfind file finder with GNU-find-oriented syntax and parallel traversal

# SYNOPSIS

*rfd* [_global-options_] [_path_ ...] [_expression_]

# DESCRIPTION

*rfd* is the installed command for *rushfind*, a Rust implementation of Unix
*find*(1) that keeps a GNU-oriented command line while adding a parallel
traversal engine.

If no _path_ arguments are given, *rfd* searches the current directory. If no
_expression_ is given, *rfd* behaves as if -print had been supplied.

Expression parsing follows GNU *find*(1) precedence:

- Adjacent primaries imply -a.
- -a binds tighter than -o.
- , sequences expressions and preserves side effects.
- Parentheses group subexpressions.

The default symlink mode is -P.

# GLOBAL OPTIONS

- *-P*, *-H*, *-L*
  Control symlink handling. -P is the default. -H follows symlinks only for
  command-line roots. -L follows symlinks during traversal.

- *--help*
  Print the built-in quick reference and exit.

- *--version*, *-version*
  Print build and target metadata and exit.

- *-Olevel*
  Accepted for GNU compatibility. The current implementation parses and records
  the level but does not change optimizer behavior.

- *-D* _opts_
  Request debug categories. Use -D help for the accepted category names.
  Current debug output is a lightweight rushfind-specific diagnostic stream, not
  GNU findutils' detailed trace output.

# COMPATIBILITY OPTIONS

- *-files0-from* _file_
  Read NUL-delimited start paths from _file_. Use - to read them from standard
  input.

- *-follow*
  Accepted as a GNU positional compatibility option. In the current
  implementation it effectively enables logical traversal.

- *-warn*, *-nowarn*
  Control warnings for unknown -D debug categories.

- *-noleaf*
  Accepted for GNU compatibility. It does not currently change traversal.

- *-ignore_readdir_race*, *-noignore_readdir_race*
  Accepted and recorded for GNU compatibility. They do not currently change
  runtime handling of disappearing entries.

*rfd* rejects explicit command-line start paths together with *-files0-from*.
*-files0-from -* also cannot be combined with *-ok* or *-okdir*, because both
would need standard input.

# TESTS AND ACTIONS

## Structural and boolean

- (, ), !, -not, -a, -and, -o, -or, ,
- -true, -false
- -maxdepth, -mindepth, -depth, -prune, -xdev, -mount

The -xdev and -mount tests are treated as traversal-wide limits in the current
implementation rather than GNU's positional controls.

## Name, path, and regex matching

- -name, -iname
- -path, -ipath
- -wholename, -iwholename
- -lname, -ilname
- -regex, -iregex
- -regextype
- -context

The -regex and -iregex tests match the whole displayed path. Supported
regextype values are emacs, posix-extended, posix-basic, rust, and pcre2.

The -context predicate is recognized for GNU compatibility but reports an
unsupported diagnostic in this build.

## Type and filesystem matching

- -type, -xtype
- -fstype

The -type and -xtype tests accept GNU-style comma lists such as -type f,d. The
Solaris door type letter D is recognized for compatibility but reports an
unsupported diagnostic in this build.

## Identity and ownership matching

- -samefile
- -inum
- -links
- -uid, -gid
- -user, -group
- -nouser, -nogroup
- Windows only: -owner, -owner-sid, -group-sid

On Windows, -uid and -gid are not supported. Use -owner-sid and -group-sid
when raw SID matching is needed.

## Permissions, access, flags, and reparse points

- -perm
- -readable, -writable, -executable
- -flags
- Windows only: -reparse-type

Access predicates use kernel access checks and may intentionally differ from
the -perm test.

Supported -reparse-type names are symbolic, mount-point, app-exec-link,
wsl-symlink, af-unix, cloud, projfs, and other.

## Size and time matching

- -size
- -empty
- -used
- -atime, -ctime, -mtime
- -amin, -cmin, -mmin
- -newer, -anewer, -cnewer
- -newerXY
- -daystart

Time predicates and -used accept GNU-style fractional magnitudes such as 0.5,
+1.25, and -0.75.

The -newerXY family supports read-only literal reference forms in addition to
file
references:

- @<unix-seconds>[.frac]
- YYYY-MM-DD
- YYYY-MM-DD[ T]HH:MM[:SS][.frac][Z|+-HH[:MM]]

Birth-time forms depend on the active platform backend. Where the platform does
not expose the required data, planning fails with an explicit diagnostic.

## Output actions

- -print
- -print0
- -printf
- -fprint
- -fprint0
- -fprintf
- -ls
- -fls

The current -printf and -fprintf subset supports GNU-style field formatting
with %[flags][width][.precision]directive.

Supported directives currently include:

```
%p %P %H %f %h %d %y %s %m %M %l %i %n %D %b %k %u %U %US %g %G %GS %F
%a %c %t %B %A* %C* %T* %B*
```

GNU special time selectors %A@, %C@, %T@, %B@, %A+, %C+, %T+, and %B+ are
supported. Unsupported directives fail during planning with explicit
diagnostics.

Escape handling also supports literal percent, backslash, newline, tab, and
NUL output forms.

## Exec, prompt, and mutation actions

- -exec ... ;
- -exec ... +
- -execdir ... ;
- -execdir ... +
- -ok ... ;
- -okdir ... ;
- -delete
- -quit

The -ok ... + and -okdir ... + forms are not supported.

The -execdir family checks *PATH* eagerly on non-Windows hosts and rejects
empty or
non-absolute entries as unsafe.

The -delete action implies depth-mode traversal, so directories are evaluated
after their
descendants.

# EXECUTION MODE

Use *RUSHFIND_WORKERS* to choose between ordered single-worker execution and
the relaxed-order parallel engine:

- RUSHFIND_WORKERS=1 keeps traversal and output closest to GNU ordering.
- Values greater than 1 enable the worker-owned relaxed-order parallel mode.
- If unset, *rfd* uses host parallelism clamped to the range 1..4.

In relaxed-order parallel mode:

- Side effects may be emitted out of sibling order.
- -fprint, -fprint0, and -fprintf writes are atomic per destination file.
- -ls stdout and -fls writes are atomic per rendered record.
- -quit is cancellation: no new subtree work is published after it is observed,
  but already granted work may still finish.
- Depth-mode actions preserve descendant-before-parent completion.

# ENVIRONMENT

- *RUSHFIND_WORKERS*
  Controls worker count and execution mode.

- *RUSHFIND_WARNINGS*
  Controls startup warnings. The default is on. Values 0, false, no, and off
  suppress them.

# EXIT STATUS

- 0 if planning and execution complete without runtime errors or action
  failures.
- 1 for parse failures, planning failures, runtime errors, or action failures.

# EXAMPLES

## Find Rust source files below src

```
rfd src -type f -name '*.rs'
```

## Skip target while printing regular files

```
rfd . -path './target/*' -prune -o -type f -print
```

## Use PCRE2 for whole-path matching

```
rfd . -regextype pcre2 -regex '.*/(src|tests)/.*\\.rs' -print
```

## Read search roots from standard input

```
printf '%s\\0' dir1 dir2 | rfd -files0-from - -maxdepth 1 -type f
```

## Keep output close to GNU ordering

```
RUSHFIND_WORKERS=1 rfd . -type f -print
```

# NOTES

Linux is the reference platform. macOS, the BSDs, a generic Unix tier, and
native Windows are also supported, but some features are approximate or
platform-specific.

Interactive locale handling for -ok and -okdir is approximate on some
non-Linux platforms and emits a startup warning when planned.

Case-insensitive glob matching may differ outside the C locale on some
non-Linux and Windows configurations and emits a startup warning when planned.

On some generic Unix targets, -fstype, %F, -flags, and birth-time predicates
fail during planning instead of running approximately.

# SEE ALSO

*find*(1)
