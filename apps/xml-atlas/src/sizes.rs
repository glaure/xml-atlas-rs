//! The sizes remembered between sessions: the window's and the panes'
//! (`docs/spec/window.md`, WIN-3; D38).
//!
//! A pane's size is its share of the room there is for it, so it fits another window or
//! screen. The share changes only when the user moves the pane's border: what a pane holds,
//! or a window that is for a moment smaller while it starts, cannot change what is saved.
//! Not eframe's `persistence`, which stores all of egui's memory in a second file (D38).

use std::time::{Duration, Instant};

use egui::containers::panel::PanelState;
use egui::{Frame, IdSalt, InnerResponse, Ui, accesskit};

/// A share of 1 in the units that are kept.
const WHOLE: u16 = 10_000;
/// A change of the window is kept once it has held this long.
const SETTLE: Duration = Duration::from_secs(1);
/// Sizes a saved window may have; others are not believed.
const WINDOW_SIZES: std::ops::RangeInclusive<u16> = 200..=16_384;
/// The tree's width until it is dragged.
const TREE_WIDTH: f32 = 320.0;

/// The panes with a border that can be dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Split {
    Tree,
    GridAbove,
    GridBelow,
    DiagramAbove,
    DiagramBelow,
    CellPane,
    Preview,
    PreviewTree,
    Problems,
    XPathResults,
}

/// The shares of the panes; `None` until the user moves a pane's border.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Panes([Option<u16>; Split::ALL.len()]);

/// The window when it is neither maximized nor full screen, in points, and whether it was
/// maximized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    pub width: u16,
    pub height: u16,
    pub maximized: bool,
}

/// What the window is like in one frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Seen {
    size: egui::Vec2,
    maximized: bool,
    /// Minimized or full screen: says nothing about the size to open with.
    passing: bool,
}

/// Follows the window and says what to remember of it.
pub struct Tracker {
    started: Instant,
    pending: Option<(Window, Instant)>,
}

impl Default for Window {
    fn default() -> Self {
        Self {
            width: 1280,
            height: 820,
            maximized: false,
        }
    }
}

impl Split {
    pub const ALL: [Split; 10] = [
        Split::Tree,
        Split::GridAbove,
        Split::GridBelow,
        Split::DiagramAbove,
        Split::DiagramBelow,
        Split::CellPane,
        Split::Preview,
        Split::PreviewTree,
        Split::Problems,
        Split::XPathResults,
    ];

    /// The pane's name in the settings file, which is also its id.
    fn key(self) -> &'static str {
        match self {
            Split::Tree => "tree",
            Split::GridAbove => "grid-above",
            Split::GridBelow => "grid-below",
            Split::DiagramAbove => "diagram-above",
            Split::DiagramBelow => "diagram-below",
            Split::CellPane => "cell-pane",
            Split::Preview => "reference-preview",
            Split::PreviewTree => "reference-tree",
            Split::Problems => "problems",
            Split::XPathResults => "xpath-results",
        }
    }

    fn panel(self) -> egui::Panel {
        let key = self.key();
        match self {
            Split::Tree => egui::Panel::left(key).default_size(TREE_WIDTH),
            Split::GridAbove | Split::DiagramAbove => egui::Panel::top(key),
            Split::CellPane | Split::Preview | Split::PreviewTree => egui::Panel::right(key),
            Split::GridBelow | Split::DiagramBelow | Split::Problems | Split::XPathResults => {
                egui::Panel::bottom(key)
            }
        }
    }

    /// 0 for a pane with a width, 1 for one with a height.
    fn axis(self) -> usize {
        match self {
            Split::Tree | Split::CellPane | Split::Preview | Split::PreviewTree => 0,
            _ => 1,
        }
    }

    /// The share until the user chooses one; `None` where the size is in points or follows
    /// what the pane holds.
    fn default_share(self) -> Option<f32> {
        match self {
            Split::GridAbove | Split::GridBelow => Some(0.45),
            Split::DiagramAbove | Split::DiagramBelow => Some(0.45),
            Split::CellPane => Some(0.35),
            Split::Preview => Some(0.45),
            Split::PreviewTree => Some(0.2),
            Split::Tree | Split::Problems | Split::XPathResults => None,
        }
    }

    /// Whether the pane is as large as it was made, whatever it holds. The two lists are only
    /// as high as their rows, up to a few.
    fn fills(self) -> bool {
        !matches!(self, Split::Problems | Split::XPathResults)
    }
}

