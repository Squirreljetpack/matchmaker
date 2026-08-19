use std::process::ExitStatus;

/// Outcome of an executed command, normalized across shell children and lua
/// scripts for the shared exit → quit/prompt policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExitDetails {
    /// Exit code when the command ran to completion.
    pub code: Option<i32>,
    /// Termination state:
    /// - `Some(true)`: User interruption (SIGINT/SIGQUIT/SIGTERM, Ctrl+C, or explicit exit code 100).
    /// - `Some(false)`: Abnormal exit / crash (stopped signal, system error, negative code).
    /// - `None`: Normal completion.
    pub interrupted: Option<bool>,
}

impl ExitDetails {
    pub fn of(status: ExitStatus) -> Self {
        #[cfg(unix)]
        let (interrupted, abnormal) = {
            use std::os::unix::process::ExitStatusExt;
            let is_int = status.signal().is_some_and(|x| [2, 3, 15].contains(&x))
                || status.code() == Some(100);
            let is_abn = status.stopped_signal().is_some();
            (is_int, is_abn)
        };

        #[cfg(windows)]
        let (interrupted, abnormal) = {
            let is_int = status.code().is_some_and(|x| x == -1073741510 || x == 100); // 0xC000013A (Ctrl+C) or 100
            let is_abn = status.code().is_some_and(|x| x < 0 && x != -1073741510);
            (is_int, is_abn)
        };

        #[cfg(not(any(unix, windows)))]
        let (interrupted, abnormal) =
            (status.code() == Some(100) || status.code().is_none(), false);

        let interrupted = if interrupted {
            Some(true)
        } else if abnormal {
            Some(false)
        } else {
            None
        };

        Self {
            code: status.code(),
            interrupted,
        }
    }

    #[cfg(feature = "mlua")]
    /// A lua script that ran to completion with `code`.
    pub fn code(code: i32) -> Self {
        let interrupted = if code == 100 { Some(true) } else { None };
        Self {
            code: Some(code),
            interrupted,
        }
    }

    #[cfg(feature = "mlua")]
    /// A script error: a failure without a code.
    pub fn error() -> Self {
        Self {
            code: Some(1),
            interrupted: None,
        }
    }

    /// Whether the execution was successful (exited with code 0).
    pub fn success(&self) -> bool {
        self.code == Some(0) && self.interrupted.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_details_classification() {
        let details_success = ExitDetails {
            code: Some(0),
            interrupted: None,
        };
        assert!(details_success.success());

        let details_fail = ExitDetails {
            code: Some(1),
            interrupted: None,
        };
        assert!(!details_fail.success());

        let details_interrupted = ExitDetails {
            code: Some(100),
            interrupted: Some(true),
        };
        assert!(!details_interrupted.success());

        #[cfg(feature = "mlua")]
        {
            let normal_zero = ExitDetails::code(0);
            assert_eq!(normal_zero.code, Some(0));
            assert_eq!(normal_zero.interrupted, None);
            assert!(normal_zero.success());

            let normal_one = ExitDetails::code(1);
            assert_eq!(normal_one.code, Some(1));
            assert_eq!(normal_one.interrupted, None);
            assert!(!normal_one.success());

            let int_100 = ExitDetails::code(100);
            assert_eq!(int_100.code, Some(100));
            assert_eq!(int_100.interrupted, Some(true));
            assert!(!int_100.success());

            let err = ExitDetails::error();
            assert_eq!(err.code, Some(1));
            assert_eq!(err.interrupted, None);
            assert!(!err.success());
        }
    }
}
