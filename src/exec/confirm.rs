use crate::diagnostics::Diagnostic;
use crate::exec::PreparedExecCommand;
use crate::messages_locale::{MessagesLocale, PromptLocale};
use std::ffi::OsString;
use std::io::{BufRead, Stderr, Stdin, Write, stderr, stdin};
use std::sync::{Arc, Mutex};

#[cfg(test)]
use std::collections::VecDeque;

type AffirmativeParser = fn(&[u8]) -> bool;

unsafe extern "C" {
    fn rpmatch(response: *const libc::c_char) -> libc::c_int;
}

pub(crate) enum ConfirmOutcome<T> {
    Accepted(T),
    Rejected,
}

struct ProcessPromptSession {
    stdin: Stdin,
    stderr: Stderr,
}

impl ProcessPromptSession {
    fn open() -> Self {
        Self {
            stdin: stdin(),
            stderr: stderr(),
        }
    }

    fn write_prompt(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.stderr.write_all(bytes)
    }

    fn flush_prompt(&mut self) -> std::io::Result<()> {
        self.stderr.flush()
    }

    fn read_reply(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize> {
        self.stdin.lock().read_until(b'\n', out)
    }
}

#[cfg(test)]
struct ScriptedPromptSession {
    replies: VecDeque<Vec<u8>>,
}

#[cfg(test)]
impl ScriptedPromptSession {
    fn write_prompt(&mut self, _bytes: &[u8]) -> std::io::Result<()> {
        Ok(())
    }

    fn flush_prompt(&mut self) -> std::io::Result<()> {
        Ok(())
    }

    fn read_reply(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize> {
        match self.replies.pop_front() {
            Some(reply) => {
                out.extend_from_slice(&reply);
                Ok(reply.len())
            }
            None => Ok(0),
        }
    }
}

enum PromptSession {
    Process(ProcessPromptSession),
    #[cfg(test)]
    Scripted(ScriptedPromptSession),
}

impl PromptSession {
    fn write_prompt(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            PromptSession::Process(session) => session.write_prompt(bytes),
            #[cfg(test)]
            PromptSession::Scripted(session) => session.write_prompt(bytes),
        }
    }

    fn flush_prompt(&mut self) -> std::io::Result<()> {
        match self {
            PromptSession::Process(session) => session.flush_prompt(),
            #[cfg(test)]
            PromptSession::Scripted(session) => session.flush_prompt(),
        }
    }

    fn read_reply(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize> {
        match self {
            PromptSession::Process(session) => session.read_reply(out),
            #[cfg(test)]
            PromptSession::Scripted(session) => session.read_reply(out),
        }
    }
}

pub(crate) struct PromptCoordinator {
    inner: Arc<Mutex<PromptSession>>,
    locale: MessagesLocale,
    affirmative_parser: AffirmativeParser,
}

impl PromptCoordinator {
    pub(crate) fn open_process() -> Self {
        Self::open_process_with_locale(default_messages_locale())
    }

    pub(crate) fn open_process_with_locale(locale: MessagesLocale) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PromptSession::Process(
                ProcessPromptSession::open(),
            ))),
            locale,
            affirmative_parser: libc_rpmatch_is_affirmative,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(scripted_replies: Vec<Vec<u8>>) -> Self {
        Self::for_tests_with(
            scripted_replies,
            default_messages_locale(),
            ascii_c_locale_yes_is_affirmative,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_tests_with(
        scripted_replies: Vec<Vec<u8>>,
        locale: MessagesLocale,
        affirmative_parser: AffirmativeParser,
    ) -> Self {
        Self {
            inner: Arc::new(Mutex::new(PromptSession::Scripted(
                ScriptedPromptSession {
                    replies: scripted_replies.into_iter().collect(),
                },
            ))),
            locale,
            affirmative_parser,
        }
    }

    pub(crate) fn confirm_prepared<T, F>(
        &self,
        prompt_argv: &[OsString],
        prepared: &PreparedExecCommand,
        run_confirmed: F,
    ) -> Result<ConfirmOutcome<T>, Diagnostic>
    where
        F: FnOnce(&PreparedExecCommand) -> Result<T, Diagnostic>,
    {
        let mut session = self
            .inner
            .lock()
            .map_err(|_| Diagnostic::new("internal error: prompt session poisoned", 1))?;
        let prompt = render_prompt(prompt_argv, &self.locale);
        session.write_prompt(&prompt).map_err(io_error)?;
        session.flush_prompt().map_err(io_error)?;

        let mut reply = Vec::new();
        let read = session.read_reply(&mut reply).map_err(io_error)?;
        if read == 0 || !(self.affirmative_parser)(trim_reply_line(&reply)) {
            return Ok(ConfirmOutcome::Rejected);
        }

        Ok(ConfirmOutcome::Accepted(run_confirmed(prepared)?))
    }
}

fn trim_reply_line(bytes: &[u8]) -> &[u8] {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    bytes.strip_suffix(b"\r").unwrap_or(bytes)
}

#[cfg(test)]
fn ascii_c_locale_yes_is_affirmative(bytes: &[u8]) -> bool {
    bytes.eq_ignore_ascii_case(b"y") || bytes.eq_ignore_ascii_case(b"yes")
}

fn libc_rpmatch_is_affirmative(bytes: &[u8]) -> bool {
    if bytes.is_empty() || bytes.contains(&0) {
        return false;
    }

    let reply = match std::ffi::CString::new(bytes) {
        Ok(reply) => reply,
        Err(_) => return false,
    };

    unsafe { rpmatch(reply.as_ptr()) == 1 }
}

fn default_messages_locale() -> MessagesLocale {
    MessagesLocale {
        resolved_name: "C".into(),
        prompt_locale: PromptLocale::C,
    }
}

pub(crate) fn render_prompt(prompt_argv: &[OsString], locale: &MessagesLocale) -> Vec<u8> {
    let fragments = locale.prompt_fragments();
    let mut out = fragments.prefix.to_vec();
    if let Some(program) = prompt_argv.first() {
        out.extend_from_slice(program.as_encoded_bytes());
    }
    if prompt_argv.len() > 1 {
        out.extend_from_slice(fragments.ellipsis);
        if let Some(last) = prompt_argv.last() {
            out.push(b' ');
            out.extend_from_slice(last.as_encoded_bytes());
        }
    }
    out.extend_from_slice(fragments.suffix);
    out
}

fn io_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::new(format!("failed to use prompt session: {error}"), 1)
}

#[cfg(test)]
mod tests {
    use super::{ConfirmOutcome, PromptCoordinator, render_prompt};
    use crate::exec::PreparedExecCommand;
    use crate::messages_locale::{MessagesLocale, PromptLocale};
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn render_prompt_uses_catalog_fragments_from_messages_locale() {
        let locale = MessagesLocale {
            resolved_name: "fr_FR.UTF-8".into(),
            prompt_locale: PromptLocale::Fr,
        };

        let prompt = render_prompt(
            &[OsString::from("printf"), OsString::from("alpha.txt")],
            &locale,
        );

        assert_eq!(prompt, b"< printf ... alpha.txt > ? ".to_vec());
    }