impl Panes {
    /// Shows a pane at its share, and takes the new share when its border is let go. The
    /// contents get the shares too, for panes inside this one.
    pub fn show<R>(
        &mut self,
        ui: &mut Ui,
        split: Split,
        frame: Option<Frame>,
        add: impl FnOnce(&mut Ui, &mut Panes) -> R,
    ) -> InnerResponse<R> {
        let axis = split.axis();
        let room = ui.available_rect_before_wrap().size()[axis];
        // As `Panel` makes the ids of its state and of its border (egui 0.37, panel.rs).
        let id = ui.scope_id().with_salt(IdSalt::new(split.key()));
        let border = id.with("__resize");
        let (dragged, released) = ui
            .ctx()
            .read_response(border)
            .map_or((false, false), |r| (r.dragged(), r.drag_stopped()));
        // Assistive technology moves a border without a drag.
        let stepped = ui.input(|i| {
            use accesskit::Action::{Decrement, Increment, SetValue};
            [Increment, Decrement, SetValue]
                .into_iter()
                .any(|action| i.has_accesskit_action_request(border, action))
        });

        let mut panel = split.panel().resizable(true);
        if let Some(frame) = frame {
            panel = panel.frame(frame);
        }
        let share = self.share(split).or(split.default_share());
        // While a border moves egui places it; at all other times the share does, also in a
        // window that has changed its size.
        if !dragged
            && !released
            && let Some(share) = share
        {
            ui.data_mut(|d| d.remove::<PanelState>(id));
            panel = panel.default_size(share * room);
        }

        let shown = panel.show(ui, |ui| {
            if split.fills() {
                // Or a pane with little in it shrinks to that, and stays so.
                if axis == 0 {
                    ui.set_min_width(ui.available_width());
                } else {
                    ui.set_min_height(ui.available_height());
                }
            }
            add(ui, self)
        });
        if (released || stepped) && room > 1.0 {
            self.set_share(split, shown.response.rect.size()[axis] / room);
        }
        shown
    }

    /// The `[panes]` lines of the settings file: one for each pane the user sized.
    pub fn to_text(self) -> String {
        Split::ALL
            .into_iter()
            .filter_map(|split| {
                let share = self.share(split)?;
                Some(format!("{} = {share:.4}\n", split.key()))
            })
            .collect()
    }

    /// Reads one `[panes]` line; `false` when it cannot be read.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let Some(split) = Split::ALL.into_iter().find(|s| s.key() == key) else {
            return false;
        };
        match value.parse::<f32>() {
            Ok(share) if share > 0.0 && share <= 1.0 => {
                self.set_share(split, share);
                true
            }
            _ => false,
        }
    }

    fn share(&self, split: Split) -> Option<f32> {
        self.0[split as usize].map(|kept| f32::from(kept) / f32::from(WHOLE))
    }

    fn set_share(&mut self, split: Split, share: f32) {
        let kept = (share.clamp(0.0, 1.0) * f32::from(WHOLE)).round() as u16;
        self.0[split as usize] = Some(kept.max(1));
    }
}

impl Window {
    /// The `[window]` lines of the settings file.
    pub fn to_text(self) -> String {
        format!(
            "width = {}\nheight = {}\nmaximized = {}\n",
            self.width, self.height, self.maximized
        )
    }

