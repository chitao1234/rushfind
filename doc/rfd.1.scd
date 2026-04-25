rfd(1)

# NAME

rfd - find files with GNU-style expressions and parallel traversal

# SYNOPSIS

*rfd* [*-H*|*-L*|*-P*] [*-O*_LEVEL_] [*-D* _LIST_] [_PATH_...] [_EXPRESSION_]

# DESCRIPTION

*rfd* walks one or more start paths and evaluates an expression for each entry.
The default start path is the current directory. The default expression is
*-print*.

The command line is intentionally close to GNU-style *find* syntax, but this
manual describes the behavior implemented by *rfd*. The binary name is *rfd*;
the Rust package and project are named rushfind.

By default, *rfd* uses physical traversal: command-line symbolic links and
symbolic links found during traversal are not followed. Use *-H*, *-L*, or *-P*
before start paths to select the global follow mode.

# OPTIONS

*-H*
	Follow symbolic links listed as command-line start paths. Symbolic links
	found during traversal are not followed.

*-L*
	Follow symbolic links while traversing. Directory link loops are detected
	and reported instead of recursing forever.

*-P*
	Use physical traversal. This is the default.

*-O*_LEVEL_
	Accept a GNU-style optimizer level for command-line compatibility. The
	current implementation records the level but does not change optimizer
	behavior.

*-D* _LIST_
	Enable lightweight debug diagnostics for comma-separated names. Valid
	names are *exec*, *opt*, *rates*, *search*, *stat*, *time*, *tree*, *all*,
	and *help*. Use *-D help* to print the debug help text.

*--help*
	Print a compact quick reference and exit before traversal starts.

*--version*, *-version*
	Print build version metadata and exit before traversal starts.

# EXPRESSIONS

Expressions are made from tests, actions, operators, and traversal controls.
If no expression is supplied, *rfd* behaves as if *-print* was supplied.

Adjacent primaries are joined by an implicit logical AND. Actions generally
return true when they succeed, allowing later expression terms to continue.

# TESTS

*-name* _PATTERN_, *-iname* _PATTERN_
	Match the entry basename with a shell-style pattern. The *i* form is
	case-insensitive.

*-path* _PATTERN_, *-ipath* _PATTERN_
	Match the full displayed path with a shell-style pattern. The *i* form is
	case-insensitive.

*-lname* _PATTERN_, *-ilname* _PATTERN_
	Match the contents of a symbolic link. The *i* form is case-insensitive.

*-regex* _PATTERN_, *-iregex* _PATTERN_
	Match paths with the active regular-expression backend. The *i* form is
	case-insensitive.

*-regextype* _TYPE_
	Select the regular-expression syntax for following regex predicates.

*-type* _LIST_, *-xtype* _LIST_
	Match file type letters. Supported letters include *b*, *c*, *d*, *f*,
	*l*, *p*, and *s*. Comma lists such as *-type f,d* are accepted. The
	Solaris door type letter *D* is recognized for compatibility but reports
	an unsupported diagnostic in this build.

*-empty*
	Match empty regular files or empty directories.

*-size* _N_[*bckMGw*]
	Match by file size using GNU-style numeric prefixes. A leading *+* means
	greater than, a leading *-* means less than, and no prefix means exactly
	the rounded unit count.

*-mtime* _N_, *-atime* _N_, *-ctime* _N_
	Match modification, access, or status-change time in days.

*-mmin* _N_, *-amin* _N_, *-cmin* _N_
	Match modification, access, or status-change time in minutes.

*-newer* _FILE_, *-anewer* _FILE_, *-cnewer* _FILE_, *-newer*_XY_ _REFERENCE_
	Compare timestamps with a reference file or supported literal timestamp.

*-used* _N_
	Match by days between last access and status change.

*-uid* _N_, *-gid* _N_, *-user* _NAME_, *-group* _NAME_
	Match numeric or named ownership where supported by the active platform.

*-nouser*, *-nogroup*
	Match entries whose owner or group cannot be resolved.

*-perm* _MODE_
	Match Unix-style permission modes where supported.

*-readable*, *-writable*, *-executable*
	Match using platform access checks. These are not simple permission-bit
	tests.

*-samefile* _FILE_, *-inum* _N_, *-links* _N_
	Match identity and link-count metadata.

*-fstype* _TYPE_
	Match entries by filesystem type using the active platform mount snapshot.

*-true*, *-false*
	Always return true or false.

*-context* _PATTERN_
	Recognized for SELinux compatibility, but SELinux context matching is not
	supported by this build.

# ACTIONS

*-print*
	Print the current path followed by a newline.

*-print0*
	Print the current path followed by a NUL byte.

*-printf* _FORMAT_
	Print formatted metadata. Supported directives include path directives
	(*%p*, *%P*, *%H*, *%f*, *%h*), depth/type/size/mode/link directives,
	owner/group directives, filesystem directives, and time directives.

*-fprint* _FILE_, *-fprint0* _FILE_, *-fprintf* _FILE_ _FORMAT_
	Write print-style or printf-style output to a destination file. Destination
	files are created or truncated during startup.

*-ls*, *-fls* _FILE_
	Render an ls-style record to stdout or a destination file.

