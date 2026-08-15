use std::fmt::{self, Debug, Formatter};

use cba::bath::PathExt;
use easy_ext::ext;

use crate::{
    MatchError, Result, SSS, Selector,
    action::{Action, ActionExt, Actions, NullActionExt},
    binds::BindMap,
    config::{ExitConfig, OverlayConfig, RenderConfig, TerminalConfig},
    event::{EventLoop, RenderSender},
    message::{Event, Interrupt},
    nucleo::Worker,
    preview::{Preview, previewer::Previewer},
    render::{self, BoxedHandler, DynamicMethod, EventHandlers, InterruptHandlers, MMState},
    tui,
    ui::{Overlay, OverlayUI, UI},
};

mod register;
pub use register::*;
pub mod config_mm;
/// A boxed closure that produces the `Vec<S>` result of a pick.
///
/// The closure receives a &mut [`MMState<T, D>`] and may inspect the selector
/// and current item to build the result.
pub type AcceptHook<T, D, S> =
    Box<dyn FnOnce(&mut MMState<'_, T, D>) -> Vec<S> + Send + Sync + 'static>;

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
    pub matcher_config: nucleo::Config,
    pub output: AcceptHook<T, D, S>,
    pub event_handlers: EventHandlers<T, D>,
    pub interrupt_handlers: InterruptHandlers<T, D>,
}