    /// Reads one `[window]` line; `false` when it cannot be read.
    pub fn read(&mut self, key: &str, value: &str) -> bool {
        let size = || value.parse().ok().filter(|v| WINDOW_SIZES.contains(v));
        match key {
            "width" => size().map(|v| self.width = v).is_some(),
            "height" => size().map(|v| self.height = v).is_some(),
            "maximized" => value.parse().map(|v| self.maximized = v).is_ok(),
            _ => false,
        }
    }
}

impl Seen {
    /// The window now. Its size comes from the viewport, as Wayland tells no position and
    /// with it no window rectangle.
    pub fn now(ctx: &egui::Context) -> Self {
        // Saved without the zoom, which is 1 again at the next start.
        let size = ctx.viewport_rect().size() * ctx.zoom_factor();
        ctx.input(|i| {
            let v = i.viewport();
            Self {
                size,
                // macOS tells it only at the start (egui-winit's `update_viewport_info`): a
                // zoomed window is remembered by its size there.
                maximized: !cfg!(target_os = "macos") && v.maximized == Some(true),
                passing: v.minimized == Some(true) || v.fullscreen == Some(true),
            }
        })
    }
}

impl Tracker {
    pub fn new(now: Instant) -> Self {
        Self {
            started: now,
            pending: None,
        }
    }

    /// Puts what `seen` says into `saved` once it has held for [`SETTLE`]. Returns when to
    /// look again while it has not.
    pub fn observe(&mut self, seen: Seen, saved: &mut Window, now: Instant) -> Option<Duration> {
        if seen.passing {
            self.pending = None;
            return None;
        }
        let mut next = *saved;
        next.maximized = seen.maximized;
        if !seen.maximized {
            let (w, h) = (seen.size.x.round() as u16, seen.size.y.round() as u16);
            // A point either way is rounding at a fractional scale, not a new size.
            let moved = w.abs_diff(saved.width) > 1 || h.abs_diff(saved.height) > 1;
            if moved && WINDOW_SIZES.contains(&w) && WINDOW_SIZES.contains(&h) {
                (next.width, next.height) = (w, h);
            }
        }
        if next == *saved {
            self.pending = None;
            return None;
        }
        let since = match self.pending {
            Some((window, since)) if window == next => since,
            _ => now,
        };
        let held = now.duration_since(since);
        if held >= SETTLE {
            *saved = next;
            self.pending = None;
            None
        } else {
            self.pending = Some((next, since));
            Some(SETTLE - held)
        }
    }

    /// At exit a change counts without waiting, except while the window was still starting:
    /// then it has not yet become what was asked for.
    pub fn finish(&mut self, saved: &mut Window, now: Instant) {
        if let Some((window, _)) = self.pending.take()
            && now.duration_since(self.started) >= SETTLE
        {
            *saved = window;
        }
    }
}

