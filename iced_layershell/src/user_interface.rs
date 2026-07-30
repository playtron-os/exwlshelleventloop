use iced_core::{Event, Size, event::Status, mouse::Cursor, window::Id};
use iced_core::{renderer::Style, widget::Operation};
use iced_program::{Instance, Program};
use iced_runtime::{
    UserInterface as IcedUserInterface,
    user_interface::{Cache, State},
};
use std::{collections::HashMap, mem};

pub(crate) trait UserInterfaceReclaim<Message, Theme, Renderer> {
    fn reclaim(&mut self, ui: IcedUserInterface<'static, Message, Theme, Renderer>);
}

/// Provide a guard to hold the ui and prevent leaking the reference to the application. A user can hold this guard without querying from map each time.
/// When this guard is dropped, it will return the ui to the manager if it is not taken.
pub(crate) struct UserInterfaceMutGuard<'a, Message, Theme, Renderer, Reclaim>
where
    Reclaim: UserInterfaceReclaim<Message, Theme, Renderer>,
{
    reclaim: Reclaim,
    /// Building 'static IcedUserInterface will draw nothing, so we should use the safe lifetime as
    /// application.
    ui: Option<IcedUserInterface<'a, Message, Theme, Renderer>>,
}

impl<'a, Message, Theme, Renderer, Reclaim>
    UserInterfaceMutGuard<'a, Message, Theme, Renderer, Reclaim>
where
    Renderer: iced_core::Renderer,
    Reclaim: UserInterfaceReclaim<Message, Theme, Renderer>,
{
    fn take(&mut self) -> IcedUserInterface<'a, Message, Theme, Renderer> {
        self.ui.take().expect("ui is taken")
    }

    pub fn draw(&mut self, renderer: &mut Renderer, theme: &Theme, style: &Style, cursor: Cursor) {
        let mut ui = self.take();
        ui.draw(renderer, theme, style, cursor);
        self.ui = Some(ui);
    }

    #[allow(unused)]
    pub fn into_cache(mut self) -> Cache {
        self.take().into_cache()
    }

    pub fn operate(&mut self, renderer: &Renderer, operation: &mut dyn Operation<()>) {
        let mut ui = self.take();
        ui.operate(renderer, operation);
        self.ui = Some(ui);
    }

    pub fn relayout(mut self, bounds: Size, renderer: &mut Renderer) -> Self {
        let ui = self.take().relayout(bounds, renderer);
        self.ui = Some(ui);
        self
    }

    /// Returns the size of the root content after layout.
    ///
    /// This is the actual size that the content wants to be, which may be
    /// smaller than the bounds passed to `build` or `relayout`.
    pub fn content_size(&self) -> Size {
        self.ui.as_ref().expect("ui is taken").content_size()
    }

    pub fn update(
        &mut self,
        events: &[Event],
        cursor: Cursor,
        renderer: &mut Renderer,
        messages: &mut Vec<Message>,
    ) -> (State, Vec<Status>) {
        let mut ui = self.take();
        let res = ui.update(events, cursor, renderer, messages);
        self.ui = Some(ui);
        res
    }
}

impl<Message, Theme, Renderer, Reclaim> Drop
    for UserInterfaceMutGuard<'_, Message, Theme, Renderer, Reclaim>
where
    Reclaim: UserInterfaceReclaim<Message, Theme, Renderer>,
{
    fn drop(&mut self) {
        if let Some(ui) = self.ui.take() {
            // SAFETY There is no public api to change ui. It always refers to application.
            let ui: IcedUserInterface<'static, _, _, _> = unsafe { mem::transmute(ui) };
            self.reclaim.reclaim(ui);
        }
    }
}

pub struct UserInterfaces<P: Program> {
    // SAFETY application will only be dropped after all uis are dropped. And we won't
    // allow publicly access to IcedUserInterface<'static, A::Message, A::Theme, A::Renderer>, so
    // reference to application won't be leaked to public.
    #[allow(clippy::type_complexity)]
    uis: HashMap<Id, IcedUserInterface<'static, P::Message, P::Theme, P::Renderer>>,
    /// Windows whose view is stale, holding the widget-tree cache their next
    /// build will restore from.
    ///
    /// A window lands here when application state changes under it. Rebuilding is
    /// deferred to [`Self::ui_mut`] — the first thing that actually needs the tree
    /// — so a burst of messages costs one `view()` + layout per window instead of
    /// one per message, and a window nothing touches before the next invalidation
    /// costs none at all. An id lives in exactly one of the two maps.
    stale: HashMap<Id, Cache>,
    application: Instance<P>,
}

