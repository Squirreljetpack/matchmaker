use std::{
    env,
    fmt::{self, Debug, Formatter},
    fs::OpenOptions,
    io::{self, Write},
    process::{Command, Stdio},
    sync::Arc,
};

use cba::{bait::ResultExt, bath::PathExt, broc::CommandExt, env_vars};
use easy_ext::ext;
use log::{debug, info, warn};
use ratatui::text::Text;
use tokio::io::AsyncReadExt;

use crate::{
    MatchError, RenderFn, Result, SSS, Selector,
    action::{Action, ActionExt, Actions, NullActionExt},
    binds::BindMap,
    config::{
        ColumnsConfig, ExitConfig, OverlayConfig, PreviewerConfig, RenderConfig, Split,
        TerminalConfig, WorkerConfig,
    },
    event::{EventLoop, RenderSender},
    message::{Event, Interrupt, RenderCommand},
    nucleo::{
        ConfigPreprocessedData, Worker,
        injector::{Either, PreprocessOptions, WorkerInjector},
    },
    preview::{
        AppendOnly, Preview,
        previewer::{PreviewMessage, Previewer},
    },
    render::{self, BoxedHandler, DynamicMethod, EventHandlers, InterruptHandlers, MMState},
    tui,
    ui::{Overlay, OverlayUI, UI},
    utils::{
        text::is_empty,
        tokio::{tokio_command_from_script, wait_with_timeout},
    },
};

/// A boxed closure that produces the `Vec<S>` result of a pick.
///
/// The closure receives a `&mut MMState<T, D>` and may inspect the selector
/// and current item to build the result.
pub type AcceptHook<T, D, S> =
    Box<dyn FnOnce(&mut MMState<'_, '_, T, D>) -> Vec<S> + Send + Sync + 'static>;

/// The main entrypoint of the library. To use:
/// 1. create your worker (T -> Columns)
/// 2. Instantiate this with Matchmaker::new(worker, accept_hook)
/// 3. Register your handlers
///    3.5 Start and connect your previewer
/// 4. Call mm.pick()
pub struct Matchmaker<T: SSS, S, D = ()> {
    pub worker: Worker<T, D>,
    pub render_config: RenderConfig,
    pub tui_config: TerminalConfig,
    pub exit_config: ExitConfig,
    pub output: AcceptHook<T, D, S>,
    pub event_handlers: EventHandlers<T, D>,
    pub interrupt_handlers: InterruptHandlers<T, D>,
}

// ----------- MAIN -----------------------

pub struct OddEnds {
    pub hidden_columns: Vec<bool>,
    pub has_error: bool,
}

pub fn set_host_clipboard_universal(text: &str) -> io::Result<()> {
    use base64::Engine;
    // 1. Encode the payload
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    let sequence = format!("\x1b]52;c;{}\x07", encoded);

    // 2. Determine the direct TTY path
    // If we are over SSH, $SSH_TTY will be set to the exact device file.
    // Otherwise, we default to the current process's controlling terminal.
    let tty_path = env::var("SSH_TTY").unwrap_or_else(|_| "/dev/tty".to_string());

    // 3. Attempt to open the TTY file directly
    match OpenOptions::new().write(true).open(&tty_path) {
        Ok(mut tty_file) => {
            // Write directly to the TTY, completely bypassing standard output, Zellij, and tmux.
            write!(tty_file, "{}", sequence)?;
            tty_file.flush()?;
        }
        Err(_) => {
            // 4. Fallback if /dev/tty isn't available
            // If the direct TTY fails (e.g., on Windows), we fall back to standard output.
            // Here, we can still include the tmux check just in case.
            let fallback_sequence = if env::var("TMUX").is_ok() {
                format!("\x1bPtmux;\x1b\x1b]52;c;{}\x07\x1b\\", encoded)
            } else {
                sequence
            };

            let mut stdout = io::stdout();
            write!(stdout, "{}", fallback_sequence)?;
            stdout.flush()?;
        }
    }

    Ok(())
}

