use std::process::{Command, Stdio};

use cba::{broc::CommandExt, env_vars};
use log::info;
use matchmaker::{
    AttachmentFormatter, Matchmaker, SSS, Selection, message::Interrupt, use_formatter,
};

#[easy_ext::ext(MMExt)]
impl<T: SSS, S: Selection + 'static> Matchmaker<T, S> {
    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn register_execute_handler(&mut self, formatter: AttachmentFormatter<T, S>) {
        let _formatter = formatter.clone();
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let keep_alive = state.discriminant_payload.take() == Some(0);
            let template = state.payload();

            if !template.is_empty() {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty())
                    ._spawn()
                {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}");
                            // signal termination don't prompt
                            if keep_alive && i.code().is_some_and(|c| c != 0) {
                                println!("\nPress enter to continue...");
                                let mut input = String::new();
                                let _ = std::io::stdin().read_line(&mut input);
                            }
                        }
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
        });
        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&_formatter, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&_formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                if let Some(mut _child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty())
                    ._spawn()
                {
                    // match child.wait() {
                    //     Ok(i) => {
                    //         info!("Command [{cmd}] exited with {i}")
                    //     }
                    //     Err(e) => {
                    //         info!("Failed to wait on command [{cmd}]: {e}")
                    //     }
                    // }
                }
            };
        });
    }
}

fn maybe_tty() -> Stdio {
    if let Ok(tty) = std::fs::File::open("/dev/tty") {
        // let _ = std::io::Write::flush(&mut tty); // does nothing but seems logical
        Stdio::from(tty)
    } else {
        log::error!("Failed to open /dev/tty");
        Stdio::inherit()
    }
}
