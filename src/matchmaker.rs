
use std::{fmt::{self, Debug, Formatter}, process::Stdio, sync::Arc};

use anyhow::bail;
use log::{debug, error, info, warn};
use tokio::sync::{watch::Sender};

use crate::{
    PickerItem, Result, Selection, SelectionSet, binds::BindMap, config::{
        self,
        ExitConfig,
        PreviewerConfig,
        RenderConfig,
        Split,
        TerminalConfig,
    }, env_vars, event::{self}, message::{Interrupt, Event}, nucleo::{
        injector::{
            Indexed, IndexedInjector, Injector, Segmented, SegmentedInjector, WorkerInjector
        },
        worker::Worker,
    }, render::{
        self,
        DynamicMethod,
        EphemeralState,
        EventHandlers,
        InterruptHandlers,
    }, spawn::{
        exec, preview::{PreviewMessage, Previewer}, spawn, tty_or_null
    }, tui::{self, map_chunks, map_reader_lines, read_to_chunks}, ui::UI
};

pub struct Matchmaker<T: PickerItem, S: Selection=T, C=()> {
    pub matcher: Option<nucleo::Matcher>,
    pub worker: Worker<T, C>,
    render_config: RenderConfig,
    bind_config: BindMap,
    #[allow(dead_code)]
    tui_config: TerminalConfig,
    exit_config: ExitConfig,
    selection_set: SelectionSet<T, S>,
    context: Arc<C>,
    event_handlers: EventHandlers<T, S, C>,
    interrupt_handlers: InterruptHandlers<T, S, C>,
    previewer: Option<Previewer>
}


// ----------- MAIN -----------------------

// todo: a stopgap until i find some better way to expose these fns, i.e. to allow clients to request new injectors in case of worker restart
pub struct MiscData {
    pub formatter: Arc<Box<dyn Fn(&Indexed<Segmented<String>>, &str) -> String + Send + Sync>>,
    pub splitter: Arc<dyn Fn(&String) -> Vec<(usize, usize)> + Send + Sync>
}