pub type ConfigInjector = WorkerInjector<String, crate::nucleo::ConfigPreprocessedData>;
pub type ConfigMatchmaker = Matchmaker<String, String, ConfigPreprocessedData>;
pub type ConfigMMItem = String;

impl ConfigMatchmaker {
    #[allow(unused)]
    /// Creates a new Matchmaker from a config::BaseConfig.
    /// Calls [`Matchmaker::prepare`];
    pub fn new_from_config(
        render_config: RenderConfig,
        tui_config: TerminalConfig,
        worker_config: WorkerConfig,
        columns_config: ColumnsConfig,
        exit_config: ExitConfig,
        preprocess_config: PreprocessOptions,
    ) -> (Self, ConfigInjector, OddEnds) {
        let mut has_error = false;

        let cc = columns_config;
        let hidden_columns = cc.names.iter().map(|x| x.hidden).collect();
        let offset = !cc.names_from_zero as usize;

        // Build column names
        let column_names: Vec<Arc<str>> = if cc.names.is_empty() {
            (offset..(cc.max_cols() + offset))
                .map(|n| Arc::from(n.to_string()))
                .collect()
        } else {
            cc.names
                .iter()
                .take(cc.max_cols())
                .map(|s| Arc::from(s.name.as_str()))
                .collect()
        };

        // Handle Split::None case - use empty name
        let (column_names, split) = match cc.split {
            Split::None => (vec![Arc::from("")], Split::None),
            _ => (column_names, cc.split),
        };

        // Build columns using the new function
        let (columns, raw_preprocessor, text_preprocessor, default_index) =
            crate::nucleo::build_columns_for_config(
                preprocess_config,
                split,
                column_names,
                cc.default.as_ref().map(|x| x.0.as_str()),
            );

        let mut worker: Worker<String, crate::nucleo::ConfigPreprocessedData> =
            Worker::new(columns, default_index, raw_preprocessor, text_preprocessor);

        #[cfg(feature = "experimental")]
        {
            worker.reverse_items(worker_config.reverse);
            worker.set_stability(*worker_config.sort_threshold);
            for (i, c) in cc.names.iter().enumerate() {
                worker.set_column_options(i, c.options)
            }
        }

        let injector = worker.injector();

        // Build the default accept_hook: returns empty Vec<String> by default.
        // The CLI overrides this in `start.rs` via the `on_accept`/`output_template`
        // logic. The hook is just a placeholder for the structural refactor; the
        // real accept pipeline will replace it.
        let accept_hook = Box::new(
            |_state: &mut MMState<'_, '_, String, ConfigPreprocessedData>| -> Vec<String> {
                vec![]
            },
        ) as AcceptHook<String, ConfigPreprocessedData, String>;

        let event_handlers = EventHandlers::new();
        let interrupt_handlers = InterruptHandlers::new();

        let mut new = Matchmaker {
            worker,
            render_config,
            tui_config,
            exit_config,
            output: accept_hook,
            event_handlers,
            interrupt_handlers,
        };
        new.prepare();

        let misc = OddEnds {
            hidden_columns,
            has_error,
        };

        (new, injector, misc)
    }
}

impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    /// Construct a `Matchmaker` with default config and the given accept hook.
    pub fn new<F>(worker: Worker<T, D>, accept_hook: F) -> Self
    where
        F: FnOnce(&mut MMState<'_, '_, T, D>) -> Vec<S> + Send + Sync + 'static,
    {
        Matchmaker {
            worker,
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            output: Box::new(accept_hook),
            event_handlers: EventHandlers::new(),
            interrupt_handlers: InterruptHandlers::new(),
        }
    }

    /// Configure the UI
    pub fn config_render(&mut self, render: RenderConfig) -> &mut Self {
        self.render_config = render;
        self
    }
    /// Configure the TUI
    pub fn config_tui(&mut self, tui: TerminalConfig) -> &mut Self {
        self.tui_config = tui;
        self
    }
    /// Configure exit conditions
    pub fn config_exit(&mut self, exit: ExitConfig) -> &mut Self {
        self.exit_config = exit;
        self
    }
    /// Register a handler to listen on [`Event`]s
    pub fn register_event_handler<F>(&mut self, event: Event, handler: F)
    where
        F: Fn(&mut MMState<'_, '_, T, D>, &Event) + 'static,
    {
        let boxed = Box::new(handler);
        self.register_boxed_event_handler(event, boxed);
    }
    /// Register a boxed handler to listen on [`Event`]s
    pub fn register_boxed_event_handler(
        &mut self,
        event: Event,
        handler: DynamicMethod<T, D, Event>,
    ) {
        self.event_handlers.set(event, handler);
    }
    /// Register a handler to listen on [`Interrupt`]s
    pub fn register_interrupt_handler<F>(&mut self, interrupt: Interrupt, handler: F)
    where
        F: FnMut(&mut MMState<'_, '_, T, D>) + 'static,
    {
        let boxed = Box::new(handler);
        self.register_boxed_interrupt_handler(interrupt, boxed);
    }
    /// Register a boxed handler to listen on [`Interrupt`]s
    pub fn register_boxed_interrupt_handler(
        &mut self,
        variant: Interrupt,
        handler: BoxedHandler<T, D>,
    ) {
        self.interrupt_handlers.set(variant, handler);
    }

    pub fn prepare(&mut self) {
        self.worker.find(&self.render_config.query.initial)
    }

    /// The main method of the Matchmaker. It starts listening for events and renders the TUI with ratatui. It successfully returns with all the selected items selected when the Accept action is received.
    pub async fn pick<A: ActionExt>(self, builder: PickOptions<'_, T, D, A>) -> Result<Vec<S>> {
        let PickOptions {
            previewer,
            ext_handler,
            ext_aliaser,
            #[cfg(feature = "bracketed-paste")]
            paste_handler,
            overlay_config,
            hidden_columns,
            initializer,
            ..
        } = builder;

        let mut event_loop = if let Some(e) = builder.event_loop {
            e
        } else if let Some(binds) = builder.binds {
            EventLoop::with_binds(binds).with_tick_rate(self.render_config.tick_rate())
        } else {
            EventLoop::new()
        };

        let mut wait = false;
        if let Some(path) = self.exit_config.last_key_path.clone()
            && !path.is_empty()
        {
            event_loop.record_last_key(path);
            wait = true;
        }

        let preview = match previewer {
            Some(Either::Left(view)) => Some(view),
            Some(Either::Right(mut previewer)) => {
                let view = previewer.view();
                previewer.connect_controller(event_loop.controller());

                tokio::spawn(async move {
                    let _ = previewer.run().await;
                });

                Some(view)
            }
            _ => None,
        };

        let (render_tx, render_rx) = builder
            .channel
            .unwrap_or_else(tokio::sync::mpsc::unbounded_channel);
        event_loop.add_tx(render_tx.clone());

        let mut tui =
            tui::Tui::new(self.tui_config).map_err(|e| MatchError::TUIError(e.to_string()))?;
        tui.enter()
            .map_err(|e| MatchError::TUIError(e.to_string()))?;

        // important to start after tui
        let event_controller = event_loop.controller();
        let event_controller_ = event_controller.clone();
        let bind_controller = event_loop.bind_controller();
        let event_loop_handle = tokio::spawn(async move {
            let _ = event_loop.run().await;
        });
        log::debug!("event loop started");

        let overlay_ui = if builder.overlays.is_empty() {
            None
        } else {
            Some(OverlayUI::new(
                builder.overlays.into_boxed_slice(),
                overlay_config.unwrap_or_default(),
            ))
        };

        let matcher = if let Some(matcher) = builder.matcher {
            matcher
        } else {
            &mut nucleo::Matcher::new(nucleo::Config::DEFAULT)
        };

        let (ui, picker, footer, preview) = UI::new(
            self.render_config,
            matcher,
            self.worker,
            Selector::new(),
            preview,
            &mut tui,
            hidden_columns,
        );

        // initial redraw to clear artifacts,
        tui.redraw();

        let ret = render::render_loop(
            ui,
            picker,
            footer,
            preview,
            tui,
            overlay_ui,
            self.exit_config,
            render_rx,
            event_controller,
            bind_controller,
            self.output,
            (self.event_handlers, self.interrupt_handlers),
            ext_handler,
            ext_aliaser,
            initializer,
            #[cfg(feature = "bracketed-paste")]
            paste_handler,
        )
        .await;

        log::trace!("render loop finished");

        if wait && event_controller_.send(Event::Resume).is_ok() {
            let _ = event_loop_handle.await;
            log::debug!("event loop finished");
        }

        ret
    }

    pub async fn pick_default(self) -> Result<Vec<S>> {
        self.pick::<NullActionExt>(PickOptions::new()).await
    }
}

