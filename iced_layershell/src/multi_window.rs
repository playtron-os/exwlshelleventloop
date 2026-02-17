use crate::{
    DefaultStyle,
    actions::{IcedNewPopupSettings, LayershellCustomActionWithId, MenuDirection},
    ime_preedit::ImeState,
    multi_window::window_manager::WindowManager,
    settings::VirtualKeyboardSettings,
    user_interface::UserInterfaces,
};
use crate::{
    actions::LayershellCustomAction, clipboard::LayerShellClipboard, conversion, error::Error,
};
use crate::{
    event::{IcedLayerShellEvent, WindowEvent as LayerShellWindowEvent},
    proxy::IcedProxy,
    settings::Settings,
};
use futures::{FutureExt, StreamExt, future::LocalBoxFuture};
#[cfg(not(all(feature = "linux-theme-detection", target_os = "linux")))]
use iced_core::theme::Mode;
use iced_core::{
    Event as IcedEvent, auto_hide as iced_auto_hide, dismiss as iced_dismiss, theme,
    voice_mode as iced_voice_mode,
    window::{Event as IcedWindowEvent, Id as IcedId, RedrawRequest},
};
use iced_core::{Size, mouse, mouse::Cursor, time::Instant};
use iced_futures::{Executor, Runtime};
use iced_graphics::{Compositor, Shell, compositor};
use iced_program::Instance;
use iced_program::Program as IcedProgram;
use iced_runtime::Action;
use iced_runtime::user_interface;
use layershellev::{
    LayerShellEvent, NewPopUpSettings, RefreshRequest, ReturnData, WindowState, WindowWrapper,
    id::Id as LayerShellId,
    reexport::{
        wayland_client::{WlCompositor, WlRegion},
        zwp_virtual_keyboard_v1,
    },
};
use std::{
    borrow::Cow,
    collections::{HashMap, VecDeque},
    mem,
    os::fd::AsFd,
    sync::Arc,
    task::Poll,
    time::Duration,
};
use window_manager::Window;

mod state;
mod window_manager;

/// Convert layershellev voice mode event to iced voice mode event
fn convert_voice_mode_event(event: &layershellev::voice_mode::VoiceModeEvent) -> Option<IcedEvent> {
    use layershellev::voice_mode::{OrbState as LayerOrbState, VoiceModeEvent};

    let convert_orb_state = |state: LayerOrbState| -> iced_voice_mode::OrbState {
        match state {
            LayerOrbState::Hidden => iced_voice_mode::OrbState::Hidden,
            LayerOrbState::Floating => iced_voice_mode::OrbState::Floating,
            LayerOrbState::Attached => iced_voice_mode::OrbState::Attached,
            LayerOrbState::Frozen => iced_voice_mode::OrbState::Frozen,
            LayerOrbState::Transitioning => iced_voice_mode::OrbState::Transitioning,
            _ => iced_voice_mode::OrbState::Hidden, // Unknown state, default to hidden
        }
    };

    let iced_event = match event {
        VoiceModeEvent::Started { orb_state } => iced_voice_mode::Event::Started {
            orb_state: convert_orb_state(*orb_state),
        },
        VoiceModeEvent::Stopped => iced_voice_mode::Event::Stopped,
        VoiceModeEvent::Cancelled => iced_voice_mode::Event::Cancelled,
        VoiceModeEvent::OrbAttached {
            x,
            y,
            width,
            height,
        } => iced_voice_mode::Event::OrbAttached {
            x: *x,
            y: *y,
            width: *width,
            height: *height,
        },
        VoiceModeEvent::OrbDetached => iced_voice_mode::Event::OrbDetached,
        VoiceModeEvent::WillStop { serial } => iced_voice_mode::Event::WillStop { serial: *serial },
        VoiceModeEvent::FocusInput => iced_voice_mode::Event::FocusInput,
    };

    Some(IcedEvent::VoiceMode(iced_event))
}

type MultiRuntime<E, Message> = Runtime<E, IcedProxy<Action<Message>>, Action<Message>>;

