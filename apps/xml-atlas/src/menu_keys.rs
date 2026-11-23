//! Opening the menu bar from the keyboard (#167): tapping Alt, as in other Windows programs,
//! puts the keyboard on the first menu. Alt held with another key, or with the mouse, is not a
//! tap; Alt+Z and Alt+Up keep their own meaning.

/// Follows Alt from frame to frame.
#[derive(Debug, Default)]
pub struct AltTap {
    down: bool,
    /// Something else happened while Alt was down.
    used: bool,
}

impl AltTap {
    /// One frame: whether Alt is down, and whether any other key or button was pressed.
    /// True in the frame Alt comes up when nothing else happened since it went down.
    pub fn update(&mut self, alt: bool, other_input: bool) -> bool {
        let tapped = self.down && !alt && !self.used;
        if alt && !self.down {
            self.used = false;
        }
        if alt && other_input {
            self.used = true;
        }
        self.down = alt;
        tapped
    }
}

/// A mouse press that began on a menu's name and has not been let go yet (see the module doc).
#[derive(Clone, Copy, Debug, Default)]
struct Drag {
    active: bool,
    /// Where the button was let go this frame, when a drag ended.
    release: Option<egui::Pos2>,
}

impl Drag {
    fn id() -> egui::Id {
        egui::Id::NULL.with("menu-drag")
    }

    fn get(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp(Self::id())).unwrap_or_default()
    }

    fn set(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }
}

/// What a menu entry asks of its response.
pub trait MenuPick {
    /// Clicked, or the mouse button was let go over it at the end of a drag from the bar.
    /// While such a drag is on, the entry under the pointer is highlighted.
    fn picked(&self) -> bool;
    /// The mouse button was let go over it at the end of a drag from the bar: for entries that
    /// toggle, which egui does not see as a click.
    fn released_over(&self) -> bool;
}

impl MenuPick for egui::Response {
    fn picked(&self) -> bool {
        self.clicked() || self.released_over()
    }

    fn released_over(&self) -> bool {
        let drag = Drag::get(&self.ctx);
        if drag.active && self.enabled() {
            let (down, pos) = self
                .ctx
                .input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
            if down && pos.is_some_and(|p| self.rect.contains(p)) {
                // The keyboard's highlight is the one that shows.
                self.request_focus();
            }
        }
        self.enabled() && drag.release.is_some_and(|p| self.rect.contains(p))
    }
}

/// A check box entry, for the end of a drag from the bar: flips `value` when the button was
/// let go over it. egui reports `changed` for a click only.
pub fn flip(response: &egui::Response, value: &mut bool) -> bool {
    let hit = response.released_over();
    if hit {
        *value = !*value;
    }
    hit
}

/// A radio entry, as [`flip`] is for check boxes.
pub fn choose<T: PartialEq>(response: &egui::Response, current: &mut T, value: T) -> bool {
    let hit = response.released_over() && *current != value;
    if hit {
        *current = value;
    }
    hit
}

/// The menu bar's keyboard state.
#[derive(Debug, Default)]
pub struct MenuKeys {
    alt: AltTap,
    /// Frames left in which the first entry of a freshly opened menu is to be focused: a
    /// popup is first laid out unseen, and only then takes the arrow keys.
    enter: u8,
    /// What had the keyboard before Alt took it, to give it back when the menu is left.
    restore: Option<egui::Id>,
}

