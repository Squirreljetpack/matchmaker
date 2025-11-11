
use std::{fmt::{self, Debug, Formatter}, sync::Arc};

use tokio::sync::{mpsc::UnboundedSender};

use crate::{
    MMItem, MatchError, RenderFn, Result, Selection, SelectionSet, SplitterFn, binds::BindMap, proc::
    Preview, config::{
        self, ExitConfig, MMConfig, RenderConfig, Split, TerminalConfig
    }, event::EventLoop, message::{Event, Interrupt}, nucleo::{
        Indexed, Segmented, Worker, injector::{
            IndexedInjector, Injector, SegmentedInjector, WorkerInjector
        }
    }, render::{
        self,
        DynamicMethod,
        EphemeralState,
        EventHandlers,
        InterruptHandlers,
    }, tui::{self}, ui::UI
};

/// The main entrypoint of the library. To use:
/// 1. create your worker (T -> Columns)
/// 2. Determine your identifier
/// 3. Instantiate this with Matchmaker::new_from_raw(..)
/// 4. Register your handlers
///    4.5 Start and connect your previewer
/// 5. Call mm.pick() or mm.pick_with_matcher(&mut matcher)
pub struct Matchmaker<T: MMItem, S: Selection=T, C=()> {
    pub worker: Worker<T, C>,
    render_config: RenderConfig,
    bind_config: BindMap,
    tui_config: TerminalConfig,
    exit_config: ExitConfig,
    selection_set: SelectionSet<T, S>,
    event_loop: EventLoop,
    context: Arc<C>,
    event_handlers: EventHandlers<T, S, C>,
    interrupt_handlers: InterruptHandlers<T, S, C>,
    previewer: Option<Preview>
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
    /// Creates a new Matchmaker from a config::Config.
    pub fn new_from_config(config: config::Config, matcher_config: MMConfig) -> (Self, ConfigInjector, OddEnds) {
        let cc = matcher_config.columns;

        let worker: Worker<Indexed<Segmented<String>>> = match cc.split {
            Split::Delimiter(_) | Split::Regexes(_) => {
                let names: Vec<Arc<str>> = if cc.names.is_empty() {
                    (0..cc.max_columns.0)
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

        let col_count = worker.columns.len();

        // Arc over box due to capturing
        let splitter: SplitterFn<String> = match cc.split {
            Split::Delimiter(ref rg) => {
                let rg = rg.clone();
                Arc::new(move |s| {
                    let mut ranges = Vec::new();
                    let mut last_end = 0;
                    for (i, m) in rg.find_iter(s).enumerate() {
                        if i >= col_count - 1 { break; }
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
                    let mut ranges = Vec::new();
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
            Split::None => Arc::new(|s| vec![(0, s.len())]),
        };
        let injector= IndexedInjector::new(injector, ());
        let injector= SegmentedInjector::new(injector, splitter.clone());

        let selection_set = SelectionSet::new(Indexed::identifier);

        let event_handlers = EventHandlers::new();
        let interrupt_handlers = InterruptHandlers::new();
        let formatter = Arc::new(worker.make_format_fn::<true>(|item| &item.inner.inner));

        let new: Matchmaker<Indexed<Segmented<String>>, Segmented<String>> = Matchmaker {
            worker,
            bind_config: config.binds,
            render_config: config.render,
            tui_config: config.tui,
            exit_config: matcher_config.exit,
            selection_set,
            context: Arc::new(()),
            event_loop: EventLoop::new(),
            event_handlers,
            interrupt_handlers,
            previewer: None
        };

        let misc = OddEnds {
            formatter,
            splitter
        };

        (new, injector, misc)
    }
}

impl<T: MMItem, S: Selection> Matchmaker<T, S> {
    pub fn new(worker: Worker<T>, identifier: fn(&T) -> (u32, S)) -> Self {
        Matchmaker {
            worker,
            bind_config: BindMap::new(),
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selection_set: SelectionSet::new(identifier),
            context: Arc::new(()),
            event_loop: EventLoop::new(),
            event_handlers: EventHandlers::new(),
            interrupt_handlers: InterruptHandlers::new(),
            previewer: None
        }
    }
}

impl<T: MMItem, S: Selection, C> Matchmaker<T, S, C>
{
    pub fn new_raw(worker: Worker<T, C>, identifier: fn(&T) -> (u32, S), context: Arc<C>) -> Self {
        Matchmaker {
            worker,
            bind_config: BindMap::new(),
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selection_set: SelectionSet::new(identifier),
            context,
            event_handlers: EventHandlers::new(),
            event_loop: EventLoop::new(),
            interrupt_handlers: InterruptHandlers::new(),
            previewer: None
        }
    }

    /// The controller can be used to influence the event loop and by proxy the render loop.
    /// However, it's role is not yet solidified.
    pub fn get_controller(&self) -> UnboundedSender<Event> {
        self.event_loop.get_controller()
    }
    /// The contents of the preview are displayed in a pane when picking.
    pub fn connect_preview(&mut self, preview: Preview) {
        self.previewer = Some(preview);
    }

    /// Configure keybinds
    pub fn config_binds(&mut self, bind_config: BindMap) -> &mut Self {
        self.bind_config = bind_config;
        self
    }
    /// Configure the UI
    pub fn config_render(&mut self, render_config: RenderConfig) -> &mut Self {
        self.render_config = render_config;
        self
    }
    /// Configure the TUI
    pub fn config_tui(&mut self, tui_config: TerminalConfig) -> &mut Self {
        self.tui_config = tui_config;
        self
    }
    /// Configure exit conditions
    pub fn config_exit(&mut self, exit_config: ExitConfig) -> &mut Self {
        self.exit_config = exit_config;
        self
    }

    /// Register a handler to listen on [`Event`]s
    pub fn register_event_handler<F, I>(&mut self, events: I, handler: F)
    where
    F: Fn(EphemeralState<'_, T, S, C>, &Event) + Send + Sync + 'static,
    I: IntoIterator<Item = Event>,
    {
        let boxed = Box::new(handler);
        self.register_boxed_event_handler(events, boxed);
    }
    /// Register a boxed handler to listen on [`Event`]s
    pub fn register_boxed_event_handler<I>(
        &mut self,
        events: I,
        handler: DynamicMethod<T, S, C, Event>,
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
    F: Fn(EphemeralState<'_, T, S, C>, &Interrupt) + Send + Sync + 'static,
    {
        let boxed = Box::new(handler);
        self.register_boxed_interrupt_handler(interrupt, boxed);
    }
    /// Register a boxed handler to listen on [`Interrupt`]s
    pub fn register_boxed_interrupt_handler(
        &mut self,
        variant: Interrupt,
        handler: DynamicMethod<T, S, C, Interrupt>,
    ) {
        self.interrupt_handlers.set(variant, handler);
    }

    /// The main method of the Matchmaker. It starts listening for events and renders the TUI with ratatui. It successfully returns with all the selected items selected when the Accept action is received.
    pub async fn pick_with_matcher(mut self, matcher: &mut nucleo::Matcher) -> Result<Vec<S>, MatchError> {
        if self.exit_config.select_1 && self.worker.counts().0 == 1 {
            return Ok(self.selection_set.map_to_vec([self.worker.get_nth(0).unwrap()]));
        }

        let (render_tx, render_rx) = tokio::sync::mpsc::unbounded_channel();
        self.event_loop.add_tx(render_tx.clone()).set_tick_rate(self.render_config.tick_rate());

        let event_controller = self.event_loop.get_controller();
        self.event_loop.binds(self.bind_config);

        let mut tui = tui::Tui::new(self.tui_config).map_err(|e| MatchError::TUIError(e.to_string()))?;
        tui.enter().map_err(|e| MatchError::TUIError(e.to_string()))?;

        tokio::spawn(async move {
            let _ = self.event_loop.run().await;
        });

        let (ui, picker, preview) = UI::new(self.render_config, matcher, self.worker, self.selection_set, self.previewer, &mut tui);

        render::render_loop(ui, picker, preview, tui, render_rx, event_controller, self.context, (self.event_handlers, self.interrupt_handlers), self.exit_config).await
    }

    pub async fn pick(self) -> Result<Vec<S>, MatchError> {
        let mut matcher=  nucleo::Matcher::new(nucleo::Config::DEFAULT);
        self.pick_with_matcher(&mut matcher).await
    }
}
// ------------ BOILERPLATE ---------------

impl<T: MMItem + Debug, S: Selection + Debug, C: Debug> Debug for Matchmaker<T, S, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matchmaker")
        // omit `worker`
        .field("render_config", &self.render_config)
        .field("bind_config", &self.bind_config)
        .field("tui_config", &self.tui_config)
        .field("selection_set", &self.selection_set)
        .field("context", &self.context)
        .field("event_handlers", &self.event_handlers)
        .field("interrupt_handlers", &self.interrupt_handlers)
        .field("previewer", &self.previewer)
        .finish()
    }
}