// a dispatch loop, another is listen loop
pub fn run<P>(
    program: P,
    namespace: &str,
    settings: Settings,
    compositor_settings: iced_graphics::Settings,
) -> Result<(), Error>
where
    P: IcedProgram + 'static,
    P::Theme: DefaultStyle,
    P::Message: 'static + TryInto<LayershellCustomActionWithId, Error = P::Message>,
{
    use futures::task;
    use layershellev::calloop::channel::channel;
    let (message_sender, message_receiver) = channel::<Action<P::Message>>();

    let boot_span = iced_debug::boot();
    let proxy = IcedProxy::new(message_sender);

    #[cfg(feature = "debug")]
    {
        let proxy = proxy.clone();

        iced_debug::on_hotpatch(move || {
            proxy.send_action(Action::Reload);
        });
    }

    let proxy_back = proxy.clone();
    let mut runtime: MultiRuntime<P::Executor, P::Message> = {
        let executor = P::Executor::new().map_err(Error::ExecutorCreationFailed)?;

        Runtime::new(executor, proxy)
    };

    let (application, task) = runtime.enter(|| Instance::new(program));

    if let Some(stream) = iced_runtime::task::into_stream(task) {
        runtime.run(stream);
    }

    runtime.track(iced_futures::subscription::into_recipes(
        runtime.enter(|| application.subscription().map(Action::Output)),
    ));

    let ev: WindowState<iced_core::window::Id> = layershellev::WindowState::new(namespace)
        .with_start_mode(settings.layer_settings.start_mode)
        .with_use_display_handle(true)
        .with_events_transparent(settings.layer_settings.events_transparent)
        .with_blur(settings.layer_settings.blur)
        .with_shadow(settings.layer_settings.shadow)
        .with_home_only(settings.layer_settings.home_only)
        .with_hide_on_home(settings.layer_settings.hide_on_home)
        .with_voice_mode(settings.layer_settings.voice_mode);

    #[cfg(feature = "foreign-toplevel")]
    let ev = ev.with_foreign_toplevel(settings.layer_settings.foreign_toplevel);

    let ev = ev
        .with_option_size(settings.layer_settings.size)
        .with_layer(settings.layer_settings.layer)
        .with_anchor(settings.layer_settings.anchor)
        .with_exclusive_zone(settings.layer_settings.exclusive_zone)
        .with_margin(settings.layer_settings.margin)
        .with_keyboard_interacivity(settings.layer_settings.keyboard_interactivity)
        .with_connection(settings.with_connection);

    // Apply corner radius if set
    let ev = if let Some(radii) = settings.layer_settings.corner_radius {
        ev.with_corner_radius(radii)
    } else {
        ev
    };

    let ev = ev.build().expect("Cannot create layershell");

    #[cfg(all(feature = "linux-theme-detection", target_os = "linux"))]
    let system_theme = {
        let to_mode = |color_scheme| match color_scheme {
            mundy::ColorScheme::NoPreference => theme::Mode::None,
            mundy::ColorScheme::Light => theme::Mode::Light,
            mundy::ColorScheme::Dark => theme::Mode::Dark,
        };

        runtime.run(
            mundy::Preferences::stream(mundy::Interest::ColorScheme)
                .map(move |preferences| {
                    Action::System(iced_runtime::system::Action::NotifyTheme(to_mode(
                        preferences.color_scheme,
                    )))
                })
                .boxed(),
        );

        runtime
            .enter(|| {
                mundy::Preferences::once_blocking(
                    mundy::Interest::ColorScheme,
                    core::time::Duration::from_millis(200),
                )
            })
            .map(|preferences| to_mode(preferences.color_scheme))
            .unwrap_or_default()
    };

    #[cfg(not(all(feature = "linux-theme-detection", target_os = "linux")))]
    let system_theme = Mode::default();

    let context = Context::<
        P,
        <P as iced_program::Program>::Executor,
        <P::Renderer as iced_graphics::compositor::Default>::Compositor,
    >::new(
        application,
        compositor_settings,
        runtime,
        settings.fonts,
        system_theme,
        proxy_back,
    );
    let mut context_state = ContextState::Context(context);
    boot_span.finish();

    let mut waiting_layer_shell_events = VecDeque::new();
    let mut task_context = task::Context::from_waker(task::noop_waker_ref());

    let _ = ev.running_with_proxy(message_receiver, move |event, ev, layer_shell_id| {
        let mut def_returndata = ReturnData::None;
        match event {
            LayerShellEvent::InitRequest => {
                def_returndata = ReturnData::RequestBind;
            }
            LayerShellEvent::BindProvide(globals, qh) => {
                let wl_compositor = globals
                    .bind::<WlCompositor, _, _>(qh, 1..=1, ())
                    .expect("could not bind wl_compositor");
                waiting_layer_shell_events.push_back((
                    None,
                    IcedLayerShellEvent::UpdateInputRegion(wl_compositor.create_region(qh, ())),
                ));

                if let Some(virtual_keyboard_setting) = settings.virtual_keyboard_support.as_ref() {
                    let virtual_keyboard_manager = globals
                        .bind::<zwp_virtual_keyboard_v1::ZwpVirtualKeyboardManagerV1, _, _>(
                            qh,
                            1..=1,
                            (),
                        )
                        .expect("no support virtual_keyboard");
                    let VirtualKeyboardSettings {
                        file,
                        keymap_size,
                        keymap_format,
                    } = virtual_keyboard_setting;
                    let seat = ev.get_seat();
                    let virtual_keyboard_in =
                        virtual_keyboard_manager.create_virtual_keyboard(seat, qh, ());
                    virtual_keyboard_in.keymap((*keymap_format).into(), file.as_fd(), *keymap_size);
                    ev.set_virtual_keyboard(virtual_keyboard_in);
                }
            }
            LayerShellEvent::RequestMessages(message) => {
                waiting_layer_shell_events.push_back((
                    layer_shell_id,
                    IcedLayerShellEvent::Window(LayerShellWindowEvent::from(message)),
                ));
            }
            LayerShellEvent::UserEvent(event) => {
                waiting_layer_shell_events
                    .push_back((layer_shell_id, IcedLayerShellEvent::UserAction(event)));
            }
            LayerShellEvent::NormalDispatch => {
                waiting_layer_shell_events
                    .push_back((layer_shell_id, IcedLayerShellEvent::NormalDispatch));
            }
            _ => {}
        }
        loop {
            let mut need_continue = false;
            context_state = match std::mem::replace(&mut context_state, ContextState::None) {
                ContextState::None => unreachable!("context state is taken but not returned"),
                ContextState::Future(mut future) => {
                    tracing::debug!("poll context future");
                    match future.as_mut().poll(&mut task_context) {
                        Poll::Ready(context) => {
                            tracing::debug!("context future is ready");
                            // context is ready, continue to run.
                            need_continue = true;
                            ContextState::Context(context)
                        }
                        Poll::Pending => ContextState::Future(future),
                    }
                }
                ContextState::Context(context) => {
                    if let Some((layer_shell_id, layer_shell_event)) =
                        waiting_layer_shell_events.pop_front()
                    {
                        need_continue = true;
                        let (context_state, waiting_layer_shell_event) =
                            context.handle_event(ev, layer_shell_id, layer_shell_event);
                        if let Some(waiting_layer_shell_event) = waiting_layer_shell_event {
                            waiting_layer_shell_events
                                .push_front((layer_shell_id, waiting_layer_shell_event));
                        }
                        context_state
                    } else {
                        ContextState::Context(context)
                    }
                }
            };
            if !need_continue {
                break;
            }
        }
        def_returndata
    });
    Ok(())
}