*-exec* _COMMAND_ *;*, *-exec* _COMMAND_ *+*
	Run a command. The *+* form batches path arguments.

*-execdir* _COMMAND_ *;*, *-execdir* _COMMAND_ *+*
	Run a command from the matched entry's parent directory.

*-ok* _COMMAND_ *;*, *-okdir* _COMMAND_ *;*
	Prompt before running a command. The batched *+* forms are not supported.

*-delete*
	Delete matched entries. This implies depth-mode traversal.

*-quit*
	Stop evaluation early. In relaxed parallel mode, already granted work may
	still complete.

# OPERATORS

*( *_EXPRESSION_* )*
	Group an expression.

*!* _EXPRESSION_, *-not* _EXPRESSION_
	Logical NOT.

_LEFT_ *-a* _RIGHT_, _LEFT_ *-and* _RIGHT_
	Logical AND. Adjacent primaries also imply AND.

_LEFT_ *-o* _RIGHT_, _LEFT_ *-or* _RIGHT_
	Logical OR.

_LEFT_ *,* _RIGHT_
	Evaluate expressions in sequence.

# TRAVERSAL CONTROLS

*-maxdepth* _N_
	Descend at most _N_ levels below each start path.

*-mindepth* _N_
	Do not apply the expression before depth _N_.

*-depth*
	Evaluate directories after their descendants.

*-prune*
	Return true and prevent traversal into the current directory in pre-order
	mode. In depth mode, *-prune* remains true but does not prevent descendants.

*-xdev*, *-mount*
	Do not descend into directories on other filesystems.

*-follow*
	Accepted as a positional compatibility control and enables logical
	symlink following for traversal.

# COMPATIBILITY OPTIONS

*-files0-from* _FILE_
	Read NUL-terminated start paths from _FILE_. Use *-* to read from stdin.

*-noleaf*
	Accepted for compatibility and recorded; it does not change traversal.

*-warn*, *-nowarn*
	Accepted for compatibility and recorded as warning-mode preferences.

*-ignore_readdir_race*, *-noignore_readdir_race*
	Accepted for compatibility and recorded. The current implementation does
	not yet alter runtime race handling for disappearing entries.

# LOCALE AND ENCODINGS

*LC_CTYPE and encodings*
	*rfd* resolves *LC_CTYPE* from *LC_ALL*, *LC_CTYPE*, then *LANG*.
	The *C* and *POSIX* locales keep byte-oriented matching. UTF-8 and
	supported legacy encodings use owned decoding tables for glob and GNU
	regex predicates. Collation ordering, equivalence classes, collating
	symbols, and locale-specific tailoring are not implemented. Case
	insensitive matching uses single-character folding only.

*LC_MESSAGES and prompts*
	*rfd* may use *LC_MESSAGES* to select localized *-ok* and *-okdir*
	prompt fragments. Prompt replies are parsed by a built-in ASCII
	affirmative parser; *LC_CTYPE* does not change confirmation parsing.

# PRINTF FORMAT

The *-printf* and *-fprintf* actions support GNU-style field formatting:
*%*[flags][width][*.precision*]_DIRECTIVE_. Common directives include *%p*
for the full displayed path, *%f* for the basename, *%h* for the dirname,
*%y* for file type, *%s* for size, *%m* and *%M* for modes, *%u*/*%g* for
owner and group names, *%U*/*%G* for numeric ownership, and *%a*/*%c*/*%t*
for timestamps.

Escapes include *\\n*, *\\t*, *\\0*, *\\\\*, and *%%*. Unsupported directives
fail during planning with an explicit diagnostic.

# PARALLEL EXECUTION

Set *RUSHFIND_WORKERS=1* for ordered single-worker traversal. Values greater
than one enable relaxed-order parallel traversal.

Parallel mode may emit output and side effects in an order different from GNU
style sibling traversal. It preserves documented structural constraints such
as prune boundaries and depth-mode descendant-before-parent completion.

Lightweight debug diagnostics from *-D* describe *rfd* internals; they are not
a full GNU findutils trace stream.

# EXIT STATUS

*0*
	The command completed successfully.

*1*
	A parse, planning, traversal, action, or output error occurred.

# EXAMPLES

Find Rust files below *src*:

```
rfd src -name '*.rs' -print
```

Print NUL-terminated large regular-file paths:

```
rfd . -type f -size +1M -print0
```

Skip target directories while printing regular files:

```
rfd . -name target -prune -o -type f -print
```

Read start paths from a NUL-delimited file:

```
rfd -files0-from roots.list -type f -print
```

Run a command for each matching file:

```
rfd . -type f -exec sh -c 'printf "%s\\n" "$1"' sh {} ;
```

Print selected metadata:

```
rfd . -type f -printf '%p %s\\n'
```

# CAVEATS

Some recognized compatibility features are intentionally unsupported in this
build and fail before traversal starts. These include SELinux *-context*
matching and Solaris door matching through *-type D* or *-xtype D*.

On non-Linux and Windows platforms, some metadata predicates use platform
backends and may differ where the host exposes different filesystem or account
semantics.

# SEE ALSO

*rfd --help*, *rfd -D help*