impl<T: SSS + Clone, D: 'static> Matchmaker<T, T, D> {
    /// Construct a `Matchmaker` whose accept hook clones the user's selected items
    /// (`T::clone()`) — or, when no items are selected, clones the currently active
    /// item. The returned `Vec<T>` is collected for the caller.
    pub fn new_on_cloneable(worker: Worker<T, D>) -> Self {
        Self::new(worker, |state| {
            state.map_selected_to_vec(|_, item| item.clone())
        })
    }
}

#[ext(MatchResultExt)]
impl<T> Result<T> {
    /// Return the first element
    pub fn first<S>(self) -> Result<S>
    where
        T: IntoIterator<Item = S>,
    {
        match self {
            Ok(v) => v.into_iter().next().ok_or(MatchError::NoMatch),
            Err(e) => Err(e),
        }
    }

    /// Handle [`MatchError::Abort`] using [`std::process::exit`]
    pub fn abort(self) -> Result<T> {
        match self {
            Err(MatchError::Abort(x)) => std::process::exit(x),
            _ => self,
        }
    }
}

// --------- BUILDER -------------

/// Returns what should be pushed to input
pub type PasteHandler<T, D> =
    Box<dyn FnMut(String, &MMState<'_, '_, T, D>) -> String + Send + Sync + 'static>;

pub type ActionExtHandler<T, D, A> =
    Box<dyn FnMut(A, &mut MMState<'_, '_, T, D>) + Send + Sync + 'static>;

pub type ActionAliaser<T, D, A> =
    Box<dyn FnMut(Action<A>, &mut MMState<'_, '_, T, D>) -> Actions<A> + Send + Sync + 'static>;

pub type Initializer<T, D> = Box<dyn FnOnce(&mut MMState<'_, '_, T, D>) + Send + Sync + 'static>;

/// Used to configure [`Matchmaker::pick`] with additional options.
pub struct PickOptions<'a, T: SSS, D, A: ActionExt = NullActionExt> {
    matcher: Option<&'a mut nucleo::Matcher>,
    matcher_config: nucleo::Config,

    event_loop: Option<EventLoop<A>>,
    binds: Option<BindMap<A>>,

    ext_handler: Option<ActionExtHandler<T, D, A>>,
    ext_aliaser: Option<ActionAliaser<T, D, A>>,
    #[cfg(feature = "bracketed-paste")]
    paste_handler: Option<PasteHandler<T, D>>,

    overlays: Vec<Box<dyn Overlay<A = A>>>,
    overlay_config: Option<OverlayConfig>,
    previewer: Option<Either<Preview, Previewer>>,

    hidden_columns: Vec<bool>,

    // Initializing code, i.e. to setup state.
    initializer: Option<Initializer<T, D>>,
    pub channel: Option<(
        RenderSender<A>,
        tokio::sync::mpsc::UnboundedReceiver<crate::message::RenderCommand<A>>,
    )>,
}