enum ContextState<Context> {
    None,
    Context(Context),
    Future(LocalBoxFuture<'static, Context>),
}

struct Context<P, E, C>
where
    P: IcedProgram + 'static,
    C: Compositor<Renderer = P::Renderer> + 'static,
    E: Executor + 'static,
    P::Theme: DefaultStyle,
    P::Message: 'static,
{
    compositor_settings: iced_graphics::Settings,
    runtime: MultiRuntime<E, P::Message>,
    system_theme: iced_core::theme::Mode,
    fonts: Vec<Cow<'static, [u8]>>,
    compositor: Option<C>,
    window_manager: WindowManager<P, C>,
    cached_layer_dimensions: HashMap<IcedId, (Size<u32>, f32)>,
    /// Windows that need auto-sizing after first render (measure phase)
    auto_size_pending: std::collections::HashSet<IcedId>,
    /// Windows hidden until auto-size resize completes - maps to expected (width, height) and frame count
    auto_size_hidden: HashMap<IcedId, (u32, u32, u8)>,
    /// Windows with continuous auto-sizing enabled (resize when content changes)
    auto_size_enabled: std::collections::HashSet<IcedId>,
    /// Track last content size for auto_size windows to detect changes
    auto_size_last_content: HashMap<IcedId, (u32, u32)>,
    /// Maximum size for auto_size windows (from original LayerShellSettings.size)
    auto_size_max: HashMap<IcedId, (u32, u32)>,
    clipboard: LayerShellClipboard,
    wl_input_region: Option<WlRegion>,
    user_interfaces: UserInterfaces<P>,
    waiting_layer_shell_actions: Vec<(Option<IcedId>, LayershellCustomAction)>,
    iced_events: Vec<(IcedId, IcedEvent)>,
    messages: Vec<P::Message>,
    proxy: IcedProxy<Action<P::Message>>,
}

impl<P, E, C> Context<P, E, C>
where
    P: IcedProgram + 'static,
    C: Compositor<Renderer = P::Renderer> + 'static,
    E: Executor + 'static,
    P::Theme: DefaultStyle,
    P::Message: 'static + TryInto<LayershellCustomActionWithId, Error = P::Message>,
{
    pub fn new(
        application: Instance<P>,
        compositor_settings: iced_graphics::Settings,
        runtime: MultiRuntime<E, P::Message>,
        fonts: Vec<Cow<'static, [u8]>>,
        system_theme: iced_core::theme::Mode,
        proxy: IcedProxy<Action<P::Message>>,
    ) -> Self {
        Self {
            compositor_settings,
            runtime,
            system_theme,
            fonts,
            compositor: Default::default(),
            window_manager: WindowManager::new(),
            cached_layer_dimensions: HashMap::new(),
            auto_size_pending: std::collections::HashSet::new(),
            auto_size_hidden: HashMap::new(),
            auto_size_enabled: std::collections::HashSet::new(),
            auto_size_last_content: HashMap::new(),
            auto_size_max: HashMap::new(),
            clipboard: LayerShellClipboard::unconnected(),
            wl_input_region: Default::default(),
            user_interfaces: UserInterfaces::new(application),
            waiting_layer_shell_actions: Default::default(),
            iced_events: Default::default(),
            messages: Default::default(),
            proxy,
        }
    }

    async fn create_compositor(mut self, window: Arc<WindowWrapper>) -> Self {
        let shell = Shell::new(self.proxy.clone());
        let mut new_compositor = C::new(
            self.compositor_settings,
            window.clone(),
            window.clone(),
            shell,
        )
        .await
        .expect("Cannot create compositer");
        for font in self.fonts.clone() {
            new_compositor.load_font(font);
        }
        self.compositor = Some(new_compositor);
        self.clipboard = LayerShellClipboard::connect(&window);
        self
    }

    fn remove_compositor(&mut self) {
        self.compositor = None;
        self.clipboard = LayerShellClipboard::unconnected();
    }

    fn handle_event(
        mut self,
        ev: &mut WindowState<IcedId>,
        layer_shell_id: Option<LayerShellId>,
        layer_shell_event: IcedLayerShellEvent<P::Message>,
    ) -> (ContextState<Self>, Option<IcedLayerShellEvent<P::Message>>) {
        tracing::trace!(
            "Handle layer shell event, layer_shell_id: {:?},  waiting actions: {}, messages: {}",
            layer_shell_id,
            self.waiting_layer_shell_actions.len(),
            self.messages.len(),
        );
        if let IcedLayerShellEvent::Window(LayerShellWindowEvent::Refresh) = layer_shell_event
            && self.compositor.is_none()
        {
            let Some(layer_shell_window) = layer_shell_id.and_then(|lid| ev.get_unit_with_id(lid))
            else {
                tracing::error!("layer shell window not found: {:?}", layer_shell_id);
                return (ContextState::Context(self), None);
            };
            tracing::debug!("creating compositor");
            let context_state = ContextState::Future(
                self.create_compositor(Arc::new(layer_shell_window.gen_wrapper()))
                    .boxed_local(),
            );
            return (context_state, Some(layer_shell_event));
        }

        match layer_shell_event {
            IcedLayerShellEvent::UpdateInputRegion(region) => self.wl_input_region = Some(region),
            IcedLayerShellEvent::Window(LayerShellWindowEvent::Refresh) => {
                self.handle_refresh_event(ev, layer_shell_id)
            }
            IcedLayerShellEvent::Window(LayerShellWindowEvent::Closed) => {
                self.handle_closed_event(ev, layer_shell_id)
            }
            IcedLayerShellEvent::Window(window_event) => {
                // Voice mode events need to trigger a refresh to process the subscription message
                let needs_refresh = self.handle_window_event(layer_shell_id, window_event);
                if needs_refresh {
                    // Process any iced_events that were added immediately (e.g. voice mode events)
                    // This ensures the UI updates without waiting for the next NormalDispatch
                    if !self.iced_events.is_empty() {
                        tracing::debug!(
                            "handle_event: processing {} iced_events immediately after window event",
                            self.iced_events.len()
                        );
                        self.handle_normal_dispatch(ev);
                    }
                    ev.request_refresh_all(RefreshRequest::NextFrame);
                }
            }
            IcedLayerShellEvent::UserAction(user_action) => {
                self.handle_user_action(ev, user_action)
            }
            IcedLayerShellEvent::NormalDispatch => self.handle_normal_dispatch(ev),
        }

        // at each interaction try to resolve those waiting actions.
        let mut waiting_layer_shell_actions = Vec::new();
        mem::swap(
            &mut self.waiting_layer_shell_actions,
            &mut waiting_layer_shell_actions,
        );
        for (iced_id, action) in waiting_layer_shell_actions {
            self.handle_layer_shell_action(ev, iced_id, action);
        }

        (ContextState::Context(self), None)
    }

    fn handle_refresh_event(
        &mut self,
        ev: &mut WindowState<IcedId>,
        layer_shell_id: Option<LayerShellId>,
    ) {
        let Some(layer_shell_window) = layer_shell_id.and_then(|lid| ev.get_unit_with_id(lid))
        else {
            return;
        };
        let (width, height) = layer_shell_window.get_size();
        let scale_float = layer_shell_window.scale_float();
        // events may not be handled after RequestRefreshWithWrapper in the same
        // interaction, we dispatched them immediately.
        let mut events = Vec::new();
        let (iced_id, window) = if let Some((iced_id, window)) =
            self.window_manager.get_mut_alias(layer_shell_window.id())
        {
            let window_size = window.state.window_size();

            if window_size.width != width
                || window_size.height != height
                || window.state.wayland_scale_factor() != scale_float
            {
                let layout_span = iced_debug::layout(iced_id);
                window.state.update_view_port(width, height, scale_float);
                if let Some(ui) = self.user_interfaces.ui_mut(&iced_id) {
                    ui.relayout(window.state.viewport().logical_size(), &mut window.renderer);
                }
                layout_span.finish();
                events.push(IcedEvent::Window(IcedWindowEvent::Resized(
                    window.state.window_size_f32(),
                )));
            }
            (iced_id, window)
        } else {
            let wrapper = Arc::new(layer_shell_window.gen_wrapper());
            let iced_id = layer_shell_window
                .get_binding()
                .copied()
                .unwrap_or_else(IcedId::unique);

            let window = self.window_manager.insert(
                iced_id,
                (width, height),
                scale_float,
                wrapper,
                self.user_interfaces.application(),
                self.compositor
                    .as_mut()
                    .expect("It should have been created"),
                self.system_theme,
            );

            self.user_interfaces.build(
                iced_id,
                user_interface::Cache::default(),
                &mut window.renderer,
                window.state.viewport().logical_size(),
            );

            events.push(IcedEvent::Window(IcedWindowEvent::Opened {
                position: None,
                size: window.state.window_size_f32(),
            }));
            (iced_id, window)
        };

        let compositor = self
            .compositor
            .as_mut()
            .expect("The compositor should have been created");

        let mut ui = self
            .user_interfaces
            .ui_mut(&iced_id)
            .expect("Get User interface");

        let cursor = if ev.is_mouse_surface(layer_shell_window.id()) {
            window.state.cursor()
        } else {
            Cursor::Unavailable
        };

        events.push(IcedEvent::Window(IcedWindowEvent::RedrawRequested(
            Instant::now(),
        )));

        let draw_span = iced_debug::draw(iced_id);
        let (ui_state, statuses) = ui.update(
            &events,
            cursor,
            &mut window.renderer,
            &mut self.clipboard,
            &mut self.messages,
        );

        let physical_size = window.state.viewport().physical_size();

        if self
            .cached_layer_dimensions
            .get(&iced_id)
            .is_none_or(|(size, scale)| {
                *size != physical_size || *scale != window.state.viewport().scale_factor()
            })
        {
            self.cached_layer_dimensions.insert(
                iced_id,
                (physical_size, window.state.viewport().scale_factor()),
            );

            compositor.configure_surface(
                &mut window.surface,
                physical_size.width,
                physical_size.height,
            );
        }

        for (idx, event) in events.into_iter().enumerate() {
            let status = statuses
                .get(idx)
                .cloned()
                .unwrap_or(iced_core::event::Status::Ignored);
            self.runtime
                .broadcast(iced_futures::subscription::Event::Interaction {
                    window: iced_id,
                    event,
                    status,
                });
        }

        // For auto_size windows, check if content might want to grow BEFORE draw
        // We do unbounded measurement here, then relayout back for proper draw
        let dynamic_resize_target: Option<(u32, u32)> = if self.auto_size_enabled.contains(&iced_id)
            && !self.auto_size_pending.contains(&iced_id)
        {
            let window_size = window.state.window_size();
            let content_size = ui.content_size();
            let content_height = content_size.height.ceil() as u32;

            // Get max bounds
            let (max_width, max_height) = self
                .auto_size_max
                .get(&iced_id)
                .copied()
                .unwrap_or((10000, 10000));

            // If content fills window height, it might want to be taller
            let content_fills_window = content_height >= window_size.height.saturating_sub(1);

            if content_fills_window && window_size.height < max_height {
                // Do measurement at max bounds
                let max_bounds = Size::new(max_width as f32, max_height as f32);
                ui = ui.relayout(max_bounds, &mut window.renderer);
                let true_content_size = ui.content_size();
                let measured_width = (true_content_size.width.ceil() as u32).min(max_width);
                let measured_height = (true_content_size.height.ceil() as u32).min(max_height);

                // Relayout back to current window size for proper draw
                let current_bounds = Size::new(window_size.width as f32, window_size.height as f32);
                ui = ui.relayout(current_bounds, &mut window.renderer);

                // Check if we need to resize
                let last_size = self.auto_size_last_content.get(&iced_id).copied();
                let needs_resize = last_size
                    .map(|(_, last_h)| measured_height != last_h)
                    .unwrap_or(true);

                if needs_resize && measured_height > 0 && measured_width > 0 {
                    tracing::debug!(
                        "Auto-size unbounded measure for {:?}: content fills window, true size=({}, {}), max=({}, {})",
                        iced_id,
                        measured_width,
                        measured_height,
                        max_width,
                        max_height
                    );
                    Some((measured_width, measured_height))
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        // Check if this window needs initial auto-sizing (first render measurement)
        // Do this BEFORE draw so we can request resize before presenting
        let initial_auto_size_target: Option<(u32, u32)> =
            if self.auto_size_pending.remove(&iced_id) {
                let window_size = window.state.window_size_f32();

                // Get max bounds from stored settings (original size acts as maximum)
                let (max_width, max_height) = self
                    .auto_size_max
                    .get(&iced_id)
                    .copied()
                    .unwrap_or((10000, 10000));

                // Do layout at max size to measure true content size
                let max_bounds = Size::new(max_width as f32, max_height as f32);
                ui = ui.relayout(max_bounds, &mut window.renderer);
                let content_size = ui.content_size();

                tracing::debug!(
                    "Auto-size INITIAL check for {:?}: content={:?}, max=({}, {})",
                    iced_id,
                    content_size,
                    max_width,
                    max_height
                );

                // Resize to measured content size (clamped to max bounds)
                let new_width = (content_size.width.ceil() as u32).min(max_width);
                let new_height = (content_size.height.ceil() as u32).min(max_height);

                // Skip if size would be zero (invalid for layer shell)
                if new_width == 0 || new_height == 0 {
                    tracing::trace!(
                        "Auto-size: skipping zero size ({}, {})",
                        new_width,
                        new_height
                    );
                    // Relayout back to window size for draw
                    let current_bounds = Size::new(window_size.width, window_size.height);
                    ui = ui.relayout(current_bounds, &mut window.renderer);
                    None
                } else {
                    tracing::debug!(
                        "Auto-sizing window {:?} from ({}, {}) to ({}, {})",
                        iced_id,
                        window_size.width as u32,
                        window_size.height as u32,
                        new_width,
                        new_height
                    );

                    // Apply size change BEFORE draw
                    layer_shell_window.set_size((new_width, new_height));

                    // Relayout to the new target size for proper draw
                    let target_bounds = Size::new(new_width as f32, new_height as f32);
                    ui = ui.relayout(target_bounds, &mut window.renderer);

                    // Store initial content size for change detection
                    self.auto_size_last_content
                        .insert(iced_id, (new_width, new_height));

                    Some((new_width, new_height))
                }
            } else {
                None
            };

        // If we just requested initial auto-size, skip present until resize is confirmed
        // The issue is that window.state still has old size, so draw will use wrong viewport
        let skip_present = if let Some((target_w, target_h)) = initial_auto_size_target {
            let current_size = window.state.window_size();
            tracing::debug!(
                "Auto-size: requested resize to ({}, {}), current window state is ({}, {})",
                target_w,
                target_h,
                current_size.width,
                current_size.height
            );
            // Window state hasn't updated yet - wait for configure event
            self.auto_size_hidden
                .insert(iced_id, (target_w, target_h, 0));
            true
        } else if let Some((target_w, target_h, frame_count)) =
            self.auto_size_hidden.get_mut(&iced_id)
        {
            let current_size = window.state.window_size();
            if current_size.width == *target_w && current_size.height == *target_h {
                // Resize confirmed, remove from hidden
                tracing::debug!(
                    "Auto-size resize CONFIRMED for {:?}: size ({}, {})",
                    iced_id,
                    target_w,
                    target_h
                );
                self.auto_size_hidden.remove(&iced_id);
                false
            } else {
                *frame_count += 1;
                tracing::info!(
                    "Auto-size waiting for resize {:?}: frame {}, current {:?}, target ({}, {})",
                    iced_id,
                    frame_count,
                    current_size,
                    target_w,
                    target_h
                );
                if *frame_count > 5 {
                    // Timeout - give up waiting
                    tracing::warn!(
                        "Auto-size TIMEOUT for {:?}: showing at size {:?}, wanted ({}, {})",
                        iced_id,
                        current_size,
                        target_w,
                        target_h
                    );
                    self.auto_size_hidden.remove(&iced_id);
                    false
                } else {
                    true
                }
            }
        } else {
            false
        };

        ui.draw(
            &mut window.renderer,
            window.state.theme(),
            &iced_core::renderer::Style {
                text_color: window.state.text_color(),
            },
            cursor,
        );

        // Check for dynamic content size changes on auto_size windows
        if self.auto_size_enabled.contains(&iced_id) && initial_auto_size_target.is_none() {
            // Check for content size changes on auto_size windows (dynamic resize)
            // Use pre-measured target if available (for content that fills window)
            // Otherwise use current content size (for content shrinking)
            let window_size = window.state.window_size();
            let content_size = ui.content_size();
            let (new_width, new_height) = if let Some((w, h)) = dynamic_resize_target {
                (w, h)
            } else {
                (
                    content_size.width.ceil() as u32,
                    content_size.height.ceil() as u32,
                )
            };

            let last_size = self.auto_size_last_content.get(&iced_id).copied();

            tracing::trace!(
                "Auto-size dynamic check for {:?}: content=({}, {}), window=({}, {}), last={:?}",
                iced_id,
                new_width,
                new_height,
                window_size.width,
                window_size.height,
                last_size
            );

            let needs_resize = last_size
                .map(|(last_w, last_h)| last_w != new_width || last_h != new_height)
                .unwrap_or(true);

            if needs_resize {
                // Skip if size would be zero (invalid for layer shell)
                if new_width == 0 || new_height == 0 {
                    tracing::trace!(
                        "Auto-size dynamic: skipping zero size ({}, {})",
                        new_width,
                        new_height
                    );
                } else {
                    tracing::debug!(
                        "Auto-size content changed for {:?}: new size ({}, {})",
                        iced_id,
                        new_width,
                        new_height
                    );

                    // Update tracked size
                    self.auto_size_last_content
                        .insert(iced_id, (new_width, new_height));

                    // Apply size change
                    layer_shell_window.set_size((new_width, new_height));
                }
            }
        }

        draw_span.finish();

        // get layer_shell_id so that layer_shell_window can be drop, and ev can be borrow mut
        let layer_shell_id = layer_shell_window.id();

        Self::handle_ui_state(ev, window, ui_state, false, true);

        window.draw_preedit();

        // Use fully transparent background while waiting for auto-size resize
        // This prevents the "flash" of the large popup before resize completes
        let background_color = if skip_present {
            tracing::trace!(
                "Using transparent background for {:?} (waiting for auto-size)",
                iced_id
            );
            iced_core::Color::TRANSPARENT
        } else {
            window.state.background_color()
        };

        let present_span = iced_debug::present(iced_id);
        match compositor.present(
            &mut window.renderer,
            &mut window.surface,
            window.state.viewport(),
            background_color,
            || {
                ev.request_next_present(layer_shell_id);
            },
        ) {
            Ok(()) => {
                present_span.finish();
            }
            Err(error) => match error {
                compositor::SurfaceError::OutOfMemory => {
                    panic!("{error:?}");
                }
                _ => {
                    tracing::error!("Error {error:?} when presenting surface.");
                }
            },
        }
    }

    fn handle_closed_event(
        &mut self,
        ev: &mut WindowState<IcedId>,
        layer_shell_id: Option<LayerShellId>,
    ) {
        let Some(iced_id) = layer_shell_id
            .and_then(|lid| ev.get_unit_with_id(lid))
            .and_then(|layer_shell_window| layer_shell_window.get_binding().copied())
        else {
            return;
        };
        self.cached_layer_dimensions.remove(&iced_id);
        self.auto_size_pending.remove(&iced_id);
        self.auto_size_hidden.remove(&iced_id);
        self.auto_size_enabled.remove(&iced_id);
        self.auto_size_last_content.remove(&iced_id);
        self.auto_size_max.remove(&iced_id);
        self.window_manager.remove(iced_id);
        self.user_interfaces.remove(&iced_id);
        self.runtime
            .broadcast(iced_futures::subscription::Event::Interaction {
                window: iced_id,
                event: IcedEvent::Window(IcedWindowEvent::Closed),
                status: iced_core::event::Status::Ignored,
            });
        // if now there is no windows now, then break the compositor, and unlink the clipboard
        if self.window_manager.is_empty() {
            self.remove_compositor();
        }
    }

    /// Handle window events. Returns true if a refresh should be requested
    /// (for events that go through subscription channels like voice mode).
    fn handle_window_event(
        &mut self,
        layer_shell_id: Option<LayerShellId>,
        event: LayerShellWindowEvent,
    ) -> bool {
        // Handle foreign toplevel events specially - they go through a subscription channel
        #[cfg(feature = "foreign-toplevel")]
        if let LayerShellWindowEvent::ForeignToplevel(ref toplevel_event) = event {
            crate::event::send_foreign_toplevel_event(toplevel_event.clone());
            return true; // Request refresh to process subscription message
        }

        // Handle voice mode events - convert to iced event and push to iced_events for immediate processing
        if let LayerShellWindowEvent::VoiceMode(ref voice_event) = event {
            tracing::debug!(
                "handle_window_event: received VoiceMode event: {:?}",
                voice_event
            );
            if let Some(iced_event) = convert_voice_mode_event(voice_event) {
                // Push to first window (voice mode events are global, not per-window)
                if let Some((iced_id, _)) = self.window_manager.iter_mut().next() {
                    tracing::debug!(
                        "handle_window_event: pushing iced voice event to window {:?}, iced_events count: {}",
                        iced_id,
                        self.iced_events.len() + 1
                    );
                    self.iced_events.push((iced_id, iced_event));
                } else {
                    tracing::warn!(
                        "handle_window_event: no windows in window_manager to receive voice event"
                    );
                }
            } else {
                tracing::warn!("handle_window_event: failed to convert voice event");
            }
            tracing::debug!("handle_window_event: requesting refresh for voice event");
            return true; // Request refresh
        }

        // Handle dismiss events - convert to iced event for immediate delivery
        if let LayerShellWindowEvent::DismissRequested = event {
            tracing::debug!("handle_window_event: received DismissRequested event");
            let iced_event = IcedEvent::Dismiss(iced_dismiss::Event::Requested);
            if let Some((iced_id, _)) = self.window_manager.iter_mut().next() {
                self.iced_events.push((iced_id, iced_event));
            }
            return true;
        }

        // Handle auto-hide visibility events - convert to iced event for immediate delivery
        if let LayerShellWindowEvent::AutoHideVisibilityChanged { visible } = event {
            tracing::debug!(
                "handle_window_event: received AutoHideVisibilityChanged: visible={}",
                visible
            );
            let iced_event = IcedEvent::AutoHide(if visible {
                iced_auto_hide::Event::Shown
            } else {
                iced_auto_hide::Event::Hidden
            });
            if let Some((iced_id, _)) = self.window_manager.iter_mut().next() {
                self.iced_events.push((iced_id, iced_event));
            }
            return true;
        }

        let id_and_window = if let Some(layer_shell_id) = layer_shell_id {
            self.window_manager.get_mut_alias(layer_shell_id)
        } else {
            self.window_manager.iter_mut().next()
        };
        let Some((iced_id, window)) = id_and_window else {
            return false;
        };
        // In previous implementation, event without layer_shell_id won't call `update` here, but
        // will broadcast to the application. I'm not sure why, but I think it is
        // reasonable to call `update` here.
        window
            .state
            .update(&event, self.user_interfaces.application());

        // Reset cached mouse_interaction on cursor leave/enter so the next
        // UI update will re-send the cursor shape to the compositor.
        // Per the Wayland protocol, after every wl_pointer::enter the cursor
        // shape is undefined and must be re-set by the client.  Without this
        // reset the cached value may match the UI value, causing the shape
        // request to be skipped (e.g. when a new layer-shell surface is
        // created and the compositor re-enters the panel surface).
        if matches!(
            event,
            LayerShellWindowEvent::CursorLeft | LayerShellWindowEvent::CursorEnter { .. }
        ) {
            tracing::trace!(
                "Cursor {:?} — resetting cached mouse_interaction for window {:?}",
                if matches!(event, LayerShellWindowEvent::CursorLeft) {
                    "left"
                } else {
                    "enter"
                },
                iced_id,
            );
            window.mouse_interaction = mouse::Interaction::Idle;
        }

        if let Some(event) = conversion::window_event(
            &event,
            window.state.application_scale_factor(),
            window.state.modifiers(),
        ) {
            self.iced_events.push((iced_id, event));
        }
        false
    }

    fn handle_user_action(&mut self, ev: &mut WindowState<IcedId>, action: Action<P::Message>) {
        let mut should_exit = false;
        run_action(
            &mut self.user_interfaces,
            &mut self.compositor,
            action,
            &mut self.messages,
            &mut self.clipboard,
            &mut self.waiting_layer_shell_actions,
            &mut should_exit,
            &mut self.window_manager,
            &mut self.system_theme,
            &mut self.runtime,
            ev,
        );
        if should_exit {
            ev.append_return_data(ReturnData::RequestExit);
        }
    }

    fn handle_layer_shell_action(
        &mut self,
        ev: &mut WindowState<IcedId>,
        mut iced_id: Option<IcedId>,
        action: LayershellCustomAction,
    ) {
        let layer_shell_window;
        macro_rules! ref_layer_shell_window {
            ($ev: ident, $iced_id: ident, $layer_shell_id: ident, $layer_shell_window: ident) => {
                if $iced_id.is_none() {
                    // Make application also works
                    if let Some(window) = self.window_manager.first() {
                        $iced_id = Some(window.iced_id);
                        $layer_shell_id = Some(window.id);
                    }
                    if $iced_id.is_none() {
                        tracing::error!(
                            "Here should be an id, it is a bug, please report an issue for us"
                        );
                        return;
                    }
                }
                if let Some(ls_window) =
                    $layer_shell_id.and_then(|layer_shell_id| $ev.get_unit_with_id(layer_shell_id))
                {
                    layer_shell_window = ls_window;
                } else {
                    return;
                }
            };
        }
        // check if window is ready
        let mut layer_shell_id = iced_id
            .and_then(|iced_id| self.window_manager.get(iced_id))
            .map(|window| window.id);
        if iced_id.is_some() && layer_shell_id.is_none() {
            // still waiting
            self.waiting_layer_shell_actions.push((iced_id, action));
            return;
        }
        match action {
            LayershellCustomAction::AnchorChange(anchor) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_anchor(anchor);
            }
            LayershellCustomAction::AnchorSizeChange(anchor, size) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_anchor_with_size(anchor, size);
            }
            LayershellCustomAction::LayerChange(layer) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_layer(layer);
            }
            LayershellCustomAction::MarginChange(margin) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_margin(margin);
            }
            LayershellCustomAction::SizeChange((width, height)) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_size((width, height));
                // Note: For auto-size windows, unhiding happens when we receive the resize event
                // with the correct size (in handle_refresh_event)
            }
            LayershellCustomAction::CornerRadiusChange(radii) => {
                // Extract surface in a block to drop the borrow before calling mutable method
                let surface = {
                    ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                    layer_shell_window.get_wlsurface().clone()
                };
                ev.set_corner_radius_for_surface(&surface, radii);
            }
            LayershellCustomAction::AutoHideChange {
                edge,
                edge_zone,
                mode,
            } => {
                // Extract surface in a block to drop the borrow before calling mutable method
                let surface = {
                    ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                    layer_shell_window.get_wlsurface().clone()
                };
                ev.set_auto_hide_for_surface(&surface, edge, edge_zone, mode);
            }
            LayershellCustomAction::AutoHideUnset => {
                // Extract surface in a block to drop the borrow before calling mutable method
                let surface = {
                    ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                    layer_shell_window.get_wlsurface().clone()
                };
                ev.unset_auto_hide_for_surface(&surface);
            }
            LayershellCustomAction::ExclusiveZoneChange(zone_size) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                layer_shell_window.set_exclusive_zone(zone_size);
            }
            LayershellCustomAction::SetInputRegion(set_region) => {
                ref_layer_shell_window!(ev, iced_id, layer_shell_id, layer_shell_window);
                let set_region = set_region.0;
                let Some(region) = &self.wl_input_region else {
                    tracing::warn!(
                        "wl_input_region is not set, ignore SetInputRegion, window_id: {:?}",
                        iced_id
                    );
                    return;
                };

                let window_size = layer_shell_window.get_size();
                let width: i32 = window_size.0.try_into().unwrap_or_default();
                let height: i32 = window_size.1.try_into().unwrap_or_default();

                region.subtract(0, 0, width, height);
                set_region(region);

                layer_shell_window
                    .get_wlsurface()
                    .set_input_region(self.wl_input_region.as_ref());
                layer_shell_window.get_wlsurface().commit();
            }
            LayershellCustomAction::VirtualKeyboardPressed { time, key } => {
                use layershellev::reexport::wayland_client::KeyState;
                let ky = ev.get_virtual_keyboard().unwrap();
                ky.key(time, key, KeyState::Pressed.into());
                ev.set_virtual_key_release(layershellev::VirtualKeyRelease {
                    delay: Duration::from_micros(100),
                    time,
                    key,
                });
            }
            LayershellCustomAction::NewLayerShell {
                mut settings,
                id: iced_id,
                ..
            } => {
                // Track this window for auto-sizing if enabled
                if settings.auto_size {
                    // Store the original size as max bounds for auto-sizing
                    let max_size = settings.size.unwrap_or((10000, 10000));
                    tracing::debug!(
                        "Auto-size enabled for window {:?}, max_size={:?}, forcing initial size to 1x1",
                        iced_id,
                        max_size
                    );
                    self.auto_size_pending.insert(iced_id);
                    self.auto_size_enabled.insert(iced_id);
                    self.auto_size_max.insert(iced_id, max_size);
                    // Force initial size to 1x1 to avoid visual flash before auto-size completes
                    // The surface will be resized to content size on first render
                    settings.size = Some((1, 1));
                }
                let layer_shell_id = layershellev::id::Id::unique();
                ev.append_return_data(ReturnData::NewLayerShell((
                    settings,
                    layer_shell_id,
                    Some(iced_id),
                )));
            }
            LayershellCustomAction::NewBaseWindow {
                settings,
                id: iced_id,
                ..
            } => {
                let layer_shell_id = layershellev::id::Id::unique();
                ev.append_return_data(ReturnData::NewXdgBase((
                    settings.into(),
                    layer_shell_id,
                    Some(iced_id),
                )));
            }
            LayershellCustomAction::RemoveWindow => {
                if let Some(layer_shell_id) = layer_shell_id {
                    ev.request_close(layer_shell_id)
                }
            }
            LayershellCustomAction::NewPopUp {
                settings: menusettings,
                id: iced_id,
            } => {
                let IcedNewPopupSettings { size, position } = menusettings;
                let Some(parent_layer_shell_id) = ev.current_surface_id() else {
                    return;
                };
                let popup_settings = NewPopUpSettings {
                    size,
                    position,
                    id: parent_layer_shell_id,
                };
                let layer_shell_id = layershellev::id::Id::unique();
                ev.append_return_data(ReturnData::NewPopUp((
                    popup_settings,
                    layer_shell_id,
                    Some(iced_id),
                )));
            }
            LayershellCustomAction::NewMenu {
                settings: menu_setting,
                id: iced_id,
            } => {
                let Some(parent_layer_shell_id) = ev.current_surface_id() else {
                    return;
                };
                let Some((_, window)) = self.window_manager.get_alias(parent_layer_shell_id) else {
                    return;
                };

                let Some(point) = window.state.mouse_position() else {
                    return;
                };

                let (x, mut y) = (point.x as i32, point.y as i32);
                if let MenuDirection::Up = menu_setting.direction {
                    y -= menu_setting.size.1 as i32;
                }
                let popup_settings = NewPopUpSettings {
                    size: menu_setting.size,
                    position: (x, y),
                    id: parent_layer_shell_id,
                };
                let layer_shell_id = layershellev::id::Id::unique();
                ev.append_return_data(ReturnData::NewPopUp((
                    popup_settings,
                    layer_shell_id,
                    Some(iced_id),
                )))
            }
            LayershellCustomAction::NewInputPanel {
                settings,
                id: iced_id,
            } => {
                let layer_shell_id = layershellev::id::Id::unique();
                ev.append_return_data(ReturnData::NewInputPanel((
                    settings,
                    layer_shell_id,
                    Some(iced_id),
                )));
            }
            LayershellCustomAction::ForgetLastOutput => {
                ev.forget_last_output();
            }
            LayershellCustomAction::HideWindow => {
                // Get the surface to hide
                let surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(surface) = surface {
                    ev.hide_surface(&surface);
                }
            }
            LayershellCustomAction::ShowWindow => {
                // Get the surface to show
                let surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(surface) = surface {
                    ev.show_surface(&surface);
                }
            }
            LayershellCustomAction::VisibilityModeChange(mode) => {
                // Get the surface first to avoid borrow conflict
                let surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(surface) = surface {
                    ev.set_visibility_mode_for_surface(&surface, mode);
                }
            }
            #[cfg(feature = "foreign-toplevel")]
            LayershellCustomAction::ToplevelAction(action) => {
                log::info!("Processing ToplevelAction: {:?}", action);
                let result = ev.execute_toplevel_action(action);
                log::info!("ToplevelAction result: {}", result);
            }
            LayershellCustomAction::SetVoiceAudioLevel(level) => {
                ev.send_voice_audio_level(level);
            }
            LayershellCustomAction::VoiceAckStop(serial, freeze) => {
                ev.voice_ack_stop(serial, freeze);
            }
            LayershellCustomAction::VoiceDismiss => {
                ev.voice_dismiss();
            }
            LayershellCustomAction::ArmDismiss => {
                // Get the surface to arm dismiss for
                let surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(surface) = surface {
                    ev.arm_dismiss(&surface);
                }
            }
            LayershellCustomAction::DisarmDismiss => {
                // Get the surface to disarm dismiss for
                let surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(surface) = surface {
                    ev.disarm_dismiss(&surface);
                }
            }
            LayershellCustomAction::AddMainSurfaceToDismissGroup => {
                // Get the popup surface
                let popup_surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                // Get the main (first) surface
                let main_surface = ev.main_window().get_wlsurface().clone();
                if let Some(popup_surface) = popup_surface {
                    ev.add_to_dismiss_group(&popup_surface, &main_surface);
                }
            }
            LayershellCustomAction::AddAllSurfacesToDismissGroup => {
                // Get the popup surface
                let popup_surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                if let Some(popup_surface) = popup_surface {
                    // Add every known window surface to the dismiss group
                    let all_surfaces: Vec<_> = ev
                        .windows()
                        .iter()
                        .map(|unit| unit.get_wlsurface().clone())
                        .collect();
                    for surface in all_surfaces {
                        ev.add_to_dismiss_group(&popup_surface, &surface);
                    }
                }
            }
            LayershellCustomAction::RemoveMainSurfaceFromDismissGroup => {
                // Get the popup surface
                let popup_surface = layer_shell_id.and_then(|id| {
                    ev.get_unit_with_id(id)
                        .map(|unit| unit.get_wlsurface().clone())
                });
                // Get the main (first) surface
                let main_surface = ev.main_window().get_wlsurface().clone();
                if let Some(popup_surface) = popup_surface {
                    ev.remove_from_dismiss_group(&popup_surface, &main_surface);
                }
            }
        }
    }

    fn handle_normal_dispatch(&mut self, ev: &mut WindowState<IcedId>) {
        if self.iced_events.is_empty() && self.messages.is_empty() {
            return;
        }

        tracing::debug!(
            "handle_normal_dispatch: iced_events={}, messages={}",
            self.iced_events.len(),
            self.messages.len()
        );

        let mut rebuilds = Vec::new();
        for (iced_id, window) in self.window_manager.iter_mut() {
            let interact_span = iced_debug::interact(iced_id);
            let mut window_events = vec![];

            self.iced_events.retain(|(window_id, event)| {
                if *window_id == iced_id {
                    window_events.push(event.clone());
                    false
                } else {
                    true
                }
            });

            if window_events.is_empty() && self.messages.is_empty() {
                continue;
            }

            let (ui_state, statuses) = self
                .user_interfaces
                .ui_mut(&iced_id)
                .expect("Get user interface")
                .update(
                    &window_events,
                    window.state.cursor(),
                    &mut window.renderer,
                    &mut self.clipboard,
                    &mut self.messages,
                );

            #[cfg(feature = "unconditional-rendering")]
            let unconditional_rendering = true;
            #[cfg(not(feature = "unconditional-rendering"))]
            let unconditional_rendering = false;
            if Self::handle_ui_state(ev, window, ui_state, unconditional_rendering, false) {
                rebuilds.push((iced_id, window));
            }

            for (event, status) in window_events.drain(..).zip(statuses.into_iter()) {
                self.runtime
                    .broadcast(iced_futures::subscription::Event::Interaction {
                        window: iced_id,
                        event,
                        status,
                    });
            }
            interact_span.finish();
        }

        if !self.messages.is_empty() {
            ev.request_refresh_all(RefreshRequest::NextFrame);
            let (caches, application) = self.user_interfaces.extract_all();

            // Update application
            update(application, &mut self.runtime, &mut self.messages);

            for (_, window) in self.window_manager.iter_mut() {
                window.state.synchronize(application);
            }
            iced_debug::theme_changed(|| {
                self.window_manager
                    .first()
                    .and_then(|window| theme::Base::palette(window.state.theme()))
            });

            for (iced_id, cache) in caches {
                let Some(window) = self.window_manager.get_mut(iced_id) else {
                    continue;
                };
                self.user_interfaces.build(
                    iced_id,
                    cache,
                    &mut window.renderer,
                    window.state.viewport().logical_size(),
                );
            }
        } else {
            for (iced_id, window) in rebuilds {
                if let Some(cache) = self.user_interfaces.remove(&iced_id) {
                    self.user_interfaces.build(
                        iced_id,
                        cache,
                        &mut window.renderer,
                        window.state.viewport().logical_size(),
                    );
                }
            }
        }
    }

    fn handle_ui_state(
        ev: &mut WindowState<IcedId>,
        window: &mut Window<P, C>,
        ui_state: user_interface::State,
        unconditional_rendering: bool,
        update_ime: bool,
    ) -> bool {
        match ui_state {
            user_interface::State::Outdated => {
                tracing::trace!(
                    "handle_ui_state: Outdated for window {:?} (rebuild needed)",
                    window.iced_id,
                );
                true
            }
            user_interface::State::Updated {
                redraw_request,
                input_method,
                mouse_interaction,
                ..
            } => {
                tracing::trace!(
                    "handle_ui_state: Updated for window {:?}, mouse_interaction={:?}, cached={:?}",
                    window.iced_id,
                    mouse_interaction,
                    window.mouse_interaction,
                );
                if unconditional_rendering {
                    ev.request_refresh(window.id, RefreshRequest::NextFrame);
                } else {
                    match redraw_request {
                        RedrawRequest::NextFrame => {
                            ev.request_refresh(window.id, RefreshRequest::NextFrame)
                        }
                        RedrawRequest::At(instant) => {
                            ev.request_refresh(window.id, RefreshRequest::At(instant))
                        }
                        RedrawRequest::Wait => {}
                    }
                }

                if update_ime {
                    let ime_flags = window.request_input_method(input_method.clone());
                    match input_method {
                        iced_core::InputMethod::Disabled => {
                            if ime_flags.contains(ImeState::Disabled) {
                                ev.set_ime_allowed(false);
                            }
                        }
                        iced_core::InputMethod::Enabled {
                            purpose,
                            preedit: _,
                            cursor,
                        } => {
                            if ime_flags.contains(ImeState::Allowed) {
                                ev.set_ime_allowed(true);
                            }

                            if ime_flags.contains(ImeState::Update) {
                                ev.set_ime_purpose(conversion::ime_purpose(purpose));
                                ev.set_ime_cursor_area(
                                    layershellev::dpi::LogicalPosition::new(cursor.x, cursor.y),
                                    layershellev::dpi::LogicalSize {
                                        width: cursor.width,
                                        height: cursor.height,
                                    },
                                    window.id,
                                );
                            }
                        }
                    }
                }

                if mouse_interaction != window.mouse_interaction {
                    // Only send cursor shape requests when the cursor is
                    // actually over this window.  Without this guard, newly
                    // created windows (e.g. popups) that have no cursor would
                    // override the cursor shape set by the window the pointer
                    // is really on, because they share the same wl_pointer.
                    if window.state.mouse_position().is_some() {
                        tracing::trace!(
                            "Cursor shape changing for window {:?}: {:?} -> {:?}",
                            window.id,
                            window.mouse_interaction,
                            mouse_interaction,
                        );
                        if let Some(pointer) = ev.get_pointer() {
                            ev.append_return_data(ReturnData::RequestSetCursorShape((
                                conversion::mouse_interaction(mouse_interaction),
                                pointer.clone(),
                            )));
                        }
                    }
                    window.mouse_interaction = mouse_interaction;
                }
                false
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update<P: IcedProgram, E: Executor>(
    application: &mut Instance<P>,
    runtime: &mut MultiRuntime<E, P::Message>,
    messages: &mut Vec<P::Message>,
) where
    P::Theme: DefaultStyle,
    P::Message: 'static,
{
    for message in messages.drain(..) {
        let task = runtime.enter(|| application.update(message));

        if let Some(stream) = iced_runtime::task::into_stream(task) {
            runtime.run(stream);
        }
    }

    let subscription = runtime.enter(|| application.subscription());
    let recipes = iced_futures::subscription::into_recipes(subscription.map(Action::Output));

    iced_debug::subscriptions_tracked(recipes.len());
    runtime.track(recipes);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_action<P, C, E: Executor>(
    user_interfaces: &mut UserInterfaces<P>,
    compositor: &mut Option<C>,
    event: Action<P::Message>,
    messages: &mut Vec<P::Message>,
    clipboard: &mut LayerShellClipboard,
    waiting_layer_shell_actions: &mut Vec<(Option<iced_core::window::Id>, LayershellCustomAction)>,
    should_exit: &mut bool,
    window_manager: &mut WindowManager<P, C>,
    system_theme: &mut iced_core::theme::Mode,
    runtime: &mut MultiRuntime<E, P::Message>,
    ev: &mut WindowState<IcedId>,
) where
    P: IcedProgram + 'static,
    C: Compositor<Renderer = P::Renderer> + 'static,
    P::Theme: DefaultStyle,
    P::Message: 'static + TryInto<LayershellCustomActionWithId, Error = P::Message>,
{
    use iced_core::widget::operation;
    use iced_runtime::Action;
    use iced_runtime::clipboard;

    use iced_runtime::window::Action as WindowAction;
    match event {
        Action::Output(stream) => match stream.try_into() {
            Ok(action) => {
                let LayershellCustomActionWithId(id, action) = action;
                waiting_layer_shell_actions.push((id, action));
            }
            Err(stream) => {
                messages.push(stream);
            }
        },
        Action::Image(action) => match action {
            iced_runtime::image::Action::Allocate(handle, sender) => {
                use iced_core::Renderer as _;

                // TODO: Shared image cache in compositor
                if let Some((_id, window)) = window_manager.iter_mut().next() {
                    window.renderer.allocate_image(&handle, move |allocation| {
                        let _ = sender.send(allocation);
                    });
                }
            }
        },
        Action::Clipboard(action) => match action {
            clipboard::Action::Read { target, channel } => {
                let _ = channel.send(clipboard.read(target));
            }
            clipboard::Action::Write { target, contents } => {
                clipboard.write(target, contents);
            }
        },
        Action::Widget(action) => {
            let mut current_operation = Some(action);

            while let Some(mut operation) = current_operation.take() {
                // kind of suboptimal that we have to iterate over all windows, but since an operation does not have
                // a window id associated with it, this is the best we can do for now
                for (id, window) in window_manager.iter_mut() {
                    if let Some(mut ui) = user_interfaces.ui_mut(&id) {
                        ui.operate(&window.renderer, operation.as_mut());
                    }
                }

                match operation.finish() {
                    operation::Outcome::None => {}
                    operation::Outcome::Some(()) => {}
                    operation::Outcome::Chain(next) => {
                        current_operation = Some(next);
                    }
                }
            }
            // Request refresh after widget operations (e.g., focus) to update visual state
            ev.request_refresh_all(RefreshRequest::NextFrame);
        }
        Action::Window(action) => match action {
            WindowAction::Close(id) => {
                waiting_layer_shell_actions.push((Some(id), LayershellCustomAction::RemoveWindow));
            }
            WindowAction::GetOldest(channel) => {
                let _ = channel.send(window_manager.first_window().map(|(id, _)| *id));
            }
            WindowAction::GetLatest(channel) => {
                let _ = channel.send(window_manager.last_window().map(|(id, _)| *id));
            }
            WindowAction::GetSize(id, channel) => 'out: {
                let Some(window) = window_manager.get(id) else {
                    break 'out;
                };
                let _ = channel.send(window.state.window_size_f32());
            }
            WindowAction::Screenshot(id, channel) => 'out: {
                let Some(window) = window_manager.get_mut(id) else {
                    break 'out;
                };
                let Some(compositor) = compositor else {
                    break 'out;
                };
                let bytes = compositor.screenshot(
                    &mut window.renderer,
                    window.state.viewport(),
                    window.state.background_color(),
                );

                let _ = channel.send(iced_core::window::Screenshot::new(
                    bytes,
                    window.state.viewport().physical_size(),
                    window.state.viewport().scale_factor(),
                ));
            }
            WindowAction::GetScaleFactor(id, channel) => {
                if let Some(window) = window_manager.get_mut(id) {
                    let _ = channel.send(window.state.wayland_scale_factor() as f32);
                };
            }
            WindowAction::RedrawAll => {
                ev.request_refresh_all(RefreshRequest::NextFrame);
            }
            WindowAction::RelayoutAll => {
                // Rebuild all user interfaces and request refresh
                for (iced_id, window) in window_manager.iter_mut() {
                    if let Some(cache) = user_interfaces.remove(&iced_id) {
                        user_interfaces.build(
                            iced_id,
                            cache,
                            &mut window.renderer,
                            window.state.viewport().logical_size(),
                        );
                    }
                }
                ev.request_refresh_all(RefreshRequest::NextFrame);
            }
            _ => {}
        },
        Action::System(action) => match action {
            iced_runtime::system::Action::GetTheme(channel) => {
                let _ = channel.send(*system_theme);
            }
            iced_runtime::system::Action::NotifyTheme(mode) => {
                if mode != *system_theme {
                    *system_theme = mode;

                    runtime.broadcast(iced_futures::subscription::Event::SystemThemeChanged(mode));
                }

                for (_id, window) in window_manager.iter_mut() {
                    window.state.update(
                        &LayerShellWindowEvent::ThemeChanged(mode),
                        user_interfaces.application(),
                    );
                }

                ev.request_refresh_all(RefreshRequest::NextFrame);
            }

            _ => {}
        },
        Action::Exit => {
            *should_exit = true;
        }
        Action::LoadFont { bytes, channel } => {
            if let Some(compositor) = compositor {
                // TODO: Error handling (?)
                compositor.load_font(bytes.clone());

                let _ = channel.send(Ok(()));
            }
        }
        Action::Reload => {
            for (iced_id, window) in window_manager.iter_mut() {
                if let Some(cache) = user_interfaces.remove(&iced_id) {
                    user_interfaces.build(
                        iced_id,
                        cache,
                        &mut window.renderer,
                        window.state.viewport().logical_size(),
                    );
                }
            }
            ev.request_refresh_all(RefreshRequest::NextFrame);
        }
        Action::Tick => {
            // Tick is handled internally by the runtime
        }
    }
}
