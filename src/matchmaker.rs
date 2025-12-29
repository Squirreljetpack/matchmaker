
use std::{fmt::{self, Debug, Formatter}, io::Write, process::Stdio, sync::Arc};

use arrayvec::ArrayVec;
use cli_boilerplate_automation::{broc::{exec_script, spawn_script}, env_vars};
use log::{debug, info, warn};
use ratatui::text::Text;

use crate::{
    MMItem, MatchError, RenderFn, Result, Selection, SelectionSet, SplitterFn, action::{ActionAliaser, ActionExt, ActionExtHandler, NullActionExt}, binds::BindMap, config::{
        ExitConfig, PreviewerConfig, RenderConfig, Split, TerminalConfig, WorkerConfig
    }, efx, event::{EventLoop, RenderSender}, message::{Event, Interrupt}, nucleo::{
        Indexed,
        Segmented,
        Worker,
        injector::{
            IndexedInjector,
            Injector,
            SegmentedInjector,
            WorkerInjector,
        },
    }, preview::{
        AppendOnly, Preview, previewer::{PreviewMessage, Previewer}
    }, render::{
        self, DynamicMethod, Effects, EventHandlers, InterruptHandlers, MMState
    }, tui, ui::{Overlay, OverlayUI, UI}
};


/// The main entrypoint of the library. To use:
/// 1. create your worker (T -> Columns)
/// 2. Determine your identifier
/// 3. Instantiate this with Matchmaker::new_from_raw(..)
/// 4. Register your handlers
///    4.5 Start and connect your previewer
/// 5. Call mm.pick() or mm.pick_with_matcher(&mut matcher)
pub struct Matchmaker<T: MMItem, S: Selection=T> {
    pub worker: Worker<T>,
    render_config: RenderConfig,
    tui_config: TerminalConfig,
    exit_config: ExitConfig,
    selection_set: SelectionSet<T, S>,
    event_handlers: EventHandlers<T, S>,
    interrupt_handlers: InterruptHandlers<T, S>,
    preview: Option<Preview>,
}


// ----------- MAIN -----------------------

// defined for lack of a better way to expose these fns, i.e. to allow clients to request new injectors in case of worker restart
pub struct OddEnds {
    pub formatter: Arc<RenderFn<Indexed<Segmented<String>>>>,
    pub splitter: SplitterFn<String>
}

pub type ConfigInjector = SegmentedInjector<String, IndexedInjector<Segmented<String>, WorkerInjector<Indexed<Segmented<String>>>>>;
pub type ConfigMatchmaker = Matchmaker<Indexed<Segmented<String>>, Segmented<String>>;