impl<'a, T: SSS, D, A: ActionExt> PickOptions<'a, T, D, A> {
    pub const fn new() -> Self {
        Self {
            matcher: None,
            event_loop: None,
            previewer: None,
            binds: None,
            matcher_config: nucleo::Config::DEFAULT,
            ext_handler: None,
            ext_aliaser: None,
            #[cfg(feature = "bracketed-paste")]
            paste_handler: None,
            overlay_config: None,
            overlays: Vec::new(),
            channel: None,
            hidden_columns: Vec::new(),
            initializer: None,
        }
    }

    pub fn with_binds(binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        ret.binds = Some(binds);
        ret
    }

    pub fn with_matcher(matcher: &'a mut nucleo::Matcher) -> Self {
        let mut ret = Self::new();
        ret.matcher = Some(matcher);
        ret
    }

    pub fn binds(mut self, binds: BindMap<A>) -> Self {
        self.binds = Some(binds);
        self
    }

    pub fn event_loop(mut self, event_loop: EventLoop<A>) -> Self {
        self.event_loop = Some(event_loop);
        self
    }

    /// Use the given [`Previewer`] to provide a [`Preview`].
    /// # Example
    /// See [`make_previewer`] for how to create one.
    pub fn previewer(mut self, previewer: Previewer) -> Self {
        self.previewer = Some(Either::Right(previewer));
        self
    }

    /// Set a [`Preview`].
    /// Overrides [`Matchmaker::connect_preview`].
    pub fn preview(mut self, preview: Preview) -> Self {
        self.previewer = Some(Either::Left(preview));
        self
    }

    pub fn matcher(mut self, matcher_config: nucleo::Config) -> Self {
        self.matcher_config = matcher_config;
        self
    }

    pub fn hidden_columns(mut self, hidden_columns: Vec<bool>) -> Self {
        self.hidden_columns = hidden_columns;
        self
    }

    pub fn ext_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut(A, &mut MMState<'_, '_, T, D>) + Send + Sync + 'static,
    {
        self.ext_handler = Some(Box::new(handler));
        self
    }

    pub fn ext_aliaser<F>(mut self, aliaser: F) -> Self
    where
        F: FnMut(Action<A>, &mut MMState<'_, '_, T, D>) -> Actions<A> + Send + Sync + 'static,
    {
        self.ext_aliaser = Some(Box::new(aliaser));
        self
    }

    pub fn initializer<F>(mut self, handler: F) -> Self
    where
        F: FnOnce(&mut MMState<'_, '_, T, D>) + Send + Sync + 'static,
    {
        self.initializer = Some(Box::new(handler));
        self
    }

    #[cfg(feature = "bracketed-paste")]
    pub fn paste_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut(String, &MMState<'_, '_, T, D>) -> String + Send + Sync + 'static,
    {
        self.paste_handler = Some(Box::new(handler));
        self
    }

    pub fn overlay<O>(mut self, overlay: O) -> Self
    where
        O: Overlay<A = A> + 'static,
    {
        self.overlays.push(Box::new(overlay));
        self
    }

    pub fn overlay_config(mut self, overlay_config: OverlayConfig) -> Self {
        self.overlay_config = Some(overlay_config);
        self
    }

    pub fn render_tx(&mut self) -> RenderSender<A> {
        if let Some((s, _)) = &self.channel {
            s.clone()
        } else {
            let channel = tokio::sync::mpsc::unbounded_channel();
            let ret = channel.0.clone();
            self.channel = Some(channel);
            ret
        }
    }
}

impl<'a, T: SSS, D, A: ActionExt> Default for PickOptions<'a, T, D, A> {
    fn default() -> Self {
        Self::new()
    }
}

// ----------- ATTACHMENTS ------------------

pub type AttachmentFormatter<T, D> = Either<
    Arc<RenderFn<T>>,
    for<'a, 'b, 'c> fn(&'a MMState<'b, 'c, T, D>, &'a str, Option<&dyn Fn(String)>) -> String,
>;