impl Matchmaker<Indexed<Segmented<String>>, Segmented<String>> {
    pub fn new_from_config(config: config::Config) -> (Self, SegmentedInjector<String, IndexedInjector<Segmented<String>, WorkerInjector<Indexed<Segmented<String>>>>>, MiscData) {
        let cc = config.matcher.columns;
        
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
        let splitter: Arc<dyn Fn(&String) -> Vec<(usize, usize)> + Send + Sync> = match cc.split {
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
        
        let (previewer, tx) = Previewer::new(config.previewer);
        
        let mut new: Matchmaker<Indexed<Segmented<String>>, Segmented<String>> = Matchmaker {
            matcher: Some(nucleo::Matcher::new(config.matcher.matcher.0)),
            worker,
            bind_config: config.binds,
            render_config: config.render,
            tui_config: config.tui,
            exit_config: config.matcher.exit,
            selection_set,
            context: Arc::new(()),
            event_handlers,
            interrupt_handlers,
            previewer: Some(previewer)
        };
        
        // handlers
        let preview_formatter = formatter.clone();
        let execute_formatter = preview_formatter.clone();
        let execute_preview_formatter = preview_formatter.clone();
        let become_formatter = preview_formatter.clone();
        let become_preview_formatter = preview_formatter.clone();
        let reload_formatter = preview_formatter.clone();
        
        new.register_event_handler([Event::CursorChange, Event::PreviewChange], move |state, event| {
            match event {
                Event::CursorChange | Event::PreviewChange => {
                    if state.preview_show &&
                    let Some(t) = state.current_raw() &&
                    !state.preview_payload().is_empty()
                    {
                        let cmd = preview_formatter.clone()(t, &state.preview_payload());
                        let mut envs = state.make_env_vars();
                        let extra = env_vars!(
                            "COLUMNS" => state.previewer_area.map_or("0".to_string(), |r| r.width.to_string()),
                            "LINES" => state.previewer_area.map_or("0".to_string(), |r| r.height.to_string()),
                        );
                        envs.extend(extra);
                        
                        let msg = PreviewMessage::Run(cmd.clone(), vec![]);
                        if tx.send(msg.clone()).is_err() {
                            warn!("Failed to send: {}", msg)
                        }
                    }
                },
                _ => {}
            }
        });
        
        new.register_interrupt_handler(Interrupt::Execute("".into()), move |state, interrupt| {
            match interrupt {
                Interrupt::Execute(template) => {
                    if let Some(t) = state.current_raw() {
                        let cmd = execute_formatter(t, template);
                        let mut vars = state.make_env_vars();
                        let preview_cmd = execute_preview_formatter(t, &state.preview_payload());
                        let extra = env_vars!(
                            "FZF_PREVIEW_COMMAND" => preview_cmd,
                        );
                        vars.extend(extra);
                        let tty = tty_or_null();
                        if let Some(mut child) = spawn(&cmd, vars, tty, Stdio::inherit(), Stdio::inherit()) {
                            match child.wait() {
                                Ok(i) => {
                                    info!("Command [{cmd}] exited with {i}")
                                },
                                Err(e) => {
                                    info!("Failed to wait on command [{cmd}]: {e}")
                                }
                            }
                        }
                    }
                },
                _ => {}
            }
        });
        
        new.register_interrupt_handler(Interrupt::Become("".into()), move |state, interrupt| {
            match interrupt {
                Interrupt::Become(template) => {
                    if let Some(t) = state.current_raw() {
                        let cmd = become_formatter(t, template);
                        let mut vars = state.make_env_vars();
                        let preview_cmd = become_preview_formatter(t, &state.preview_payload());
                        let extra = env_vars!(
                            "FZF_PREVIEW_COMMAND" => preview_cmd,
                        );
                        vars.extend(extra);
                        debug!("Becoming: {cmd}");
                        exec(&cmd, vars);
                    }
                },
                _ => {}
            }
        });
        
        
        let reload_splitter = splitter.clone();
        new.register_interrupt_handler(Interrupt::Reload("".into()), move |state, interrupt| {
            let injector = state.injector();
            let injector= IndexedInjector::new(injector, ());
            let injector= SegmentedInjector::new(injector, reload_splitter.clone());
            
            match interrupt {
                Interrupt::Reload(template) => {
                    if let Some(t) = state.current_raw() {
                        let cmd = reload_formatter(t, template);
                        let vars = state.make_env_vars();
                        // let extra = env_vars!(
                        //     "FZF_PREVIEW_COMMAND" => preview_cmd,
                        // );
                        // vars.extend(extra);
                        debug!("Reloading: {cmd}");
                        if let Some(mut child) = spawn(&cmd, vars, Stdio::null(), Stdio::piped(), Stdio::null()) {
                            if let Some(stdout) = child.stdout.take() {
                                let _handle = if let Some(delim) = config.matcher.delimiter {
                                    tokio::spawn(async move {
                                        map_chunks::<true>(read_to_chunks(stdout, delim), |line| injector.push(line).map_err(|e| e.into()))
                                    })
                                } else {
                                    tokio::spawn(async move {
                                        map_reader_lines::<true>(stdout, |line| injector.push(line).map_err(|e| e.into()))
                                    })
                                };
                            } else {
                                error!("Failed to capture stdout");
                            }
                        }
                    }
                },
                _ => {}
            }
        });
        
        let misc = MiscData {
            formatter,
            splitter
        };
        
        (new, injector, misc)
    }
}

impl<T: PickerItem, S: Selection> Matchmaker<T, S> {
    pub fn new(worker: Worker<T>, identifier: fn(&T) -> (u32, S)) -> Self {
        let new = Matchmaker {
            matcher: Some(nucleo::Matcher::new(nucleo::Config::DEFAULT)),
            worker,
            bind_config: BindMap::new(),
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selection_set: SelectionSet::new(identifier),
            context: Arc::new(()),
            event_handlers: EventHandlers::new(),
            interrupt_handlers: InterruptHandlers::new(),
            previewer: None
        };
        
        new
    }
}

impl<T: PickerItem, S: Selection, C> Matchmaker<T, S, C>
{
    /// For library use:
    /// 1. create your worker (T -> Columns)
    /// 2. instantiate a matcher (i.e. globally with lazylock)
    /// 3. Determine your identifier
    /// 4. Call mm.pick_with_matcher(&mut matcher)
    pub fn new_raw(worker: Worker<T, C>, identifier: fn(&T) -> (u32, S), context: Arc<C>) -> Self {
        let new = Matchmaker {
            matcher: None,
            worker,
            bind_config: BindMap::new(),
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            selection_set: SelectionSet::new(identifier),
            context,
            event_handlers: EventHandlers::new(),
            interrupt_handlers: InterruptHandlers::new(),
            previewer: None
        };
        
        new
    }
    