/// When `panes` were reset since they were last `shown`, drops the sizes egui holds for
/// this session: the panes without a first share would keep theirs.
pub fn follow_reset(ctx: &egui::Context, shown: &mut Panes, panes: Panes) {
    if *shown != panes && panes == Panes::default() {
        ctx.data_mut(|d| d.remove_by_type::<PanelState>());
    }
    *shown = panes;
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Rect, pos2, vec2};

    use super::*;

    const ROOM: egui::Vec2 = vec2(1000.0, 700.0);

    /// One frame with `split` as the only pane, `rows` lines in it. Returns where it is.
    fn frame(
        ctx: &Context,
        panes: &mut Panes,
        split: Split,
        room: egui::Vec2,
        rows: usize,
        events: Vec<Event>,
    ) -> Rect {
        let input = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
            ..Default::default()
        };
        let mut rect = Rect::NOTHING;
        let mut out = ctx.run_ui(input, |ui| {
            rect = panes
                .show(ui, split, None, |ui, _| {
                    for _ in 0..rows {
                        ui.label("Größe");
                    }
                })
                .response
                .rect;
            egui::CentralPanel::default().show(ui, |_| {});
        });
        out.textures_delta.clear();
        rect
    }

    fn settled(ctx: &Context, panes: &mut Panes, split: Split, room: egui::Vec2) -> Rect {
        frame(ctx, panes, split, room, 1, Vec::new());
        frame(ctx, panes, split, room, 1, Vec::new())
    }

    /// Drags the pane's inner border so that the pane grows by `by`.
    fn drag(ctx: &Context, panes: &mut Panes, split: Split, by: f32) {
        let rect = settled(ctx, panes, split, ROOM);
        let (from, towards) = match split {
            Split::Tree => (pos2(rect.right() - 1.0, rect.center().y), vec2(1.0, 0.0)),
            Split::GridAbove | Split::DiagramAbove => {
                (pos2(rect.center().x, rect.bottom() - 1.0), vec2(0.0, 1.0))
            }
            _ if split.axis() == 0 => (pos2(rect.left() + 1.0, rect.center().y), vec2(-1.0, 0.0)),
            _ => (pos2(rect.center().x, rect.top() + 1.0), vec2(0.0, -1.0)),
        };
        let button = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let to = from + towards * by;
        for events in [
            vec![Event::PointerMoved(from)],
            vec![button(from, true)],
            vec![Event::PointerMoved(from + towards * by / 2.0)],
            vec![Event::PointerMoved(to)],
            vec![button(to, false)],
            vec![],
        ] {
            frame(ctx, panes, split, ROOM, 1, events);
        }
    }

    /// A dragged border is there again after a start, for each pane that is as large as it
    /// was made, larger and smaller.
    #[test]
    fn a_dragged_pane_has_its_size_again_after_a_start() {
        for split in Split::ALL.into_iter().filter(|s| s.fills()) {
            for by in [90.0, -90.0] {
                let ctx = Context::default();
                let mut panes = Panes::default();
                let before = settled(&ctx, &mut panes, split, ROOM).size()[split.axis()];
                drag(&ctx, &mut panes, split, by);
                let after = settled(&ctx, &mut panes, split, ROOM).size()[split.axis()];
                assert!(
                    (after - before - by).abs() < 8.0,
                    "{split:?} by {by}: {before} to {after}"
                );
                assert!(panes.share(split).is_some(), "{split:?} is not remembered");

                let mut read = Panes::default();
                for line in panes.to_text().lines() {
                    let (key, value) = line.split_once(" = ").unwrap();
                    assert!(read.read(key, value), "{line}");
                }
                let again = settled(&Context::default(), &mut read, split, ROOM);
                assert!(
                    (again.size()[split.axis()] - after).abs() < 1.0,
                    "{split:?} by {by}: {after}, after a start {again:?}"
                );
            }
        }
    }

    /// A window that is small while it starts, or for a while, loses no share.
    #[test]
    fn a_share_follows_the_window_and_is_not_changed_by_it() {
        let mut panes = Panes::default();
        assert!(panes.read("reference-preview", "0.6"));
        let saved = panes;
        let ctx = Context::default();
        for width in [400.0, 1800.0, 120.0, 1000.0] {
            let rect = settled(&ctx, &mut panes, Split::Preview, vec2(width, 700.0));
            assert!(
                (rect.width() - 0.6 * width).abs() < 2.0,
                "{width}: {rect:?}"
            );
            assert_eq!(panes, saved);
        }
    }

    /// Little in a pane does not make it narrow (the cell pane before a cell is clicked).
    #[test]
    fn an_empty_pane_keeps_its_width() {
        let ctx = Context::default();
        let mut panes = Panes::default();
        for rows in [0, 0, 3, 0] {
            let rect = frame(&ctx, &mut panes, Split::CellPane, ROOM, rows, Vec::new());
            assert!((rect.width() - 350.0).abs() < 2.0, "{rows} rows: {rect:?}");
        }
        assert_eq!(panes, Panes::default());
    }

    /// The problems list is as high as its rows, not as the share, when it has few.
    #[test]
    fn a_list_is_no_higher_than_its_rows() {
        let mut panes = Panes::default();
        assert!(panes.read("problems", "0.5"));
        let rect = settled(&Context::default(), &mut panes, Split::Problems, ROOM);
        assert!(rect.height() < 100.0, "{rect:?}");
    }

    #[test]
    fn reset_gives_the_first_sizes_back() {
        let ctx = Context::default();
        let mut panes = Panes::default();
        drag(&ctx, &mut panes, Split::Tree, 150.0);
        assert!(settled(&ctx, &mut panes, Split::Tree, ROOM).width() > 400.0);
        let mut shown = panes;
        panes = Panes::default();
        follow_reset(&ctx, &mut shown, panes);
        let rect = settled(&ctx, &mut panes, Split::Tree, ROOM);
        assert!((rect.width() - TREE_WIDTH).abs() < 2.0, "{rect:?}");
    }

    #[test]
    fn impossible_sizes_are_not_believed() {
        let mut panes = Panes::default();
        for value in ["0", "-0.2", "1.5", "NaN", "inf", "wide"] {
            assert!(!panes.read("tree", value), "{value}");
        }
        assert!(!panes.read("sidebar", "0.3"));
        assert_eq!(panes, Panes::default());
        let mut window = Window::default();
        for value in ["0", "199", "16385", "99999", "-5", "wide"] {
            assert!(!window.read("width", value), "{value}");
        }
        assert_eq!(window, Window::default());
    }

    fn seen(width: f32, height: f32, maximized: bool) -> Seen {
        Seen {
            size: vec2(width, height),
            maximized,
            passing: false,
        }
    }

    /// Maximized keeps the size to go back to; full screen and minimized change nothing.
    #[test]
    fn the_window_is_kept_once_it_holds() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let mut tracker = Tracker::new(start);
        let mut saved = Window::default();

        // While a border is dragged nothing is written.
        assert!(
            tracker
                .observe(seen(1000.0, 700.0, false), &mut saved, at(0))
                .is_some()
        );
        assert!(
            tracker
                .observe(seen(900.0, 650.0, false), &mut saved, at(900))
                .is_some()
        );
        assert!(
            tracker
                .observe(seen(900.0, 650.0, false), &mut saved, at(1500))
                .is_some()
        );
        assert_eq!(saved, Window::default());
        assert_eq!(
            tracker.observe(seen(900.0, 650.0, false), &mut saved, at(1900)),
            None
        );
        assert_eq!(
            (saved.width, saved.height, saved.maximized),
            (900, 650, false)
        );

        tracker.observe(seen(1920.0, 1050.0, true), &mut saved, at(2000));
        tracker.observe(seen(1920.0, 1050.0, true), &mut saved, at(3000));
        assert_eq!(
            (saved.width, saved.height, saved.maximized),
            (900, 650, true)
        );

        let passing = Seen {
            passing: true,
            ..Seen::default()
        };
        assert_eq!(tracker.observe(passing, &mut saved, at(5000)), None);
        assert_eq!(
            (saved.width, saved.height, saved.maximized),
            (900, 650, true)
        );
    }

    /// Closing right after a change keeps it; closing while the window starts does not take
    /// the size it had before it was maximized.
    #[test]
    fn closing_keeps_the_last_change_but_not_the_start() {
        let start = Instant::now();
        let at = |ms| start + Duration::from_millis(ms);
        let maximized = Window {
            maximized: true,
            ..Window::default()
        };

        let (mut tracker, mut saved) = (Tracker::new(start), maximized);
        tracker.observe(seen(1280.0, 820.0, false), &mut saved, at(50));
        tracker.finish(&mut saved, at(300));
        assert_eq!(saved, maximized);

        tracker.observe(seen(1100.0, 750.0, false), &mut saved, at(5000));
        tracker.finish(&mut saved, at(5100));
        assert_eq!(
            (saved.width, saved.height, saved.maximized),
            (1100, 750, false)
        );
    }
}
