use std::{
    fmt::{self, Debug, Formatter},
    process::{Command, Stdio},
    sync::Arc,
};

use arrayvec::ArrayVec;
use cli_boilerplate_automation::{_log, bath::PathExt, broc::CommandExt, env_vars};
use easy_ext::ext;
use log::{debug, info, warn};
use ratatui::text::Text;

use crate::{
    MatchError, RenderFn, Result, SSS, Selection, Selector,
    action::{Action, ActionExt, Actions, NullActionExt},
    binds::BindMap,
    config::{
        ExitConfig, OverlayConfig, PreviewerConfig, RenderConfig, Split, TerminalConfig,
        WorkerConfig,
    },
    event::{EventLoop, RenderSender},
    message::{Event, Interrupt},
    nucleo::{
        Indexed, Segmented, Worker,
        injector::{
            AnsiInjector, Either, IndexedInjector, Injector, PreprocessOptions, SegmentedInjector,
            SplitterFn, WorkerInjector,
        },
    },
    preview::{
        AppendOnly, Preview,
        previewer::{PreviewMessage, Previewer},
    },
    render::{self, BoxedHandler, DynamicMethod, EventHandlers, InterruptHandlers, MMState},
    tui,
    ui::{Overlay, OverlayUI, UI},
};

/// The main entrypoint of the library. To use:
/// 1. create your worker (T -> Columns)
/// 2. Determine your identifier
/// 3. Instantiate this with Matchmaker::new_from_raw(..)
/// 4. Register your handlers
///    4.5 Start and connect your previewer
/// 5. Call mm.pick() or mm.pick_with_matcher(&mut matcher)
pub struct Matchmaker<T: SSS, S: Selection = T> {
    pub worker: Worker<T>,
    pub render_config: RenderConfig,
    pub tui_config: TerminalConfig,
    pub exit_config: ExitConfig,
    pub selector: Selector<T, S>,
    pub event_handlers: EventHandlers<T, S>,
    pub interrupt_handlers: InterruptHandlers<T, S>,
}

// ----------- MAIN -----------------------

// defined for lack of a better way to expose these fns, i.e. to allow clients to request new injectors in case of worker restart
pub struct OddEnds {
    pub formatter: Arc<RenderFn<ConfigMMItem>>,
    pub splitter: SplitterFn<Either<String, Text<'static>>>,
    pub hidden_columns: Vec<bool>
}

pub type ConfigInjector = AnsiInjector<
SegmentedInjector<
Either<String, Text<'static>>,
IndexedInjector<Segmented<Either<String, Text<'static>>>, WorkerInjector<ConfigMMItem>>,
>,
>;
pub type ConfigMatchmaker = Matchmaker<ConfigMMItem, Segmented<Either<String, Text<'static>>>>;
pub type ConfigMMInnerItem = Segmented<Either<String, Text<'static>>>;
pub type ConfigMMItem = Indexed<ConfigMMInnerItem>;

