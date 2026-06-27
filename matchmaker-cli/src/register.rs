use std::{process::Command};

use cba::{
    broc::{CommandExt, tty_or_inherit},
    env_vars,
};
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
        let formatter_ = formatter.clone();
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let discriminant = state.discriminant_payload.take();
            let template = state.payload();
            
            if !template.is_empty() {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();
                
                let preview_template = if let Some(Ok(s)) = state.preview_set_payload() {
                    s
                } else {
                    state.preview_payload().clone()
                };
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                
                if let Some(mut child) = Command::from_script(&cmd)
                .envs(vars)
                .stdin(tty_or_inherit())
                ._spawn()
                {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}");
                            match discriminant {
                                // signal termination don't prompt
                                Some(0) if i.code().is_some_and(|c| c != 0) => {
                                    println!("\nPress enter to continue...");
                                    let mut input = String::new();
                                    let _ = std::io::stdin().read_line(&mut input);
                                }
                                Some(1) if i.success() => {
                                    state.should_quit = true;
                                }
                                Some(2) => {
                                    #[cfg(unix)]
                                    let interrupted = {
                                        use std::os::unix::process::ExitStatusExt;
                                        i.signal().is_some_and(|x| [2, 3, 15].contains(&x))
                                    };

                                    #[cfg(windows)]
                                    let interrupted = i.code().is_some_and(|x| x == -1073741510); // 0xC000013A (Ctrl+C)

                                    #[cfg(not(any(unix, windows)))]
                                    let interrupted = i.code().is_none();

                                    if i.success() {
                                        state.should_quit = true;
                                    } else if i.code().is_some_and(|x| x == 100) || interrupted
                                    {
                                        // resume on _user_ termination signal
                                    } else {
                                        println!("\nPress enter to continue...");
                                        let mut input = String::new();
                                        let _ = std::io::stdin().read_line(&mut input);
                                    }
                                }
                                Some(3) => {
                                    if i.success() {
                                        state.should_quit = true;
                                    }
                                    
                                    // quit on **any abnormal** exit
                                    if i.code().is_none() {
                                        state.should_quit_nomatch = true;
                                    }

                                    #[cfg(unix)]
                                    {
                                        use std::os::unix::process::ExitStatusExt;
                                        if i.stopped_signal().is_some() {
                                            // better to propogate this signal but this is a standby for now
                                            state.should_quit_nomatch = true; 
                                        }
                                    }
                                    
                                    #[cfg(windows)]
                                    {
                                        if let Some(code) = i.code() && code < 0 {
                                            log::error!("Child process suffered a system crash/abnormal exit: 0x{:X}", code);
                                            state.should_quit_nomatch = true;
                                        }
                                    }
                                }
                                _ => {}
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
                let cmd = use_formatter(&formatter_, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();
                
                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                
                if let Some(mut _child) = Command::from_script(&cmd)
                .envs(vars)
                .stdin(tty_or_inherit())
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