impl MenuKeys {
    /// Call once per frame before the bar is drawn: the menu's own arrow keys act while it is
    /// drawn, so the request to enter a menu has to be there first.
    pub fn before(&mut self, ctx: &egui::Context) {
        let mut drag = Drag::get(ctx);
        drag.release = None;
        let (down, released, pos) = ctx.input(|i| {
            (
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.latest_pos(),
            )
        });
        if drag.active && released {
            drag.release = pos;
            drag.active = false;
        } else if !down {
            drag.active = false;
        }
        drag.set(ctx);
        if self.enter > 0 {
            self.enter -= 1;
            if self.enter == 0 && egui::Popup::is_any_open(ctx) {
                ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::Down));
            }
            ctx.request_repaint();
        }
    }

    /// Call once per frame after the bar is drawn, with the bar's buttons in order.
    /// `in_submenu`: a submenu of the open menu has the keys (Left and Right are its own).
    pub fn bar(&mut self, ctx: &egui::Context, bar: &[egui::Response], in_submenu: bool) {
        use egui::{Key, Modifiers, Popup};
        let ids: Vec<egui::Id> = bar.iter().map(Popup::default_response_id).collect();
        let open = ids.iter().position(|id| Popup::is_id_open(ctx, *id));
        let focused = bar.iter().position(|r| r.has_focus());
        let tapped = alt_tapped(ctx, &mut self.alt);
        let press = |key| ctx.input_mut(|i| i.consume_key(Modifiers::NONE, key));
        let not_moving = || ctx.memory_mut(|m| m.move_focus(egui::FocusDirection::None));

        if tapped {
            if let (None, None) = (open, focused) {
                self.restore = ctx.memory(|m| m.focused());
                bar[0].request_focus();
            } else {
                // Alt again leaves the menu, as in other programs.
                Popup::close_all(ctx);
                if let Some(i) = focused {
                    ctx.memory_mut(|m| m.surrender_focus(bar[i].id));
                }
            }
        } else if let Some(i) = open {
            let step = |back: bool| (i + if back { bar.len() - 1 } else { 1 }) % bar.len();
            for (key, back) in [(Key::ArrowLeft, true), (Key::ArrowRight, false)] {
                if !in_submenu && press(key) {
                    let to = step(back);
                    Popup::open_id(ctx, ids[to]);
                    // The new menu is entered from its button, as after Down.
                    bar[to].request_focus();
                    self.enter = 8;
                    not_moving();
                }
            }
        } else if let Some(i) = focused
            && press(Key::ArrowDown)
        {
            Popup::open_id(ctx, ids[i]);
            self.enter = 8;
            not_moving();
        }

        self.mouse(ctx, bar, &ids, open);

        // Nothing of the menu has the keyboard any more (Esc, a click elsewhere): back to
        // where it was.
        if !tapped
            && self.enter == 0
            && open.is_none()
            && focused.is_none()
            && let Some(id) = self.restore.take()
        {
            ctx.memory_mut(|m| m.request_focus(id));
        }
    }
}

impl MenuKeys {
    fn mouse(
        &mut self,
        ctx: &egui::Context,
        bar: &[egui::Response],
        ids: &[egui::Id],
        open: Option<usize>,
    ) {
        use egui::Popup;
        let mut drag = Drag::get(ctx);
        let (pressed, down, pos) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.latest_pos(),
            )
        });
        let over = |p: egui::Pos2| bar.iter().position(|r| r.rect.contains(p));
        if let Some(pos) = pos {
            if pressed
                && let Some(j) = over(pos)
                && open != Some(j)
            {
                // Pressed on a menu that is not open: open it now and follow the pointer.
                Popup::open_id(ctx, ids[j]);
                drag.active = true;
            } else if drag.active
                && down
                && let Some(j) = over(pos)
                && open != Some(j)
            {
                Popup::open_id(ctx, ids[j]);
            }
        }
        if let Some(pos) = drag.release {
            if let Some(j) = over(pos) {
                // Let go on a name: the menu stays open, whatever egui's click made of it.
                Popup::open_id(ctx, ids[j]);
            } else if ctx
                .layer_id_at(pos)
                .is_none_or(|l| l.order != egui::Order::Foreground)
            {
                Popup::close_all(ctx);
            }
        }
        drag.set(ctx);
    }
}

fn alt_tapped(ctx: &egui::Context, tap: &mut AltTap) -> bool {
    let (alt, other) = ctx.input(|i| {
        let other = i.events.iter().any(|e| {
            matches!(e, egui::Event::Key { pressed: true, key, .. } if !is_modifier(*key))
                || matches!(e, egui::Event::PointerButton { pressed: true, .. })
                || matches!(e, egui::Event::Text(_))
        });
        (i.modifiers.alt, other)
    });
    tap.update(alt, other)
}

/// The modifier keys are keys to egui too; pressing Alt is not "something else".
fn is_modifier(key: egui::Key) -> bool {
    use egui::Key::*;
    matches!(
        key,
        ShiftLeft
            | ShiftRight
            | ControlLeft
            | ControlRight
            | AltLeft
            | AltRight
            | SuperLeft
            | SuperRight
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alone_is_a_tap() {
        let mut t = AltTap::default();
        assert!(!t.update(true, false));
        assert!(!t.update(true, false));
        assert!(t.update(false, false));
        assert!(!t.update(false, false));
    }

    #[test]
    fn with_a_key_or_click_it_is_not() {
        let mut t = AltTap::default();
        t.update(true, false);
        t.update(true, true);
        assert!(!t.update(false, false));
        // And the next tap is clean again.
        t.update(true, false);
        assert!(t.update(false, false));
    }

    #[test]
    fn a_key_in_the_frame_alt_goes_down_counts() {
        let mut t = AltTap::default();
        t.update(true, true);
        assert!(!t.update(false, false));
    }
}