impl ConfigMatchmaker {
    /// Creates a new Matchmaker from a config::BaseConfig.
    pub fn new_from_config(
        render_config: RenderConfig,
        tui_config: TerminalConfig,
        worker_config: WorkerConfig,
        exit_config: ExitConfig,
        preprocess_config: PreprocessOptions,
    ) -> (Self, ConfigInjector, OddEnds) {
        let cc = worker_config.columns;
        let hidden_columns = cc.names.iter().map(|x| x.hidden).collect();
        // "hack" because we cannot make the results stable in the worker as our current hack uses the identifier
        let mut worker: Worker<ConfigMMItem> = match cc.split {
            Split::Delimiter(_) | Split::Regexes(_) => {
                let names: Vec<Arc<str>> = if cc.names.is_empty() {
                    (0..cc.max_cols())
                    .map(|n| Arc::from(n.to_string()))
                    .collect()
                } else {
                    cc.names
                    .iter()
                    .map(|s| Arc::from(s.name.as_str()))
                    .collect()
                };
                Worker::new_indexable(names)
            }
            Split::None => Worker::new_indexable([""]),
        };
        
        #[cfg(feature = "experimental")]
        worker.reverse_items(worker_config.reverse);
        #[cfg(feature = "experimental")]
        worker.set_stability(worker_config.sort_threshold);
        
        let injector = worker.injector();
        
        // the computed number of columns, <= cc.max_columns = MAX_COLUMNS
        let col_count = worker.columns.len();
        
        // Arc over box due to capturing
        let splitter: SplitterFn<Either<String, Text>> = match cc.split {
            Split::Delimiter(ref rg) => {
                let rg = rg.clone();
                Arc::new(move |s| {
                    let s = &s.to_cow();
                    
                    let mut ranges = ArrayVec::new();
                    let mut last_end = 0;
                    for m in rg.find_iter(s).take(col_count) {
                        ranges.push((last_end, m.start()));
                        last_end = m.end();
                    }
                    ranges.push((last_end, s.len()));
                    ranges
                })
            }
            Split::Regexes(ref rgs) => {
                let rgs = rgs.clone(); // or Arc
                Arc::new(move |s| {
                    let s = &s.to_cow();
                    let mut ranges = ArrayVec::new();
                    
                    for re in rgs.iter().take(col_count) {
                        if let Some(m) = re.find(s) {
                            ranges.push((m.start(), m.end()));
                        } else {
                            ranges.push((0, 0));
                        }
                    }
                    ranges
                })
            }
            Split::None => Arc::new(|s| ArrayVec::from_iter([(0, s.to_cow().len())])),
        };
        let injector = IndexedInjector::new_globally_indexed(injector);
        let injector = SegmentedInjector::new(injector, splitter.clone());
        let injector = AnsiInjector::new(injector, preprocess_config);
        
        // segment then index. Question: what about ansi sequences?
        
        let selection_set = if render_config.results.multi {
            Selector::new(Indexed::identifier)
            
        } else {
            Selector::new(Indexed::identifier).disabled()
        };
        
        let event_handlers = EventHandlers::new();
        let interrupt_handlers = InterruptHandlers::new();
        let formatter = Arc::new(worker.default_format_fn::<true>(|item| item.to_cow()));
        
        let new = Matchmaker {
            worker,
            render_config,
            tui_config,
            exit_config,
            selector: selection_set,
            event_handlers,
            interrupt_handlers,
        };
        
        let misc = OddEnds {
            formatter,
            splitter,
            hidden_columns
        };
        
        (new, injector, misc)
    }
}

