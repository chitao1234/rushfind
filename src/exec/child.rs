use crate::diagnostics::Diagnostic;
use crate::output::BrokerMessage;
use crossbeam_channel::Sender;
use std::io::{self, Read, Seek, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use super::batch::ReadyBatch;
use super::template::{
    ImmediateExecAction, PreparedExecCommand, build_batched_argv, build_immediate_command,
};

pub(super) fn run_ready_batch<E: Write>(
    ready: &ReadyBatch,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    let prepared = build_batched_argv(&ready.spec, &ready.paths)?;
    run_prepared_inherited(&prepared, stderr)
}

pub(super) fn run_immediate_ordered<E: Write>(
    spec: &ImmediateExecAction,
    path: &Path,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    let prepared = build_immediate_command(spec, path);
    run_prepared_inherited(&prepared, stderr)
}

pub(crate) fn run_prepared_inherited<E: Write>(
    command_spec: &PreparedExecCommand,
    stderr: &mut E,
) -> Result<bool, Diagnostic> {
    let Some(program) = command_spec.argv.first() else {
        return Err(Diagnostic::new(
            "internal error: exec action missing command",
            1,
        ));
    };

    let mut command = Command::new(program);
    command.args(&command_spec.argv[1..]);
    if let Some(cwd) = command_spec.cwd.as_ref() {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    match command.status() {
        Ok(status) => Ok(status.success()),
        Err(error) => {
            writeln!(stderr, "findoxide: {}: {error}", program.to_string_lossy()).map_err(
                |io_error| Diagnostic::new(format!("failed to write stderr: {io_error}"), 1),
            )?;
            Ok(false)
        }
    }
}

pub(crate) fn run_immediate_parallel(
    spec: &ImmediateExecAction,
    path: &Path,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    run_parallel_command(build_immediate_command(spec, path), broker, spill_threshold)
}

pub(crate) fn run_parallel_ready_batch(
    ready: &ReadyBatch,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    run_parallel_command(
        build_batched_argv(&ready.spec, &ready.paths)?,
        broker,
        spill_threshold,
    )
}

fn run_parallel_command(
    command_spec: PreparedExecCommand,
    broker: &Sender<BrokerMessage>,
    spill_threshold: usize,
) -> Result<bool, Diagnostic> {
    let Some(program) = command_spec.argv.first() else {
        return Err(Diagnostic::new(
            "internal error: exec action missing command",
            1,
        ));
    };

    let mut command = Command::new(program);
    command.args(&command_spec.argv[1..]);
    if let Some(cwd) = command_spec.cwd.as_ref() {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::null());
    command.stdout(Stdio::piped());
    command.stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            send_broker_message(
                broker,
                BrokerMessage::Stderr(
                    format!("findoxide: {}: {error}\n", program.to_string_lossy()).into_bytes(),
                ),
            )?;
            return Ok(false);
        }
    };

    let stdout = child
        .stdout
        .take()
        .expect("stdout is piped for parallel exec children");
    let stderr = child
        .stderr
        .take()
        .expect("stderr is piped for parallel exec children");

    let stdout_thread = std::thread::spawn(move || read_child_pipe(stdout, spill_threshold));
    let stderr_thread = std::thread::spawn(move || read_child_pipe(stderr, spill_threshold));
    let status = child
        .wait()
        .map_err(|error| Diagnostic::new(format!("failed to wait for exec child: {error}"), 1))?;

    let stdout = join_child_reader(stdout_thread)?;
    let stderr = join_child_reader(stderr_thread)?;

    if !stdout.is_empty() {
        send_broker_message(broker, BrokerMessage::Stdout(stdout))?;
    }
    if !stderr.is_empty() {
        send_broker_message(broker, BrokerMessage::Stderr(stderr))?;
    }

    Ok(status.success())
}

fn read_child_pipe<R: Read>(mut reader: R, threshold: usize) -> io::Result<Vec<u8>> {
    let mut buffer = SpillBuffer::new(threshold)?;
    let mut chunk = [0_u8; 8192];

    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        buffer.write_all(&chunk[..read])?;
    }

    buffer.into_bytes()
}

fn join_child_reader(
    handle: std::thread::JoinHandle<io::Result<Vec<u8>>>,
) -> Result<Vec<u8>, Diagnostic> {
    handle
        .join()
        .map_err(|_| Diagnostic::new("internal error: exec output reader thread panicked", 1))?
        .map_err(|error| Diagnostic::new(format!("failed to read exec child output: {error}"), 1))
}

pub(crate) fn send_broker_message(
    broker: &Sender<BrokerMessage>,
    message: BrokerMessage,
) -> Result<(), Diagnostic> {
    broker
        .send(message)
        .map_err(|_| Diagnostic::new("internal error: output broker is unavailable", 1))
}

pub struct SpillBuffer {
    threshold: usize,
    memory: Vec<u8>,
    spill: Option<tempfile::NamedTempFile>,
}

impl SpillBuffer {
    pub fn new(threshold: usize) -> io::Result<Self> {
        Ok(Self {
            threshold,
            memory: Vec::new(),
            spill: None,
        })
    }

    pub fn spilled_path(&self) -> Option<&Path> {
        self.spill.as_ref().map(tempfile::NamedTempFile::path)
    }

    pub fn into_bytes(mut self) -> io::Result<Vec<u8>> {
        if let Some(mut spill) = self.spill.take() {
            let mut bytes = Vec::new();
            spill.rewind()?;
            spill.read_to_end(&mut bytes)?;
            return Ok(bytes);
        }

        Ok(self.memory)
    }
}

impl Write for SpillBuffer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_all(bytes)?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Some(spill) = self.spill.as_mut() {
            spill.flush()?;
        }
        Ok(())
    }

    fn write_all(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.spill.is_none() && self.memory.len() + bytes.len() <= self.threshold {
            self.memory.extend_from_slice(bytes);
            return Ok(());
        }

        if self.spill.is_none() {
            let mut file = tempfile::NamedTempFile::new()?;
            file.write_all(&self.memory)?;
            self.memory.clear();
            self.spill = Some(file);
        }

        self.spill
            .as_mut()
            .expect("spill exists after initialization")
            .write_all(bytes)
    }
}