// we could check if template is empty here to avoid allocating but feels like it might be a footgun
pub fn use_formatter<T: SSS, D: 'static>(
    formatter: &AttachmentFormatter<T, D>,
    state: &MMState<'_, '_, T, D>,
    template: &str,
    repeat: Option<&dyn Fn(String)>,
) -> String {
    match formatter {
        Either::Left(f) => {
            if let Some(t) = state.current_raw() {
                f(t, template)
            } else {
                String::new()
            }
        }
        Either::Right(f) => f(state, template, repeat),
    }
}

// todo: this static bound shouldn't be necessary on S i don't know why its needed

/// A set of methods for registering the "standard" functionality for various interrupts/events.
/// These methods are prefixed with _ to indicate that library users will often prefer to override them.
impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    // technically we don't need concurrency but the cost should be negligable
    /// Causes [`Action::Print`] to print to stdout.
    pub fn _register_print_handler(
        &mut self,
        print_handle: AppendOnly<String>,
        output_separator: String,
        formatter: AttachmentFormatter<T, D>,
    ) {
        self.register_interrupt_handler(Interrupt::Print, move |state| {
            let template = state.payload().clone();
            let repeat = |s: String| {
                if atty::is(atty::Stream::Stdout) {
                    print_handle.push(s);
                } else {
                    print!("{}{}", s, output_separator);
                }
            };
            let s = use_formatter(&formatter, state, &template, Some(&repeat));
            if !s.is_empty() {
                repeat(s)
            }
        });
    }

    /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn _register_execute_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        let formatter_1 = formatter.clone();
        self.register_interrupt_handler(Interrupt::Execute, move |state| {
            let template = state.payload();

            if !template.is_empty() {
                let cmd = use_formatter(&formatter_1, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_1, state, &preview_template, None);
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
                        }
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
        });

        let formatter_2 = formatter.clone();
        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            let template = state.payload();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_2, state, template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_2, state, &preview_template, None);
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

    /// Causes [`Action::ExecuteAsync`] and [`Action::ExecuteThen`] to execute their payload without blocking, and for the remaining actions in the batch to depend on the execution result.
    pub fn _register_execute_async_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p >= 2)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let id = payload / 2;
                let require_success = (payload % 2) == 1;

                let closure_opt = state.take_actions(id);

                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);

                tokio::spawn(async move {
                    let mut child = match tokio_command_from_script(&cmd)
                        .envs(vars)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!("Failed to spawn async command [{}]: {}", cmd, e);
                            return;
                        }
                    };

                    match child.wait().await {
                        Ok(s) => {
                            info!("Async command [{}] exited with {}", cmd, s);
                            if (require_success || s.success())
                                && let Some(closure) = closure_opt
                            {
                                closure();
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to wait on async command [{}]: {}", cmd, e);
                        }
                    }
                });
            }
        });
    }

    /// Causes [`Action::Copy`] and [`Action::CopySync`] to execute their payload, and copy the result to the clipboard.
    /// Note:
    /// - intended for direct use
    pub fn register_copy<A: ActionExt + Send + 'static>(
        &mut self,
        formatter: AttachmentFormatter<T, D>,
        copy_trailing_newline: bool,
        render_tx: Option<RenderSender<A>>,
    ) {
        let formatter_1 = formatter.clone();
        let render_tx_1 = render_tx.clone();
        self.register_interrupt_handler(Interrupt::ExecuteAsync, move |state| {
            if state.discriminant_payload.as_ref().is_some_and(|p| *p <= 1)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter_1, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let vars = state.make_env_vars();
                let render_tx = render_tx_1.clone();

                tokio::spawn(async move {
                    let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());
                    let mut child = match tokio_command_from_script(&cmd)
                        .envs(vars)
                        .stdin(Stdio::null())
                        .stdout(Stdio::piped())
                        .stderr(Stdio::null())
                        .spawn()
                    {
                        Ok(c) => c,
                        Err(e) => {
                            log::warn!("Failed to spawn copy command [{}]: {}", cmd, e);
                            return;
                        }
                    };

                    let mut text = String::new();
                    if let Some(mut stdout) = child.stdout.take() {
                        let _ = stdout.read_to_string(&mut text).await;
                    }

                    if !copy_trailing_newline && text.ends_with('\n') {
                        text.pop();

                        if text.ends_with('\r') {
                            text.pop();
                        }
                    }

                    let _ = child.wait().await;

                    if !text.is_empty() {
                        if payload == 1 {
                            if let Err(e) = set_host_clipboard_universal(&text) {
                                log::warn!("Failed to set host clipboard: {}", e);
                            }

                            if let Some(tx) = render_tx {
                                let _ = tx.send(RenderCommand::Action(Action::Redraw));
                            }
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 0: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let mut child = match tokio_command_from_script(&clip_cmd)
                                    .stdin(Stdio::piped())
                                    .spawn()
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        log::warn!("Failed to spawn CLIPcmd [{}]: {}", clip_cmd, e);
                                        return;
                                    }
                                };

                                if let Some(mut stdin) = child.stdin.take() {
                                    use tokio::io::AsyncWriteExt;
                                    let _ = stdin.write_all(text.as_bytes()).await;
                                    let _ = stdin.flush().await;
                                }
                                let _ = child.wait().await;
                            }
                        }
                    }
                });
            }
        });

        self.register_interrupt_handler(Interrupt::ExecuteSilent, move |state| {
            if state
                .discriminant_payload
                .as_ref()
                .is_some_and(|p| *p == 2 || *p == 3)
                && let payload = state.discriminant_payload.take().unwrap()
                && let template = state.payload()
                && !template.is_empty()
            {
                let cmd = use_formatter(&formatter, state, template, None);
                if cmd.is_empty() {
                    return;
                }

                let vars = state.make_env_vars();
                let clip_cmd = vars.get("CLIPcmd").map(|x| x.to_string());

                if let Some(contents) = Command::from_script(&cmd)
                    .envs(vars)
                    .read_to_string()
                    ._elog()
                {
                    let mut text = contents;

                    if !copy_trailing_newline && text.ends_with('\n') {
                        text.pop();

                        if text.ends_with('\r') {
                            text.pop();
                        }
                    }

                    if !text.is_empty() {
                        if payload == 3 {
                            if let Err(e) = set_host_clipboard_universal(&text) {
                                log::warn!("Failed to set host clipboard: {}", e);
                            }

                            if let Some(tx) = render_tx.as_ref() {
                                let _ = tx.send(RenderCommand::Action(Action::Redraw));
                            }
                        } else if let Some(clip_cmd) = clip_cmd {
                            // discriminant 2: use CLIPcmd
                            if !clip_cmd.is_empty() {
                                let Some(mut child) = Command::from_script(&clip_cmd)
                                    .stdin(Stdio::piped())
                                    ._spawn()
                                else {
                                    return;
                                };

                                if let Some(mut stdin) = child.stdin.take() {
                                    let _ = stdin.write_all(text.as_bytes());
                                    let _ = stdin.flush();
                                } else {
                                    log::error!("CLIPcmd had no stdin");
                                }

                                wait_with_timeout(child, std::time::Duration::from_millis(500));
                            }
                        }
                    }
                }
            }
        });
    }

    /// Causes [`Action::Become`] to cause the program to become the program specified by its payload.
    /// Note:
    /// - not intended for direct use.
    /// - Assumes preview and cmd formatter are the same.
    pub fn _register_become_handler(&mut self, formatter: AttachmentFormatter<T, D>) {
        let formatter_2 = formatter.clone();
        self.register_interrupt_handler(Interrupt::Become, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter, state, &template, None);
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
                debug!("Becoming: {cmd}");

                Command::from_script(&cmd).envs(vars)._exec()
            }
        });
        self.register_interrupt_handler(Interrupt::BecomeSilent, move |state| {
            let template = state.payload().clone();
            if !template.is_empty() {
                let cmd = use_formatter(&formatter_2, state, &template, None);
                if cmd.is_empty() {
                    return;
                }
                let mut vars = state.make_env_vars();

                let preview_template = state.preview_payload().clone();
                let preview_cmd = use_formatter(&formatter_2, state, &preview_template, None);
                let extra = env_vars!(
                    "MM_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                debug!("Becoming: {cmd}");

                Command::from_script(&cmd).envs(vars)._exec()
            }
        });
    }
}