impl<T: SSS, S: Selection> Matchmaker<T, S> {
    pub fn new(worker: Worker<T>, selector: Selector<T, S>) -> Self {
        Matchmaker {
            worker,
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selector,
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
    F: Fn(&mut MMState<'_, '_, T, S>, &Event) + 'static,
    {
        let boxed = Box::new(handler);
        self.register_boxed_event_handler(event, boxed);
    }
    /// Register a boxed handler to listen on [`Event`]s
    pub fn register_boxed_event_handler(
        &mut self,
        event: Event,
        handler: DynamicMethod<T, S, Event>,
    ) {
        self.event_handlers.set(event, handler);
    }
    /// Register a handler to listen on [`Interrupt`]s
    pub fn register_interrupt_handler<F>(&mut self, interrupt: Interrupt, handler: F)
    where
    F: Fn(&mut MMState<'_, '_, T, S>) + 'static,
    {
        let boxed = Box::new(handler);
        self.register_boxed_interrupt_handler(interrupt, boxed);
    }
    /// Register a boxed handler to listen on [`Interrupt`]s
    pub fn register_boxed_interrupt_handler(
        &mut self,
        variant: Interrupt,
        handler: BoxedHandler<T, S>,
    ) {
        self.interrupt_handlers.set(variant, handler);
    }
    
    /// The main method of the Matchmaker. It starts listening for events and renders the TUI with ratatui. It successfully returns with all the selected items selected when the Accept action is received.
    pub async fn pick<A: ActionExt>(self, builder: PickOptions<'_, T, S, A>) -> Result<Vec<S>> {
        let PickOptions {
            previewer,
            ext_handler,
            ext_aliaser,
            #[cfg(feature = "bracketed-paste")]
            paste_handler,
            overlay_config,
            hidden_columns,
            ..
        } = builder;
        
        if self.exit_config.select_1 && self.worker.counts().0 == 1 {
            return Ok(self
                .selector
                .identify_to_vec([self.worker.get_nth(0).unwrap()]));
            }
            
            let mut event_loop = if let Some(e) = builder.event_loop {
                e
            } else if let Some(binds) = builder.binds {
                EventLoop::with_binds(binds).with_tick_rate(self.render_config.tick_rate())
            } else {
                EventLoop::new()
            };
            
            let mut wait = false;
            if let Some(path) = self.exit_config.last_key_path.clone() && !path.is_empty() {
                event_loop.record_last_key(path);
                wait = true;
            }
            
            // note: this part is "crate-specific" since clients likely use their own previewer
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
            let  event_controller = event_loop.controller();
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
            
            // initial redraw to clear artifacts,
            tui.redraw();
            
            let ret = if let Some(matcher) = builder.matcher {
                let (ui, picker, footer, preview) = UI::new(
                    self.render_config,
                    matcher,
                    self.worker,
                    self.selector,
                    preview,
                    &mut tui,
                    hidden_columns
                );
                render::render_loop(
                    ui,
                    picker,
                    footer,
                    preview,
                    tui,
                    overlay_ui,
                    self.exit_config,
                    render_rx,
                    event_controller,
                    (self.event_handlers, self.interrupt_handlers),
                    ext_handler,
                    ext_aliaser,
                    #[cfg(feature = "bracketed-paste")]
                    paste_handler,
                )
                .await
            } else {
                let mut matcher = nucleo::Matcher::new(nucleo::Config::DEFAULT);
                let (ui, picker, footer, preview) = UI::new(
                    self.render_config,
                    &mut matcher,
                    self.worker,
                    self.selector,
                    preview,
                    &mut tui,
                    hidden_columns
                );
                render::render_loop(
                    ui,
                    picker,
                    footer,
                    preview,
                    tui,
                    overlay_ui,
                    self.exit_config,
                    render_rx,
                    event_controller,
                    (self.event_handlers, self.interrupt_handlers),
                    ext_handler,
                    ext_aliaser,
                    #[cfg(feature = "bracketed-paste")]
                    paste_handler,
                )
                .await
            };
            if wait {
                let _ = event_loop_handle.await;
                log::debug!("event loop finished");
            }
            
            ret
        }
        
        pub async fn pick_default(self) -> Result<Vec<S>> {
            self.pick::<NullActionExt>(PickOptions::new()).await
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
    pub type PasteHandler<T, S> = 
    Box<dyn FnMut(String, &MMState<'_, '_, T, S>) -> String + Send + Sync + 'static>;
    
    pub type ActionExtHandler<T, S, A> =
    Box<dyn FnMut(A, &mut MMState<'_, '_, T, S>) + Send + Sync + 'static>;
    
    pub type ActionAliaser<T, S, A> =
    Box<dyn FnMut(Action<A>, &mut MMState<'_, '_, T, S>) -> Actions<A> + Send + Sync + 'static>;
    
    /// Used to configure [`Matchmaker::pick`] with additional options.
    pub struct PickOptions<'a, T: SSS, S: Selection, A: ActionExt = NullActionExt> {
        matcher: Option<&'a mut nucleo::Matcher>,
        matcher_config: nucleo::Config,
        
        event_loop: Option<EventLoop<A>>,
        binds: Option<BindMap<A>>,
        
        ext_handler: Option<ActionExtHandler<T, S, A>>,
        ext_aliaser: Option<ActionAliaser<T, S, A>>,
        #[cfg(feature = "bracketed-paste")]
        paste_handler: Option<PasteHandler<T, S>>,
        
        overlays: Vec<Box<dyn Overlay<A = A>>>,
        overlay_config: Option<OverlayConfig>,
        previewer: Option<Either<Preview, Previewer>>,

        hidden_columns: Vec<bool>,
        
        /// # Experimental
        // pub signal_handler: Option<(&'static std::sync::atomic::AtomicUsize, SignalHandler<T, S>)>,
        /// Initializing code, i.e. to setup context in the running thread. Since render_loop runs on the same thread this isn't actually necessary
        /// but maybe its a good idea to provide a channel for it anyway?
        // initializer: Option<Box<dyn FnOnce()>>,
        pub channel: Option<(
            RenderSender<A>,
            tokio::sync::mpsc::UnboundedReceiver<crate::message::RenderCommand<A>>,
        )>,
    }
    
    impl<'a, T: SSS, S: Selection, A: ActionExt> PickOptions<'a, T, S, A> {
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
                hidden_columns: Vec::new()
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
        F: FnMut(A, &mut MMState<'_, '_, T, S>) + Send + Sync + 'static,
        {
            self.ext_handler = Some(Box::new(handler));
            self
        }
        
        pub fn ext_aliaser<F>(mut self, aliaser: F) -> Self
        where
        F: FnMut(Action<A>, &mut MMState<'_, '_, T, S>) -> Actions<A>
        + Send
        + Sync
        + 'static,
        {
            self.ext_aliaser = Some(Box::new(aliaser));
            self
        }
        
        #[cfg(feature = "bracketed-paste")]
        pub fn paste_handler<F>(mut self, handler: F) -> Self 
        where
        F: FnMut(String, &MMState<'_, '_, T, S>) -> String
        + Send
        + Sync
        + 'static,
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
        
        pub fn overlay_config(mut self, overlay: OverlayConfig) -> Self {
            self.overlay_config = Some(overlay);
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
    
    impl<'a, T: SSS, S: Selection, A: ActionExt> Default for PickOptions<'a, T, S, A> {
        fn default() -> Self {
            Self::new()
        }
    }
    
    // ----------- ATTACHMENTS ------------------
    
    impl<T: SSS, S: Selection> Matchmaker<T, S> {
        // technically we don't need concurrency but the cost should be negligable
        /// Causes [`Action::Print`] to print to stdout.
        pub fn register_print_handler(
            &mut self,
            print_handle: AppendOnly<String>,
            output_separator: String,
            formatter: Arc<RenderFn<T>>,
        ) {
            self.register_interrupt_handler(Interrupt::Print, move |state| {
                if let Some(t) = state.current_raw() {
                    let s = formatter(t, state.payload());
                    if atty::is(atty::Stream::Stdout) {
                        print_handle.push(s);
                    } else {
                        print!("{}{}", s, output_separator);
                    }
                };
            });
        }
        
        /// Causes [`Action::Execute`] to cause the program to execute the program specified by its payload.
        /// Note:
        /// - not intended for direct use.
        /// - Assumes preview and cmd formatter are the same.
        pub fn register_execute_handler(&mut self, formatter: Arc<RenderFn<T>>) {
            self.register_interrupt_handler(Interrupt::Execute, move |state| {
                let template = state.payload();
                if !template.is_empty()
                && let Some(t) = state.current_raw()
                {
                    let cmd = formatter(t, template);
                    let mut vars = state.make_env_vars();
                    
                    let preview_cmd = formatter(t, state.preview_payload());
                    let extra = env_vars!(
                        "FZF_PREVIEW_COMMAND" => preview_cmd,
                    );
                    vars.extend(extra);
                    
                    if let Some(mut child) = Command::from_script(&cmd)
                    .envs(vars)
                    .stdin(maybe_tty())
                    ._spawn()
                    {
                        match child.wait() {
                            Ok(i) => {
                                info!("Command [{cmd}] exited with {i}")
                            }
                            Err(e) => {
                                info!("Failed to wait on command [{cmd}]: {e}")
                            }
                        }
                    }
                };
            });
        }
        
        /// Causes [`Action::Become`] to cause the program to become the program specified by its payload.
        /// Note:
        /// - not intended for direct use.
        /// - Assumes preview and cmd formatter are the same.
        pub fn register_become_handler(&mut self, formatter: Arc<RenderFn<T>>) {
            self.register_interrupt_handler(Interrupt::Become, move |state| {
                let template = state.payload();
                if !template.is_empty()
                && let Some(t) = state.current_raw()
                {
                    let cmd = formatter(t, template);
                    let mut vars = state.make_env_vars();
                    
                    let preview_cmd = formatter(t, state.preview_payload());
                    let extra = env_vars!(
                        "FZF_PREVIEW_COMMAND" => preview_cmd,
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
    pub fn make_previewer<T: SSS, S: Selection>(
        mm: &mut Matchmaker<T, S>,
        previewer_config: PreviewerConfig, // note: help_str is provided separately so help_colors is ignored
        formatter: Arc<RenderFn<T>>,
        help_str: Text<'static>,
    ) -> Previewer {
        // initialize previewer
        let (previewer, tx) = Previewer::new(previewer_config);
        let preview_tx = tx.clone();
        
        // preview handler
        mm.register_event_handler(Event::CursorChange | Event::PreviewChange, move |state, _| {
            if state.preview_visible &&
            let Some(t) = state.current_raw() &&
            let m = state.preview_payload() &&
            !m.is_empty()
            {
                let cmd = formatter(t, m);
                let mut envs = state.make_env_vars();
                let extra = env_vars!(
                    "COLUMNS" => state.previewer_area().map_or("0".to_string(), |r| r.width.to_string()),
                    "LINES" => state.previewer_area().map_or("0".to_string(), |r| r.height.to_string()),
                );
                envs.extend(extra);
                
                let msg = PreviewMessage::Run(cmd.clone(), envs);
                _log!("{cmd:?}");
                if preview_tx.send(msg.clone()).is_err() {
                    warn!("Failed to send to preview: {}", msg)
                }
                
                let target = state.preview_ui.as_ref().and_then(|p| p.config.scroll.index.as_ref().and_then(|index_col| {
                    state.current_raw().and_then(|item| {
                        state.picker_ui.worker.format_with(item, index_col).and_then(|t| t.parse::<isize>().ok())
                    })
                })).unwrap_or(0); // reset to 0 each time
                
                if let Some(p) = state.preview_ui {
                    p.set_target(target);
                };
                
            } else if preview_tx.send(PreviewMessage::Stop).is_err() {
                warn!("Failed to send to preview: stop")
            }
            
            state.preview_set_payload = None;
        });
        
        mm.register_event_handler(Event::PreviewSet, move |state, _event| {
            if state.preview_visible {
                let msg = if let Some(m) = state.preview_set_payload() {
                    let m = if m.is_empty() && !help_str.lines.is_empty() {
                        help_str.clone()
                    } else {
                        Text::from(m)
                    };
                    PreviewMessage::Set(m)
                } else {
                    PreviewMessage::Unset
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
    
    impl<T: SSS + Debug, S: Selection + Debug> Debug for Matchmaker<T, S> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            f.debug_struct("Matchmaker")
            // omit `worker`
            .field("render_config", &self.render_config)
            .field("tui_config", &self.tui_config)
            .field("selection_set", &self.selector)
            .field("event_handlers", &self.event_handlers)
            .field("interrupt_handlers", &self.interrupt_handlers)
            .finish()
        }
    }
    