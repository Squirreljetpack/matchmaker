mod preview;
pub mod previewer;
pub mod utils;
pub mod io;

pub use preview::Preview;
pub use io::*;

use std::{
    env,
    process::{Child, Command, Stdio},
    sync::LazyLock,
};

// todo: support -i
pub fn spawn(
    cmd: &str,
    vars: impl IntoIterator<Item = (String, String)>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
) -> Option<Child> {
    let (shell, arg) = &*SHELL;

    Command::new(shell)
    .arg(arg)
    .arg(cmd)
    .envs(vars)
    .stdin(stdin)
    .stdout(stdout)
    .stderr(stderr)
    .spawn()
    .map_err(|e| log::error!("Failed to spawn command {cmd}: {e}"))
    .ok()
}

pub fn exec(cmd: &str, vars: impl IntoIterator<Item = (String, String)>) -> ! {
    let (shell, arg) = &*SHELL;

    let mut command = Command::new(shell);
    command.arg(arg).arg(cmd).envs(vars);

    #[cfg(not(windows))]
    {
        // replace current process

        use std::os::unix::process::CommandExt;
        let err = command.exec();
        use std::process::exit;

        eprintln!("Could not exec {cmd:?}: {err}");
        exit(1);
    }

    #[cfg(windows)]
    {
        match command.status() {
            Ok(status) => {
                exit(
                    status
                    .code()
                    .unwrap_or(if status.success() { 0 } else { 1 }),
                );
            }
            Err(err) => {
                eprintln!("Could not spawn {cmd:?}: {err}");
                exit(1);
            }
        }
    }
}

static SHELL: LazyLock<(String, String)> = LazyLock::new(|| {
    #[cfg(windows)]
    {
        let path = env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string());
        let flag = if path.to_lowercase().contains("powershell") {
            "-Command".to_string()
        } else {
            "/C".to_string()
        };
        (path, flag)
    }
    #[cfg(unix)]
    {
        let path = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
        let flag = "-c".to_string();
        log::debug!("SHELL: {}, {}", path, flag);
        (path, flag)
    }
});

pub type EnvVars = Vec<(String, String)>;

#[macro_export]
macro_rules! env_vars {
    ($( $name:expr => $value:expr ),* $(,)?) => {
        Vec::<(String, String)>::from([
            $( ($name.into(), $value.into()) ),*
            ]
        )
    };
}

// -------------- APPENDONLY
use std::ops::Deref;
use std::sync::{Arc, RwLock};

#[derive(Debug, Clone)]
pub struct AppendOnly<T>(Arc<RwLock<boxcar::Vec<T>>>);

impl<T> Default for AppendOnly<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> AppendOnly<T> {
    pub fn new() -> Self {
        Self(Arc::new(RwLock::new(boxcar::Vec::new())))
    }

    pub fn is_empty(&self) -> bool {
        let guard = self.0.read().unwrap();
        guard.is_empty()
    }

    pub fn len(&self) -> usize {
        let guard = self.0.read().unwrap();
        guard.count()
    }

    pub fn clear(&self) {
        let mut guard = self.0.write().unwrap(); // acquire write lock
        guard.clear();
    }

    pub fn push(&self, val: T) {
        let guard = self.0.read().unwrap();
        guard.push(val);
    }

    pub fn map_to_vec<U, F>(&self, mut f: F) -> Vec<U>
    where
    F: FnMut(&T) -> U,
    {
        let guard = self.0.read().unwrap();
        guard.iter().map(move |(_i, v)| f(v)).collect()
    }
}

impl<T> Deref for AppendOnly<T> {
    type Target = RwLock<boxcar::Vec<T>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