/// Causes the program to display a preview of the active result.
/// The Previewer can be connected to [`Matchmaker`] using [`PickOptions::previewer`]
pub fn make_previewer<T: SSS, S, D: 'static>(
    mm: &mut Matchmaker<T, S, D>,
    previewer_config: PreviewerConfig, // note: help_str is provided separately so help_colors is ignored
    formatter: AttachmentFormatter<T, D>,
    help_factory: Box<dyn Fn(&crate::config::HelpDisplayConfig) -> Text<'static> + Send + Sync>,
) -> Previewer {
    // initialize previewer
    let (previewer, tx) = Previewer::new(previewer_config.clone());
    let preview_tx = tx.clone();
    let formatter_clone = formatter.clone();

    let help_config = previewer_config.help.clone();

    // preview handler
    // important that PreviewSet events don't accidentally trigger this!
    mm.register_event_handler(Event::CursorChange | Event::PreviewChange | Event::Synced, move |state, _| {
            // don't clobber previewset events
            if state.contains(Event::PreviewSet) {
                // code logic-wise, recieve PreviewSet::None semantically => will recieve PreviewMessage::Unset => we should skip anyways (events is immutable), altho semantically such a state should actually trigger a new preview tho it would be niche
                return;
            }

            if state.preview_visible() &&
            let m = state.preview_payload().clone() &&
            let cmd = use_formatter(&formatter, state, &m, None) &&
            !cmd.is_empty()
            {
                let mut envs = state.make_env_vars();
                let extra = env_vars!(
                    "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                    "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                );
                envs.extend(extra);

                let msg = PreviewMessage::Run(cmd.clone(), envs);
                if preview_tx.send(msg.clone()).is_err() {
                    warn!("Failed to send to preview: {}", msg)
                }

                // -----------------
                let target = state.preview_ui.as_ref().and_then(|p| p.config.initial.index.as_ref().and_then(|index_col| {
                    state.current_raw().and_then(|item| {
                        state.picker_ui.worker.format_with(item, index_col).and_then(|t| atoi::atoi(t.as_bytes()))
                    })
                }));

                if let Some(p) = state.preview_ui {
                    p.set_target(target);
                    p.jump = Default::default();
                };

            } else if preview_tx.send(PreviewMessage::Stop).is_err() {
                warn!("Failed to send to preview: stop")
            }

            state.preview_set_payload = None; // reset None here instead of on consume so that ::Help can toggle
        }
    );

    mm.register_event_handler(Event::PreviewSet, move |state, _event| {
        if state.preview_visible() {
            let payload = state.preview_set_payload();
            let msg = match payload {
                Some(Err(m)) => {
                    let m = if is_empty(&m) {
                        help_factory(&help_config)
                    } else {
                        m
                    };
                    PreviewMessage::Set(m)
                }
                None => PreviewMessage::Unset,
                Some(Ok(template)) => {
                    let cmd = use_formatter(&formatter_clone, state, &template, None);
                    if cmd.is_empty() {
                        PreviewMessage::Stop
                    } else {
                        let mut envs = state.make_env_vars();
                        let extra = env_vars!(
                            "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                            "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                        );
                        envs.extend(extra);
                        PreviewMessage::Run(cmd, envs)
                    }
                }
            };

            if tx.send(msg.clone()).is_err() {
                warn!("Failed to send: {}", msg)
            }
        }
    });

    previewer
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

// ------------ BOILERPLATE ---------------

impl<T: SSS, S, D> Debug for Matchmaker<T, S, D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matchmaker")
            .field("render_config", &self.render_config)
            .field("tui_config", &self.tui_config)
            .field("exit_config", &self.exit_config)
            .field("accept_hook", &"<accept_hook>")
            .field("event_handlers", &self.event_handlers)
            .field("interrupt_handlers", &self.interrupt_handlers)
            .finish()
    }
}