impl ConfigMatchmaker {
    /// Creates a new Matchmaker from a config::BaseConfig.
    pub fn new_from_config(render_config: RenderConfig, tui_config: TerminalConfig, worker_config: WorkerConfig) -> (Self, ConfigInjector, OddEnds) {
        let cc = worker_config.columns;

        let worker: Worker<Indexed<Segmented<String>>> = match cc.split {
            Split::Delimiter(_) | Split::Regexes(_) => {
                let names: Vec<Arc<str>> = if cc.names.is_empty() {
                    (0..cc.max_cols())
                    .map(|n| Arc::from(n.to_string()))
                    .collect()
                } else {
                    cc.names.iter().map(|s| Arc::from(s.name.as_str())).collect()
                };
                Worker::new_indexable(names)
            },
            Split::None => {
                Worker::new_indexable([""])
            }
        };

        let injector = worker.injector();

        // the computed number of columns, <= cc.max_columns = MAX_COLUMNS
        let col_count = worker.columns.len();

        // Arc over box due to capturing
        let splitter: SplitterFn<String> = match cc.split {
            Split::Delimiter(ref rg) => {
                let rg = rg.clone();
                Arc::new(move |s| {
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
            Split::None => Arc::new(|s| ArrayVec::from_iter([(0, s.len())])),
        };
        let injector= IndexedInjector::new(injector, 0);
        let injector= SegmentedInjector::new(injector, splitter.clone());

        let selection_set = SelectionSet::new(Indexed::identifier);

        let event_handlers = EventHandlers::new();
        let interrupt_handlers = InterruptHandlers::new();
        let formatter = Arc::new(worker.make_format_fn::<true>(|item| std::borrow::Cow::Borrowed(&item.inner.inner)));

        let new = Matchmaker {
            worker,
            render_config,
            tui_config,
            exit_config: worker_config.exit,
            selection_set,
            event_handlers,
            interrupt_handlers,
            preview: None
        };

        let misc = OddEnds {
            formatter,
            splitter
        };

        (new, injector, misc)
    }
}


impl<T: MMItem, S: Selection> Matchmaker<T, S>
{
    pub fn new(worker: Worker<T>, identifier: fn(&T) -> (u32, S)) -> Self {
        Matchmaker {
            worker,
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selection_set: SelectionSet::new(identifier),
            event_handlers: EventHandlers::new(),
            interrupt_handlers: InterruptHandlers::new(),
            preview: None,
        }
    }

    // pub fn new_raw(worker: Worker<T>, identifier: fn(&T) -> (u32, S)) -> Self {
    //     Matchmaker {
    //         worker,
    //         render_config: RenderConfig::default(),
    //         overlay_config: Default::default(),
    //         tui_config: TerminalConfig::default(),
    //         exit_config: ExitConfig::default(),
    //         selection_set: SelectionSet::new(identifier),
    //         event_handlers: EventHandlers::new(),
    //         interrupt_handlers: InterruptHandlers::new(),
    //         preview: None
    //     }
    // }

    /// The contents of the preview are displayed in a pane when picking.
    pub fn connect_preview(&mut self, preview: Preview) {
        self.preview = Some(preview);
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
    pub fn register_event_handler<F, I>(&mut self, events: I, handler: F)
    where
    F: Fn(&mut MMState<'_, T, S>, &Event) -> Effects + MMItem,
    I: IntoIterator<Item = Event>,
    {
        let boxed = Box::new(handler);
        self.register_boxed_event_handler(events, boxed);
    }
    /// Register a boxed handler to listen on [`Event`]s
    pub fn register_boxed_event_handler<I>(
        &mut self,
        events: I,
        handler: DynamicMethod<T, S, Event>,
    )
    where
    I: IntoIterator<Item = Event>,
    {
        let events_vec: Vec<_> = events.into_iter().collect();
        self.event_handlers.set(events_vec, handler);
    }
    /// Register a handler to listen on [`Interrupt`]s
    pub fn register_interrupt_handler<F>(
        &mut self,
        interrupt: Interrupt,
        handler: F,
    )
    where
    F: Fn(&mut MMState<'_, T, S>, &Interrupt) -> Effects + MMItem,
    {
        let boxed = Box::new(handler);
        self.register_boxed_interrupt_handler(interrupt, boxed);
    }
    /// Register a boxed handler to listen on [`Interrupt`]s
    pub fn register_boxed_interrupt_handler(
        &mut self,
        variant: Interrupt,
        handler: DynamicMethod<T, S, Interrupt>,
    ) {
        self.interrupt_handlers.set(variant, handler);
    }

    /// The main method of the Matchmaker. It starts listening for events and renders the TUI with ratatui. It successfully returns with all the selected items selected when the Accept action is received.
    pub async fn pick<A: ActionExt>(mut self, builder: PickOptions<'_, T, S, A>) -> Result<Vec<S>, MatchError> {
        let PickOptions { previewer, ext_handler, ext_aliaser, .. } = builder;

        if self.exit_config.select_1 && self.worker.counts().0 == 1 {
            return Ok(self.selection_set.identify_to_vec([self.worker.get_nth(0).unwrap()]));
        }

        let mut event_loop = if let Some(e) = builder.event_loop {
            e
        } else if let Some(binds) = builder.binds {
            EventLoop::with_binds(binds).with_tick_rate(self.render_config.tick_rate())
        } else {
            EventLoop::new()
        };

        // note: this part is "crate-specific" since clients likely use their own previewer
        if let Some(mut previewer) = previewer {
            if self.preview.is_none() {
                self.preview = Some(previewer.view());
            }
            previewer.connect_controller(event_loop.get_controller());
            tokio::spawn(async move {
                let _ = previewer.run().await;
            });
        }

        let (render_tx, render_rx) = builder.channel.unwrap_or_else(tokio::sync::mpsc::unbounded_channel);
        event_loop
        .add_tx(render_tx.clone());

        let mut tui = tui::Tui::new(self.tui_config).map_err(|e| MatchError::TUIError(e.to_string()))?;
        tui.enter().map_err(|e| MatchError::TUIError(e.to_string()))?;

        // important to start after tui
        let event_controller = event_loop.get_controller();
        tokio::spawn(async move {
            let _ = event_loop.run().await;
        });
        log::debug!("event loop started");

        let overlay_ui = if builder.overlays.is_empty() {
            None
        } else {
            Some(
                OverlayUI::new(builder.overlays.into_boxed_slice(), self.render_config.overlay.take().unwrap_or_default())
            )
        };

        if let Some(matcher) = builder.matcher {
            let (ui, picker, preview) = UI::new(self.render_config, matcher, self.worker, self.selection_set, self.preview, &mut tui);
            render::render_loop(
                ui,
                picker,
                preview,
                tui,

                overlay_ui,
                self.exit_config,

                render_rx,
                event_controller,
                (self.event_handlers, self.interrupt_handlers),
                ext_handler,
                ext_aliaser,
                // signal_handler
            ).await
        } else {
            let mut matcher=  nucleo::Matcher::new(nucleo::Config::DEFAULT);
            let (ui, picker, preview) = UI::new(self.render_config, &mut matcher, self.worker, self.selection_set, self.preview, &mut tui);
            render::render_loop(
                ui,
                picker,
                preview,
                tui,
                overlay_ui,
                self.exit_config,
                render_rx,
                event_controller,
                (self.event_handlers, self.interrupt_handlers),
                ext_handler,
                ext_aliaser,
                // signal_handler
            ).await
        }
    }

    pub async fn pick_default(self) -> Result<Vec<S>, MatchError> {
        self.pick::<NullActionExt>(PickOptions::new()).await
    }
}

// --------- BUILDER -------------
pub struct PickOptions<'a, T: MMItem, S: Selection, A: ActionExt = NullActionExt> {
    pub matcher: Option<&'a mut nucleo::Matcher>,
    pub event_loop: Option<EventLoop<A>>,
    pub previewer: Option<Previewer>,
    pub binds: Option<BindMap<A>>,
    pub matcher_config: nucleo::Config,
    pub ext_handler: Option<ActionExtHandler<T, S, A>>,
    pub ext_aliaser: Option<ActionAliaser<T, S, A>>,
    // pub signal_handler: Option<(&'static std::sync::atomic::AtomicUsize, SignalHandler<T, S>)>,
    pub overlays: Vec<Box<dyn Overlay<A=A>>>,

    pub channel: Option<(RenderSender<A>, tokio::sync::mpsc::UnboundedReceiver<crate::message::RenderCommand<A>>)>
}

pub type SignalHandler<T, S> = fn(usize, &mut MMState<'_, T, S>);

impl<'a, T: MMItem, S: Selection, A: ActionExt> PickOptions<'a, T, S, A> {
    pub const fn new() -> Self {
        Self {
            matcher: None,
            event_loop: None,
            previewer: None,
            binds: None,
            matcher_config: nucleo::Config::DEFAULT,
            ext_handler: None,
            ext_aliaser: None,
            // signal_handler: None,
            overlays: Vec::new(),
            channel: None
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

    pub fn previewer(mut self, previewer: Previewer) -> Self {
        self.previewer = Some(previewer);
        self
    }

    pub fn matcher(mut self, matcher_config: nucleo::Config) -> Self {
        self.matcher_config = matcher_config;
        self
    }

    pub fn ext_handler(
        mut self,
        handler: ActionExtHandler<T, S, A>,
    ) -> Self {
        self.ext_handler = Some(handler);
        self
    }

    pub fn ext_aliaser(
        mut self,
        aliaser: ActionAliaser<T, S, A>,
    ) -> Self {
        self.ext_aliaser = Some(aliaser);
        self
    }

    pub fn push_overlay<O>(mut self, overlay: O) -> Self
    where
    O: Overlay<A = A> + 'static,
    {
        self.overlays.push(Box::new(overlay));
        self
    }

    // pub fn signal_handler(
    //     mut self,
    //     signal: &'static std::sync::atomic::AtomicUsize,
    //     handler: SignalHandler<T, S>,
    // ) -> Self {
    //     self.signal_handler = Some((signal, handler));
    //     self
    // }

    pub fn get_tx(&mut self) -> RenderSender<A>{
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

impl<'a, T: MMItem, S: Selection, A: ActionExt> Default for PickOptions<'a, T, S, A> {
    fn default() -> Self {
        Self::new()
    }
}


// ----------- ATTACHMENTS ------------------

impl<T: MMItem, S: Selection> Matchmaker<T, S>
{
    pub fn register_print_handler(&mut self, print_handle: AppendOnly<String>, formatter: Arc<RenderFn<T>>) {
        self.register_interrupt_handler(
            Interrupt::Print("".into()),
            move |state, i| {
                if let Interrupt::Print(template) = i
                && let Some(t) = state.current_raw() {
                    let s = formatter(t, template);
                    print_handle.push(s);
                };
                efx![]
            },
        );
    }

    pub fn register_execute_handler(&mut self, formatter: Arc<RenderFn<T>>) {
        let preview_formatter = formatter.clone();

        self.register_interrupt_handler(Interrupt::Execute("".into()), move |state, interrupt| {
            if let Interrupt::Execute(template) = interrupt &&
            !template.is_empty() &&
            let Some(t) = state.current_raw() {
                let cmd = formatter(t, template);
                let mut vars = state.make_env_vars();
                let preview_cmd = preview_formatter(t, state.preview_payload());
                let extra = env_vars!(
                    "FZF_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                let tty = maybe_tty();
                if let Some(mut child) = spawn_script(&cmd, vars, tty, Stdio::inherit(), Stdio::inherit()) {
                    match child.wait() {
                        Ok(i) => {
                            info!("Command [{cmd}] exited with {i}")
                        },
                        Err(e) => {
                            info!("Failed to wait on command [{cmd}]: {e}")
                        }
                    }
                }
            };
            efx![]
        });
    }

    pub fn register_become_handler(&mut self, formatter: Arc<RenderFn<T>>) {
        let preview_formatter = formatter.clone();

        self.register_interrupt_handler(Interrupt::Become("".into()), move |state, interrupt| {
            if let Interrupt::Become(template) = interrupt &&
            !template.is_empty() &&
            let Some(t) = state.current_raw() {
                let cmd = formatter(t, template);
                let mut vars = state.make_env_vars();

                let preview_cmd = preview_formatter(t, state.preview_payload());
                let extra = env_vars!(
                    "FZF_PREVIEW_COMMAND" => preview_cmd,
                );
                vars.extend(extra);
                debug!("Becoming: {cmd}");
                exec_script(&cmd, vars);
            }
            efx![]
        });
    }
}

pub fn make_previewer<T: MMItem, S: Selection>(
    mm: &mut Matchmaker<T, S>,
    previewer_config: PreviewerConfig, // help_str is provided seperately so help_colors is ignored
    formatter: Arc<RenderFn<T>>,
    help_str: Text<'static>
) -> Previewer {
    // initialize previewer
    let (previewer, tx) = Previewer::new(previewer_config);
    mm.connect_preview(previewer.view());
    let preview_tx = tx.clone();

    // preview handler
    mm.register_event_handler([Event::CursorChange, Event::PreviewChange], move |state, _| {
        if state.preview_show &&
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
            if preview_tx.send(msg.clone()).is_err() {
                warn!("Failed to send to preview: {}", msg)
            }
        } else if preview_tx.send(PreviewMessage::Stop).is_err() {
            warn!("Failed to send to preview: stop")
        }

        efx![render::Effect::ClearPreviewSet] //
    });

    mm.register_event_handler([Event::PreviewSet], move |state, _event| {
        if state.preview_show {
            let msg = if let Some(m) = state.preview_set_payload() {
                let m = if m.is_empty() && !help_str.lines.is_empty() {
                    help_str.clone()
                } else {
                    Text::from(m.clone())
                };
                PreviewMessage::Set(m.clone())
            } else {
                PreviewMessage::Unset
            };

            if tx.send(msg.clone()).is_err() {
                warn!("Failed to send: {}", msg)
            }
        }
        efx![]
    });

    previewer
}

fn maybe_tty() -> Stdio {
    if let Ok(mut tty) = std::fs::File::open("/dev/tty") {
        let _ = tty.flush(); // does nothing but seems logical
        Stdio::from(tty)
    } else {
        log::error!("Failed to open /dev/tty");
        Stdio::inherit()
    }
}

// ------------ BOILERPLATE ---------------

impl<T: MMItem + Debug, S: Selection + Debug> Debug for Matchmaker<T, S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matchmaker")
        // omit `worker`
        .field("render_config", &self.render_config)
        .field("tui_config", &self.tui_config)
        .field("selection_set", &self.selection_set)
        .field("event_handlers", &self.event_handlers)
        .field("interrupt_handlers", &self.interrupt_handlers)
        .field("previewer", &self.preview)
        .finish()
    }
}