    #[test]
    fn render_prompt_preserves_non_utf8_payload_bytes() {
        let locale = MessagesLocale {
            resolved_name: "C".into(),
            prompt_locale: PromptLocale::C,
        };
        let argv = vec![
            OsString::from_vec(b"printf".to_vec()),
            OsString::from_vec(b"bad-\xff-name".to_vec()),
        ];

        let prompt = render_prompt(&argv, &locale);
        assert!(
            prompt
                .windows(b"bad-\xff-name".len())
                .any(|window| window == b"bad-\xff-name")
        );
    }

    #[test]
    fn confirm_prepared_uses_injected_affirmative_parser() {
        fn accepts_oui(bytes: &[u8]) -> bool {
            bytes == b"oui"
        }

        let locale = MessagesLocale {
            resolved_name: "fr".into(),
            prompt_locale: PromptLocale::Fr,
        };
        let coordinator =
            PromptCoordinator::for_tests_with(vec![b"oui\n".to_vec()], locale, accepts_oui);
        let prepared = PreparedExecCommand {
            cwd: None,
            argv: vec!["echo".into(), "file.txt".into()],
        };

        let outcome = coordinator
            .confirm_prepared(
                &[OsString::from("echo"), OsString::from("file.txt")],
                &prepared,
                |_prepared| Ok(true),
            )
            .unwrap();

        assert!(matches!(outcome, ConfirmOutcome::Accepted(true)));
    }

    #[test]
    fn confirm_prepared_rejects_eof_without_running_the_child() {
        let coordinator = PromptCoordinator::for_tests(vec![Vec::new()]);
        let prepared = PreparedExecCommand {
            cwd: None,
            argv: vec!["echo".into(), "file.txt".into()],
        };
        let mut ran = false;

        let outcome = coordinator
            .confirm_prepared(
                &[OsString::from("echo"), OsString::from("file.txt")],
                &prepared,
                |_prepared| {
                    ran = true;
                    Ok(true)
                },
            )
            .unwrap();

        assert!(matches!(outcome, ConfirmOutcome::Rejected));
        assert!(!ran);
    }

    #[test]
    fn confirmed_closure_runs_while_the_session_lock_is_still_held() {
        let coordinator = Arc::new(PromptCoordinator::for_tests(vec![
            b"yes\n".to_vec(),
            b"yes\n".to_vec(),
        ]));
        let prepared = PreparedExecCommand {
            cwd: Some(PathBuf::from("dir")),
            argv: vec!["echo".into(), "./file.txt".into()],
        };
        let log = Arc::new(Mutex::new(Vec::new()));

        let left = {
            let coordinator = coordinator.clone();
            let prepared = prepared.clone();
            let log = log.clone();
            thread::spawn(move || {
                coordinator
                    .confirm_prepared(
                        &[OsString::from("echo"), OsString::from("./file.txt")],
                        &prepared,
                        |_prepared| {
                            log.lock().unwrap().push("left:start");
                            thread::sleep(Duration::from_millis(50));
                            log.lock().unwrap().push("left:end");
                            Ok(true)
                        },
                    )
                    .unwrap();
            })
        };

        let right = {
            let coordinator = coordinator.clone();
            let prepared = prepared.clone();
            let log = log.clone();
            thread::spawn(move || {
                coordinator
                    .confirm_prepared(
                        &[OsString::from("echo"), OsString::from("./file.txt")],
                        &prepared,
                        |_prepared| {
                            log.lock().unwrap().push("right:start");
                            log.lock().unwrap().push("right:end");
                            Ok(true)
                        },
                    )
                    .unwrap();
            })
        };

        left.join().unwrap();
        right.join().unwrap();

        let log = log.lock().unwrap().clone();
        assert!(
            log == vec!["left:start", "left:end", "right:start", "right:end"]
                || log == vec!["right:start", "right:end", "left:start", "left:end"]
        );
    }

    #[test]
    fn open_process_constructs_a_process_backed_prompt_session() {
        let _coordinator = PromptCoordinator::open_process();
    }
}