impl<P: Program> UserInterfaces<P>
where
    P: Program + 'static,
{
    pub fn new(application: Instance<P>) -> Self {
        Self {
            uis: HashMap::new(),
            stale: HashMap::new(),
            application,
        }
    }

    pub fn application(&self) -> &Instance<P> {
        &self.application
    }

    pub fn remove(&mut self, id: &Id) -> Option<Cache> {
        self.stale
            .remove(id)
            .or_else(|| self.uis.remove(id).map(IcedUserInterface::into_cache))
    }

    /// Whether this window's view is waiting to be rebuilt.
    ///
    /// For callers that would otherwise lay a window out twice: the pending build
    /// lays it out at whatever size [`Self::ui_mut`] is given, so a relayout on top
    /// of it is wasted work.
    pub fn is_stale(&self, id: &Id) -> bool {
        self.stale.contains_key(id)
    }

    /// Mark one window's view stale, to be rebuilt on next access.
    pub fn invalidate(&mut self, id: &Id) {
        if let Some(ui) = self.uis.remove(id) {
            self.stale.insert(*id, ui.into_cache());
        }
        // Already stale: keep the cache we have. Overwriting it with a fresh one
        // would drop the widget state (focus, scroll offsets, running animations)
        // the pending build is meant to restore.
    }

    /// Mark every window's view stale and hand back the application.
    ///
    /// Returning the `&mut Instance<P>` from the same call is what makes this
    /// sound: dropping every live `UserInterface` first is precisely the
    /// precondition for handing out a mutable reference to the application they
    /// borrow from.
    pub fn invalidate_all(&mut self) -> &mut Instance<P> {
        // SAFETY remove all references before return mut reference of application
        for (id, ui) in self.uis.drain() {
            self.stale.entry(id).or_insert_with(|| ui.into_cache());
        }
        &mut self.application
    }

    #[allow(clippy::type_complexity)]
    pub fn ui_mut(
        &mut self,
        id: &Id,
        renderer: &mut P::Renderer,
        size: Size,
    ) -> Option<UserInterfaceMutGuard<'static, P::Message, P::Theme, P::Renderer, (&mut Self, Id)>>
    {
        if let Some(cache) = self.stale.remove(id) {
            self.build(*id, cache, renderer, size);
        }

        self.uis.remove(id).map(|ui| UserInterfaceMutGuard {
            reclaim: (self, *id),
            ui: Some(ui),
        })
    }

    pub fn build(&mut self, id: Id, cache: Cache, renderer: &mut P::Renderer, size: Size) {
        // A build supersedes any pending one, so the stale entry has to go with
        // it — otherwise the next `ui_mut` would rebuild from the older cache.
        self.stale.remove(&id);

        let view_span = iced_debug::view(id);
        let view = self.application.view(id);
        view_span.finish();

        let layout_span = iced_debug::layout(id);
        let ui = IcedUserInterface::build(view, size, cache, renderer);
        layout_span.finish();
        // SAFETY ui won't outlive application.
        let ui: IcedUserInterface<'static, _, _, _> = unsafe { mem::transmute(ui) };
        self.uis.insert(id, ui);
    }
}

impl<P: Program> Drop for UserInterfaces<P> {
    fn drop(&mut self) {
        // SAFETY drop all references of application before dropping application
        self.uis.clear();
        self.stale.clear();
    }
}

impl<P: Program> UserInterfaceReclaim<P::Message, P::Theme, P::Renderer>
    for (&mut UserInterfaces<P>, Id)
{
    fn reclaim(&mut self, ui: IcedUserInterface<'static, P::Message, P::Theme, P::Renderer>) {
        self.0.uis.insert(self.1, ui);
    }
}
