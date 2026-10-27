//! Windows transport for pinned mpv's JSON IPC.
//!
//! mpv on Windows serves `--input-ipc-server` as a named pipe and has no
//! `fd://` client, so the core connects to the pipe as a client. The mpv
//! worker is a single thread that writes a command and then polls for lines,
//! so one synchronous handle is enough: reads only take bytes the pipe already
//! holds (`PeekNamedPipe`), which keeps a read from blocking the next write and
//! gives the same timed-out poll a Unix socket read timeout gives.
//!
//! The lifetime link Unix gets from mpv's stdin socket comes from a Job Object
//! that kills its processes when the core's last handle to it closes, which
//! includes the core being killed.

use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::process::Child;
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{ERROR_BROKEN_PIPE, ERROR_NO_DATA, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows::Win32::System::Pipes::PeekNamedPipe;
use windows::core::PCWSTR;

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_PIPE_BUSY: i32 = 231;
const READ_POLL_INTERVAL: Duration = Duration::from_millis(5);
const CONNECT_RETRY_INTERVAL: Duration = Duration::from_millis(10);

/// `CREATE_NO_WINDOW`: mpv and its `--version` probe never open a console.
pub(crate) const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// The pipe name mpv serves for one private IPC endpoint. The random
/// endpoint directory's name keeps it unguessable, like the Unix socket path.
pub(crate) fn pipe_name(ipc_directory: &Path) -> String {
    let name = ipc_directory
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "earthly-audio-mpv".to_owned());
    format!(r"\\.\pipe\{name}")
}

/// The write half of the mpv connection. It owns the pipe handle; the reader
/// is a clone of it.
pub(crate) struct MpvPipe {
    file: File,
}

impl MpvPipe {
    /// Connects to mpv's pipe, retrying while mpv has not created it yet or
    /// every instance is busy. Fails once `has_exited` reports mpv has exited
    /// or `deadline` passes.
    pub(crate) fn connect(
        name: &str,
        deadline: Instant,
        mut has_exited: impl FnMut() -> Result<bool, String>,
    ) -> Result<Self, String> {
        loop {
            match OpenOptions::new().read(true).write(true).open(name) {
                Ok(file) => return Ok(Self { file }),
                Err(error)
                    if matches!(
                        error.raw_os_error(),
                        Some(ERROR_FILE_NOT_FOUND | ERROR_PIPE_BUSY)
                    ) => {}
                Err(error) => {
                    return Err(format!(
                        "Pinned mpv private IPC could not be opened: {error}"
                    ));
                }
            }
            if has_exited()? {
                return Err("Pinned mpv exited before opening private IPC.".to_owned());
            }
            if Instant::now() >= deadline {
                return Err(
                    "Pinned mpv did not open private IPC before startup timeout.".to_owned(),
                );
            }
            thread::sleep(CONNECT_RETRY_INTERVAL);
        }
    }

    pub(crate) fn reader(&self, poll_interval: Duration) -> io::Result<MpvPipeReader> {
        Ok(MpvPipeReader {
            file: self.file.try_clone()?,
            pending: Vec::new(),
            poll_interval,
        })
    }
}

impl Write for MpvPipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

/// Reads mpv's newline-delimited messages. `read_line` behaves like
/// `BufRead::read_line` on a socket with a read timeout: it returns
/// `TimedOut` when no complete line arrived within the poll interval and
/// `Ok(0)` once mpv closed the pipe.
pub(crate) struct MpvPipeReader {
    file: File,
    pending: Vec<u8>,
    poll_interval: Duration,
}

impl MpvPipeReader {
    pub(crate) fn read_line(&mut self, line: &mut String) -> io::Result<usize> {
        let deadline = Instant::now() + self.poll_interval;
        loop {
            if let Some(end) = self.pending.iter().position(|byte| *byte == b'\n') {
                let message: Vec<u8> = self.pending.drain(..=end).collect();
                let text = String::from_utf8(message)
                    .map_err(|error| io::Error::new(ErrorKind::InvalidData, error))?;
                line.push_str(&text);
                return Ok(text.len());
            }
            match self.available()? {
                None => return Ok(0),
                Some(0) => {}
                Some(available) => {
                    let start = self.pending.len();
                    self.pending.resize(start + available, 0);
                    let read = self.file.read(&mut self.pending[start..])?;
                    self.pending.truncate(start + read);
                    if read == 0 {
                        return Ok(0);
                    }
                    continue;
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    ErrorKind::TimedOut,
                    "no mpv IPC message arrived",
                ));
            }
            thread::sleep(READ_POLL_INTERVAL);
        }
    }

    /// Bytes waiting in the pipe, or `None` once mpv closed it.
    fn available(&self) -> io::Result<Option<usize>> {
        let mut available = 0_u32;
        // SAFETY: the handle belongs to `self.file`; only the byte count is
        // written, into a local.
        let result = unsafe {
            PeekNamedPipe(
                HANDLE(self.file.as_raw_handle()),
                None,
                0,
                None,
                Some(&mut available),
                None,
            )
        };
        match result {
            Ok(()) => Ok(Some(available as usize)),
            Err(error)
                if error.code() == ERROR_BROKEN_PIPE.to_hresult()
                    || error.code() == ERROR_NO_DATA.to_hresult() =>
            {
                Ok(None)
            }
            Err(error) => Err(io::Error::other(error.to_string())),
        }
    }
}

/// Places mpv in the core's kill-on-close Job Object, so mpv ends with the
/// core even when the core is killed without running its teardown.
pub(crate) fn bind_lifetime_to_core(child: &Child) -> Result<(), String> {
    let job = core_job()?;
    // SAFETY: both handles are valid: the job lives for the whole process and
    // the child handle belongs to `child`.
    unsafe { AssignProcessToJobObject(HANDLE(job as _), HANDLE(child.as_raw_handle())) }
        .map_err(|error| format!("Pinned mpv lifetime link could not be created: {error}"))
}

/// The process-wide Job Object. Its handle is never closed, so the system
/// closes it, and kills mpv, exactly when the core process ends.
fn core_job() -> Result<usize, String> {
    static JOB: OnceLock<Result<usize, String>> = OnceLock::new();
    JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls; the limit structure outlives the call.
        unsafe {
            let job = CreateJobObjectW(None, PCWSTR::null())
                .map_err(|error| format!("Pinned mpv Job Object could not be created: {error}"))?;
            let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|error| format!("Pinned mpv Job Object could not be configured: {error}"))?;
            Ok(job.0 as usize)
        }
    })
    .clone()
}