// ----------- MAIN -----------------------
impl<T: SSS, S, D: 'static> Matchmaker<T, S, D> {
    /// Construct a `Matchmaker` with default config and the given accept hook.
    pub fn new<F>(worker: Worker<T, D>, accept_hook: F) -> Self
    where
        F: FnOnce(&mut MMState<'_, T, D>) -> Vec<S> + Send + Sync + 'static,
    {
        Matchmaker {
            worker,
            render_config: RenderConfig::default(),
            tui_config: TerminalConfig::default(),
            exit_config: ExitConfig::default(),
            matcher_config: nucleo::Config::DEFAULT,
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
    /// Configure the scoring matcher
    ///
    /// A matcher is a scratch space that needs to be configured ber thread. This config will be used to configure the Matchers used by the worker (for sorting and filtering), and the Matcher used in the UI thread (to compute the highlight characters within each item).
    pub fn config_matcher(&mut self, matcher_config: nucleo::Config) -> &mut Self {
        self.matcher_config = matcher_config;
        self
    }
    /// Register a handler to listen on [`Event`]s
    pub fn register_event_handler<F>(&mut self, event: Event, handler: F)
    where
        F: Fn(&mut MMState<'_, T, D>, &Event) + 'static,
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
        F: FnMut(&mut MMState<'_, T, D>) + 'static,
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
        self.worker.find(&self.render_config.query.initial);
    }

    /// The main method of the Matchmaker. It starts listening for events and renders the TUI with ratatui. It successfully returns with all the selected items selected when the Accept action is received.
    pub async fn pick<A: ActionExt>(mut self, builder: PickOptions<T, D, A>) -> Result<Vec<S>> {
        let PickOptions {
            previewer,
            ext_handler,
            ext_aliaser,
            #[cfg(feature = "bracketed-paste")]
            paste_handler,
            overlay_config,
            initializer,
            ..
        } = builder;

        let has_event_loop_override = builder.event_loop.is_some();
        let mut event_loop = if let Some(e) = builder.event_loop {
            e
        } else if let Some(binds) = builder.binds {
            EventLoop::with_binds(binds)
                .with_tick_rate(self.render_config.ui.tick_rate)
                .with_mouse_events(self.render_config.ui.mouse_events)
                .with_scroll_debounce(self.render_config.ui.mouse_scroll_debounce_ms)
        } else {
            EventLoop::new()
        };

        // Headless test mode: there is no terminal to read events from, so
        // run the loop without a crossterm event stream unless the caller
        // supplied their own event loop.
        if !has_event_loop_override && self.tui_config.stream == tui::IoStream::Test {
            event_loop = event_loop.as_optional();
        }

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

        let mut tui: tui::Tui<Box<_>> =
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

        // The matcher config is applied to both halves of the pipeline: the
        // worker (ordering) and the process-wide scoring matcher (highlights).
        let matcher_config = self.matcher_config.clone();
        self.worker.set_config(matcher_config.clone());
        crate::matcher::set_matcher_config(matcher_config);

        let (ui, picker, footer, preview) = UI::new(
            self.render_config,
            self.worker,
            Selector::new(),
            preview,
            &mut tui,
        );

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
    Box<dyn FnMut(String, &MMState<'_, T, D>) -> String + Send + Sync + 'static>;

pub type ActionExtHandler<T, D, A> =
    Box<dyn FnMut(A, &mut MMState<'_, T, D>) + Send + Sync + 'static>;

pub type ActionAliaser<T, D, A> =
    Box<dyn FnMut(Action<A>, &mut MMState<'_, T, D>) -> Actions<A> + Send + Sync + 'static>;

pub type Initializer<T, D> = Box<dyn FnOnce(&mut MMState<'_, T, D>) + Send + Sync + 'static>;

/// Used to configure [`Matchmaker::pick`] with additional options.
pub struct PickOptions<T: SSS, D, A: ActionExt = NullActionExt> {
    event_loop: Option<EventLoop<A>>,
    binds: Option<BindMap<A>>,

    ext_handler: Option<ActionExtHandler<T, D, A>>,
    ext_aliaser: Option<ActionAliaser<T, D, A>>,
    #[cfg(feature = "bracketed-paste")]
    paste_handler: Option<PasteHandler<T, D>>,

    overlays: Vec<Box<dyn Overlay<A, T, D>>>,
    overlay_config: Option<OverlayConfig>,
    previewer: Option<Either<Preview, Previewer>>,

    // Initializing code, i.e. to setup state.
    initializer: Option<Initializer<T, D>>,
    pub channel: Option<(
        RenderSender<A>,
        tokio::sync::mpsc::UnboundedReceiver<crate::message::RenderCommand<A>>,
    )>,
}

impl<T: SSS, D, A: ActionExt> PickOptions<T, D, A> {
    pub fn new() -> Self {
        Self {
            event_loop: None,
            previewer: None,
            binds: None,
            ext_handler: None,
            ext_aliaser: None,
            #[cfg(feature = "bracketed-paste")]
            paste_handler: None,
            overlay_config: None,
            overlays: Vec::new(),
            channel: None,
            initializer: None,
        }
    }

    pub fn with_binds(binds: BindMap<A>) -> Self {
        let mut ret = Self::new();
        ret.binds = Some(binds);
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

    pub fn ext_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut(A, &mut MMState<'_, T, D>) + Send + Sync + 'static,
    {
        self.ext_handler = Some(Box::new(handler));
        self
    }

    pub fn ext_aliaser<F>(mut self, aliaser: F) -> Self
    where
        F: FnMut(Action<A>, &mut MMState<'_, T, D>) -> Actions<A> + Send + Sync + 'static,
    {
        self.ext_aliaser = Some(Box::new(aliaser));
        self
    }

    pub fn initializer<F>(mut self, handler: F) -> Self
    where
        F: FnOnce(&mut MMState<'_, T, D>) + Send + Sync + 'static,
    {
        self.initializer = Some(Box::new(handler));
        self
    }

    #[cfg(feature = "bracketed-paste")]
    pub fn paste_handler<F>(mut self, handler: F) -> Self
    where
        F: FnMut(String, &MMState<'_, T, D>) -> String + Send + Sync + 'static,
    {
        self.paste_handler = Some(Box::new(handler));
        self
    }

    pub fn overlay<O>(mut self, overlay: O) -> Self
    where
        O: Overlay<A, T, D> + 'static,
        D: 'static,
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

impl<T: SSS, D, A: ActionExt> Default for PickOptions<T, D, A> {
    fn default() -> Self {
        Self::new()
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