    // todo: accept static matcher
    pub fn config_binds(&mut self, bind_config: BindMap) -> &mut Self {
        self.bind_config = bind_config;
        self
    }
    pub fn config_render(&mut self, render_config: RenderConfig) -> &mut Self {
        self.render_config = render_config;
        self
    }
    pub fn config_preview(&mut self, preview_config: PreviewerConfig) -> Sender<PreviewMessage> {
        let (previewer, tx) = Previewer::new(preview_config);
        self.previewer = Some(previewer);
        tx
    }
    pub fn config_tui(&mut self, tui_config: TerminalConfig) -> &mut Self {
        self.tui_config = tui_config;
        self
    }
    pub fn config_exit(&mut self, exit_config: ExitConfig) -> &mut Self {
        self.exit_config = exit_config;
        self
    }
    
    pub fn register_event_handler<F, I>(&mut self, events: I, handler: F)
    where
    F: Fn(EphemeralState<'_, T, S, C>, &Event) + Send + Sync + 'static,
    I: IntoIterator<Item = Event>,
    {
        let boxed = Box::new(handler);
        self.register_boxed_event_handler(events, boxed);
    }
    
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
    
    pub fn register_boxed_interrupt_handler(
        &mut self,
        variant: Interrupt,
        handler: DynamicMethod<T, S, C, Interrupt>,
    ) {
        self.interrupt_handlers.set(variant, handler);
    }
    
    
    // Some repetition until i figure out if its possible to somehow be generic over owned or static mut references (i.e. to LazyLock Matcher)
    pub async fn pick(mut self) -> Result<impl IntoIterator<Item = S>> {
        if let Some(matcher) = self.matcher.as_mut() {
            if self.exit_config.select_1 && self.worker.counts().0 == 1 {
                return Ok(self.selection_set.map_to_vec([self.worker.get_nth(0).unwrap()]));
            }
            
            let (render_tx, render_rx) = tokio::sync::mpsc::unbounded_channel();
            let (mut event_loop, controller_tx) = event::EventLoop::new(vec![render_tx.clone()], self.render_config.tick_rate());
            event_loop.binds(self.bind_config);
            
            let mut tui = tui::Tui::new(self.tui_config).expect("Failed to create TUI instance");
            tui.enter()?;
            
            tokio::spawn(async move {
                let _ = event_loop.run().await;
            });
            
            let view = if let Some(mut previewer) = self.previewer {
                previewer.connect_controller(controller_tx.clone());
                let view = previewer.view();
                tokio::spawn(async move {
                    let _ = previewer.run().await;
                });
                
                Some(view)
            } else {
                None
            };
            
            let (ui, picker, preview) = UI::new(self.render_config, matcher, self.worker, self.selection_set, view, &mut tui);
            
            render::render_loop(ui, picker, preview, tui, render_rx, controller_tx, self.context, (self.event_handlers, self.interrupt_handlers), self.exit_config).await
        } else {
            bail!("No matcher")
        }
    }
    
    pub async fn pick_with_matcher(self, matcher: &mut nucleo::Matcher) -> Result<impl IntoIterator<Item = S>> {
        if self.exit_config.select_1 && self.worker.counts().0 == 1 {
            return Ok(self.selection_set.map_to_vec([self.worker.get_nth(0).unwrap()]));
        }
        
        let (render_tx, render_rx) = tokio::sync::mpsc::unbounded_channel();
        let (mut event_loop, controller_tx) = event::EventLoop::new(vec![render_tx.clone()], self.render_config.tick_rate());
        event_loop.binds(self.bind_config);
        
        let mut tui = tui::Tui::new(self.tui_config).expect("Failed to create TUI instance");
        tui.enter()?;
        
        tokio::spawn(async move {
            let _ = event_loop.run().await;
        });
        
        let view = if let Some(mut previewer) = self.previewer {
            previewer.connect_controller(controller_tx.clone());
            let view = previewer.view();
            tokio::spawn(async move {
                let _ = previewer.run().await;
            });
            
            Some(view)
        } else {
            None
        };
        
        let (ui, picker, preview) = UI::new(self.render_config, matcher, self.worker, self.selection_set, view, &mut tui);
        
        render::render_loop(ui, picker, preview, tui, render_rx, controller_tx, self.context, (self.event_handlers, self.interrupt_handlers), self.exit_config).await
    }
}


// --------------------------------- BOILERPLATE -----------------------------------------------------------

impl<T: PickerItem + Debug, S: Selection + Debug, C: Debug> Debug for Matchmaker<T, S, C> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matchmaker")
        // omit `worker`
        .field("matcher", &self.matcher)
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