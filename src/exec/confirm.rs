use crate::diagnostics::Diagnostic;
use crate::exec::PreparedExecCommand;
use std::ffi::OsString;
use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::sync::{Arc, Mutex};

pub(crate) enum ConfirmOutcome<T> {
    Accepted(T),
    Rejected,
}

trait PromptChannel: Send {
    fn write_prompt(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    fn flush_prompt(&mut self) -> std::io::Result<()>;
    fn read_reply(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize>;
}

struct ProcessPromptChannel {
    reader: Box<dyn BufRead + Send>,
    writer: Box<dyn Write + Send>,
}

impl ProcessPromptChannel {
    fn open() -> Result<Self, Diagnostic> {
        if let Ok(tty) = OpenOptions::new().read(true).write(true).open("/dev/tty") {
            let reader = tty.try_clone().map_err(io_error)?;
            return Ok(Self {
                reader: Box::new(BufReader::new(reader)),
                writer: Box::new(tty),
            });
        }

        let stdin = OpenOptions::new()
            .read(true)
            .open("/dev/stdin")
            .map_err(io_error)?;
        let stderr = OpenOptions::new()
            .write(true)
            .open("/dev/stderr")
            .map_err(io_error)?;
        Ok(Self {
            reader: Box::new(BufReader::new(stdin)),
            writer: Box::new(stderr),
        })
    }
}

impl PromptChannel for ProcessPromptChannel {
    fn write_prompt(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(bytes)
    }

    fn flush_prompt(&mut self) -> std::io::Result<()> {
        self.writer.flush()
    }

    fn read_reply(&mut self, out: &mut Vec<u8>) -> std::io::Result<usize> {
        self.reader.read_until(b'\n', out)
    }
}

enum PromptState {
    ProcessUnopened,
    Ready(Box<dyn PromptChannel>),
}

impl PromptState {
    fn channel_mut(&mut self) -> Result<&mut dyn PromptChannel, Diagnostic> {
        if matches!(self, PromptState::ProcessUnopened) {
            *self = PromptState::Ready(Box::new(ProcessPromptChannel::open()?));
        }

        match self {
            PromptState::Ready(channel) => Ok(channel.as_mut()),
            PromptState::ProcessUnopened => unreachable!("prompt channel initialized above"),
        }
    }
}

pub(crate) struct PromptCoordinator {
    inner: Arc<Mutex<PromptState>>,
}

impl PromptCoordinator {
    pub(crate) fn open_process() -> Self {
        Self {
            inner: Arc::new(Mutex::new(PromptState::ProcessUnopened)),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_tests(scripted_replies: Vec<Vec<u8>>) -> Self {
        use std::collections::VecDeque;

        struct ScriptedPromptChannel {
            replies: VecDeque<Vec<u8>>,
        }

        impl PromptChannel for ScriptedPromptChannel {
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

        Self {
            inner: Arc::new(Mutex::new(PromptState::Ready(Box::new(
                ScriptedPromptChannel {
                    replies: scripted_replies.into_iter().collect(),
                },
            )))),
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
        let mut state = self
            .inner
            .lock()
            .map_err(|_| Diagnostic::new("internal error: prompt channel poisoned", 1))?;
        let channel = state.channel_mut()?;
        let prompt = render_prompt(prompt_argv);
        channel.write_prompt(&prompt).map_err(io_error)?;
        channel.flush_prompt().map_err(io_error)?;

        let mut reply = Vec::new();
        let read = channel.read_reply(&mut reply).map_err(io_error)?;
        if read == 0 || !parse_affirmative_reply(&reply) {
            return Ok(ConfirmOutcome::Rejected);
        }

        Ok(ConfirmOutcome::Accepted(run_confirmed(prepared)?))
    }
}

pub(crate) fn parse_affirmative_reply(bytes: &[u8]) -> bool {
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    bytes.eq_ignore_ascii_case(b"y") || bytes.eq_ignore_ascii_case(b"yes")
}

pub(crate) fn render_prompt(prompt_argv: &[OsString]) -> Vec<u8> {
    let mut out = b"< ".to_vec();
    for (index, arg) in prompt_argv.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        out.extend_from_slice(arg.as_encoded_bytes());
    }
    out.extend_from_slice(b" > ? ");
    out
}

fn io_error(error: std::io::Error) -> Diagnostic {
    Diagnostic::new(format!("failed to use prompt channel: {error}"), 1)
}

#[cfg(test)]
mod tests {
    use super::{ConfirmOutcome, PromptCoordinator, parse_affirmative_reply};
    use crate::exec::PreparedExecCommand;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn parse_affirmative_reply_accepts_c_locale_ascii_yes_variants() {
        for reply in [b"y\n".as_slice(), b"Y\n", b"yes\n", b"YeS\r\n"] {
            assert!(parse_affirmative_reply(reply));
        }
        for reply in [b"\n".as_slice(), b"no\n", b"oui\n", b"1\n"] {
            assert!(!parse_affirmative_reply(reply));
        }
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
    fn open_process_is_lazy_until_the_first_confirmation() {
        let _coordinator = PromptCoordinator::open_process();
    }
}
