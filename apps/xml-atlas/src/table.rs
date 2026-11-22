//! The table of the grid and the cell pane, virtualized in both directions
//! (`docs/decisions.md`, D5, D42; GRID-9): only the rows and the columns in sight get cells.
//!
//! The layout, resizing and hover rules, and the ids, are adapted from `egui_extras` 0.37's
//! table (MIT OR Apache-2.0, Copyright (c) 2018-2021 Emil Ernerfeldt; the notice is in
//! `licenses/egui-MIT.txt`), which makes a `Ui` for every column of every row in each frame.
//! What is left out here is what the two views do not use.
//!
//! A cell's id comes from its row and its column, not from what is drawn beside it, so
//! menus, editors and clicks stay with their cell while the columns scroll. The first column
//! grows with its contents and is always drawn; every other one clips.
//!
//! What was in sight in the last frame is drawn in this one too: egui finds hover and clicks
//! by the last frame's rectangles, and the view can have jumped since. While it stands still
//! that is the same columns.

use egui::{
    CursorIcon, Id, IdSalt, Layout, Popup, Rangef, Rect, Response, ScrollArea, Sense, Ui,
    UiBuilder, UiKind, UiStackInfo, emath::GuiRounding as _, pos2, scroll_area::DragScroll,
    scroll_area::ScrollSource, vec2,
};

/// Drawn past either edge of what is in sight: a stripe reaches half the spacing beyond its
/// cell, and the pointer takes a widget a few points away.
const MARGIN: f32 = 12.0;
/// The body is never made lower than this while it scrolls.
const MIN_SCROLLED_HEIGHT: f32 = 200.0;

/// One column.
#[derive(Clone, Copy, Debug)]
pub struct Column {
    width: f32,
    min: f32,
    /// Sized by its contents when the table is first shown; only the first column can be.
    auto: bool,
}

/// A table being set up.
pub struct Table<'a> {
    ui: &'a mut Ui,
    salt: IdSalt,
    columns: Vec<Column>,
    scroll_to: Option<usize>,
    keep: Vec<usize>,
}

/// A table whose header is drawn.
pub struct Body<'a> {
    ui: &'a mut Ui,
    id: Id,
    top: f32,
    scroll_to: Option<usize>,
    plan: Plan,
}

/// A row being drawn: [`Row::next_column`] names each column to draw, [`Row::col`] draws it.
pub struct Row<'a> {
    ui: &'a mut Ui,
    plan: &'a mut Plan,
    layout: Layout,
    index: usize,
    left: f32,
    top: f32,
    height: f32,
    striped: bool,
    hovered: bool,
    selected: bool,
    at: usize,
    bottom: f32,
    response: Option<Response>,
}

/// What is kept between frames.
#[derive(Clone, Debug, Default)]
struct State {
    widths: Vec<f32>,
    /// Drawn also out of sight: a menu is open on one of its cells, or it is pressed.
    held: Vec<usize>,
    /// The columns that were in sight.
    in_sight: std::ops::Range<usize>,
    /// The borders that were in sight.
    borders: std::ops::Range<usize>,
    /// The column whose border is pressed or dragged, which is followed out of sight.
    pressed: Option<usize>,
}

/// The columns of one frame: where they are, and which are drawn.
struct Plan {
    columns: Vec<Column>,
    state: State,
    keep: Vec<usize>,
    /// The first column is being measured.
    measuring: bool,
    /// The widest content of the first column.
    first_used: f32,
    spacing: f32,
    seen: Rangef,
    /// Where the second column starts, for which `lefts` and `drawn` are worked out.
    from: Option<f32>,
    lefts: Vec<f32>,
    /// In order, the first column always first.
    drawn: Vec<usize>,
    any_popup: bool,
    held: Vec<usize>,
    in_sight: std::ops::Range<usize>,
}

impl Column {
    /// This wide until it is dragged, and never narrower than `min`.
    pub fn new(width: f32, min: f32) -> Self {
        Self {
            width,
            min,
            auto: false,
        }
    }

    /// As wide as its contents when the table is first shown.
    pub fn auto(min: f32) -> Self {
        Self {
            width: 100.0,
            min,
            auto: true,
        }
    }
}

impl<'a> Table<'a> {
    pub fn new(ui: &'a mut Ui, salt: impl egui::AsIdSalt, columns: Vec<Column>) -> Self {
        Self {
            ui,
            salt: IdSalt::new(salt),
            columns,
            scroll_to: None,
            keep: Vec::new(),
        }
    }

    /// Scrolls the row into view.
    pub fn scroll_to_row(mut self, row: Option<usize>) -> Self {
        self.scroll_to = row;
        self
    }

    /// Draws the column also out of sight: it is edited, or is to be scrolled into view.
    pub fn keep_column(mut self, column: Option<usize>) -> Self {
        self.keep.extend(column);
        self
    }

    /// Draws the header, which stays at the top.
    pub fn header(self, height: f32, add: impl FnOnce(&mut Row<'_>)) -> Body<'a> {
        let Self {
            ui,
            salt,
            columns,
            scroll_to,
            mut keep,
        } = self;
        let id = ui.scope_id().with(salt);
        let saved = ui.data_mut(|d| d.get_temp::<State>(id));
        // Also when the columns changed: the menu that hid one is still open.
        if let Some(state) = &saved {
            keep.extend(&state.held);
            keep.extend(state.in_sight.clone());
        }
        // Widths of another number of columns say nothing about these.
        let saved = saved.filter(|state| state.widths.len() == columns.len());
        let measuring = ui.is_sizing_pass() || saved.is_none() && columns.iter().any(|c| c.auto);
        let state = saved.unwrap_or_else(|| State {
            widths: columns.iter().map(|c| c.width).collect(),
            ..State::default()
        });
        let mut plan = Plan {
            keep,
            measuring,
            first_used: 0.0,
            spacing: ui.spacing().item_spacing.x,
            seen: Rangef::NOTHING,
            from: None,
            lefts: Vec::new(),
            drawn: Vec::new(),
            any_popup: Popup::is_any_open(ui.ctx()),
            held: Vec::new(),
            in_sight: 0..0,
            columns,
            state,
        };
        let layout = *ui.layout();
        let top = ui.cursor().top();
        ui.scope_builder(UiBuilder::new().scope_id(id.with("__header")), |ui| {
            let area = ui.available_rect_before_wrap();
            plan.look(ui.clip_rect().x_range());
            let mut row = Row::new(ui, &mut plan, layout, 0, area.min, height);
            add(&mut row);
            let bottom = row.bottom;
            let right = plan.right(area.left());
            ui.advance_cursor_after_rect(Rect::from_min_max(area.min, pos2(right, bottom)));
        });
        Body {
            ui,
            id,
            top,
            scroll_to,
            plan,
        }
    }
}

impl Body<'_> {
    /// Draws those of `count` rows, each `height` high, that are in sight.
    pub fn rows(self, height: f32, count: usize, mut add: impl FnMut(&mut Row<'_>)) {
        let Self {
            ui,
            id,
            top,
            scroll_to,
            mut plan,
        } = self;
        let layout = *ui.layout();
        let start = ui.cursor().min;
        ScrollArea::new([false, true])
            .id_salt(id.with("__scroll_area"))
            .scroll_source(ScrollSource {
                drag: DragScroll::OnTouch,
                ..Default::default()
            })
            .min_scrolled_height(MIN_SCROLLED_HEIGHT)
            .max_height(f32::INFINITY)
            .auto_shrink(false)
            .show(ui, |ui| {
                let clip = ui.clip_rect();
                let mut scroll_y = None;
                ui.scope_builder(UiBuilder::new().scope_id(id.with("__body")), |ui| {
                    let hovered_id = id.with("__table_hovered_row");
                    let hovered = ui.data_mut(|d| d.remove_temp::<usize>(hovered_id));
                    let area = ui.available_rect_before_wrap();
                    plan.look(ui.clip_rect().x_range());

                    let gap = ui.spacing().item_spacing.y;
                    let step = height + gap;
                    if let Some(row) = scroll_to {
                        let row = row.min(count.saturating_sub(1)) as f32;
                        let y = area.top();
                        scroll_y = Some(Rangef::new(y + row * step, y + (row + 1.0) * step));
                    }
                    let offset = (clip.top() - area.top()).min(count as f32 * step);
                    let mut y = area.top();
                    let mut first = 0;
                    if offset > 0.0 {
                        first = (offset / step).floor() as usize;
                        let before = pos2(area.left(), y);
                        y += first as f32 * step;
                        ui.expand_to_include_rect(Rect::from_two_pos(before, pos2(area.left(), y)));
                    }
                    let end = ((offset + clip.height()) / step).ceil() as usize + 1;
                    let end = end.min(count);

                    let mut bottom = area.top();
                    for index in first..end {
                        let at = pos2(area.left(), y);
                        let mut row = Row::new(ui, &mut plan, layout, index, at, height);
                        row.striped = index.is_multiple_of(2);
                        row.hovered = hovered == Some(index);
                        add(&mut row);
                        let over = row.response.as_ref().is_some_and(|r| r.hovered());
                        bottom = bottom.max(row.bottom);
                        y = bottom + gap;
                        if over {
                            ui.data_mut(|d| d.insert_temp(hovered_id, index));
                        }
                    }
                    if end < count {
                        let below = (count - end) as f32 * step - gap;
                        let before = pos2(area.left(), y);
                        ui.expand_to_include_rect(Rect::from_two_pos(
                            before,
                            before + vec2(0.0, below),
                        ));
                    }
                    let right = if first < end {
                        plan.right(area.left())
                    } else {
                        area.left()
                    };
                    let all = Rect::from_min_max(area.min, pos2(right, bottom)).round_ui();
                    ui.advance_cursor_after_rect(all);
                });
                if let Some(y) = scroll_y {
                    ui.scroll_to_rect(Rect::from_x_y_ranges(0.0..=0.0, y), None);
                }
            });
        plan.borders(ui, id, start.x, top);
        plan.held.sort_unstable();
        plan.held.dedup();
        plan.state.held = plan.held;
        plan.state.in_sight = plan.in_sight;
        ui.data_mut(|d| d.insert_temp(id, plan.state));
    }
}

impl<'a> Row<'a> {
    fn new(
        ui: &'a mut Ui,
        plan: &'a mut Plan,
        layout: Layout,
        index: usize,
        at: egui::Pos2,
        height: f32,
    ) -> Self {
        Self {
            ui,
            plan,
            layout,
            index,
            left: at.x,
            top: at.y,
            height,
            striped: false,
            hovered: false,
            selected: false,
            at: 0,
            bottom: at.y + height,
            response: None,
        }
    }

    pub fn index(&self) -> usize {
        self.index
    }

    /// Marks the cells drawn after this as selected.
    pub fn set_selected(&mut self, selected: bool) {
        self.selected = selected;
    }

    /// The next column to draw; `None` after the last one in sight. Each gets a [`Row::col`].
    pub fn next_column(&self) -> Option<usize> {
        if self.at == 0 {
            (!self.plan.columns.is_empty()).then_some(0)
        } else {
            self.plan.drawn.get(self.at).copied()
        }
    }

    /// Draws the column [`Row::next_column`] named. Returns the whole cell's response.
    pub fn col(&mut self, add: impl FnOnce(&mut Ui)) -> Response {
        let column = self.next_column().expect("a column to draw");
        let first = column == 0;
        let x = if first {
            self.left
        } else {
            self.plan.lefts[column]
        };
        let width = self.plan.state.widths[column];
        let cell = Rect::from_min_max(pos2(x, self.top), pos2(x + width, self.top + self.height));

        // No gap in the stripe or the highlight.
        let spacing = self.ui.spacing().item_spacing;
        let filled = cell.expand2(0.5 * spacing).round_ui();
        let visuals = self.ui.visuals();
        let fills = [
            (self.striped, visuals.faint_bg_color),
            (self.selected, visuals.selection.bg_fill),
            (
                self.hovered && !self.selected,
                visuals.widgets.hovered.bg_fill,
            ),
        ];
        for (on, fill) in fills {
            if on {
                self.ui.painter().rect_filled(filled, 0.0, fill);
            }
        }

        let id = self.ui.scope_id().with(IdSalt::new((self.index, column)));
        let mut builder = UiBuilder::new()
            .scope_id(id)
            .ui_stack_info(UiStackInfo::new(UiKind::TableCell))
            .max_rect(cell)
            .layout(self.layout)
            .sense(Sense::click());
        if first && self.plan.measuring {
            builder = builder.sizing_pass();
        }
        let mut ui = self.ui.new_child(builder);
        if !first {
            ui.shrink_clip_rect(cell);
            if !ui.is_sizing_pass() {
                // Better truncated than cut off.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
            }
        }
        if self.selected {
            let colour = ui.style().visuals.selection.stroke.color;
            ui.style_mut().visuals.override_text_color = Some(colour);
        }
        add(&mut ui);

        let used = ui.min_rect();
        // Clicks count on the whole cell.
        ui.set_min_size(cell.size());
        let taken = if first { cell | used } else { cell };
        self.ui.advance_cursor_after_rect(taken);
        self.bottom = self.bottom.max(taken.bottom());
        let response = ui.response();
        drop(ui);

        if first {
            self.plan.first_used = self.plan.first_used.max(used.width());
            self.plan.place(taken.right() + spacing.x);
        }
        if !first {
            let menu = self.plan.any_popup
                && Popup::is_id_open(self.ui.ctx(), Popup::default_response_id(&response));
            if menu || response.is_pointer_button_down_on() {
                self.plan.held.push(column);
            }
        }
        self.response = Some(match self.response.take() {
            Some(row) => row.union(response.clone()),
            None => response.clone(),
        });
        self.at += 1;
        response
    }

    /// The cells drawn so far, as one response.
    pub fn response(&self) -> Option<Response> {
        self.response.clone()
    }
}

impl Plan {
    /// What is in sight from now on: the header and the body each have their own.
    fn look(&mut self, seen: Rangef) {
        self.seen = seen;
        self.from = None;
    }

    /// Lays the columns after the first out from `from` and picks those to draw. Done again
    /// only when the first column came out with another width.
    fn place(&mut self, from: f32) {
        if self.from == Some(from) {
            return;
        }
        self.from = Some(from);
        let count = self.columns.len();
        self.lefts.clear();
        self.lefts.push(0.0); // The first column's, which each row has itself.
        let mut x = from;
        for width in self.state.widths.iter().skip(1) {
            self.lefts.push(x);
            x = x + width + self.spacing;
        }

        let seen = self.seen.expand(MARGIN);
        let lefts = &self.lefts[1..];
        // Sorted: from the one that reaches into sight to the last that starts in it.
        let start = lefts.partition_point(|&left| left <= seen.min).max(1);
        let end = 1 + lefts.partition_point(|&left| left <= seen.max);
        self.drawn.clear();
        self.drawn.push(0);
        let sight = start..end.min(count);
        if !sight.is_empty() {
            self.in_sight = sight.clone();
        }
        self.drawn.extend(sight);
        let kept = self.keep.iter().copied().filter(|&c| c > 0 && c < count);
        self.drawn.extend(kept);
        self.drawn.sort_unstable();
        self.drawn.dedup();
    }

    /// The right end of the last column, of a table that starts at `left`.
    fn right(&self, left: f32) -> f32 {
        match self.lefts.last() {
            Some(&x) if self.lefts.len() > 1 => x + self.state.widths[self.lefts.len() - 1],
            _ => left + self.state.widths.first().copied().unwrap_or(0.0),
        }
    }

    /// Draws the borders in sight and resizes by them.
    fn borders(&mut self, ui: &Ui, id: Id, left: f32, top: f32) {
        let bottom = ui.min_rect().bottom();
        let radius = ui.style().interaction.resize_grab_radius_side;
        let seen = ui.clip_rect().x_range().expand(MARGIN + radius);
        let pressed = self.state.pressed.take();
        let before = std::mem::take(&mut self.state.borders);
        let mut in_sight: Option<(usize, usize)> = None;
        let mut x = left - self.spacing * 0.5;
        for (i, column) in self.columns.iter().enumerate() {
            let range = Rangef::new(column.min, f32::INFINITY);
            let width = &mut self.state.widths[i];
            if i == 0 {
                // It never hides what it holds.
                *width = width.max(self.first_used);
            }
            *width = range.clamp(*width);
            x += *width + self.spacing;
            if i == 0 && column.auto && self.measuring {
                *width = range.clamp(self.first_used);
                continue;
            }
            if seen.contains(x) {
                in_sight = Some((in_sight.map_or(i, |(first, _)| first), i + 1));
            } else if !before.contains(&i) && pressed != Some(i) {
                continue;
            }

            let (mut p0, mut p1) = (pos2(x, top), pos2(x, bottom));
            let grip = Rect::from_min_max(p0, p1).expand(radius);
            let response = ui.interact(
                grip,
                id.with("resize_column").with(i),
                Sense::click_and_drag(),
            );
            if response.is_pointer_button_down_on() || response.dragged() {
                self.state.pressed = Some(i);
            }
            if response.dragged() {
                if let Some(pointer) = ui.ctx().pointer_latest_pos() {
                    let mut new = *width + pointer.x - x;
                    if i == 0 {
                        // A little each frame, so that contents that give way can.
                        new = new.max(self.first_used - 8.0);
                    }
                    let new = range.clamp(new);
                    let moved = x - *width + new;
                    (p0.x, p1.x) = (moved, moved);
                    *width = new;
                }
            }
            let busy = ui.input(|i| i.pointer.any_down() || i.pointer.any_pressed());
            let hover = response.hovered() && !busy;
            if hover || response.dragged() {
                ui.set_cursor_icon(CursorIcon::ResizeColumn);
            }
            let widgets = &ui.visuals().widgets;
            let stroke = if response.dragged() {
                widgets.active.bg_stroke
            } else if hover {
                widgets.hovered.bg_stroke
            } else {
                widgets.noninteractive.bg_stroke
            };
            ui.painter().line_segment([p0, p1], stroke);
        }
        self.state.borders = in_sight.map_or(0..0, |(first, end)| first..end);
    }
}

#[cfg(test)]
mod tests {
    use egui::{Context, Event, Modifiers, PointerButton, Pos2, RawInput, Vec2};

    use super::*;

    const SIZE: Vec2 = vec2(640.0, 320.0);

    struct Shown {
        /// The columns drawn in each row, the header first.
        rows: Vec<Vec<usize>>,
        ids: Vec<((usize, usize), Id)>,
        /// Each header cell's rectangle.
        header: Vec<(usize, Rect)>,
        content_width: f32,
    }

    /// One frame of a table of `columns` columns and 1,000 rows, scrolled `x` to the side.
    fn frame(
        ctx: &Context,
        columns: usize,
        x: f32,
        keep: Option<usize>,
        events: Vec<Event>,
    ) -> Shown {
        let input = RawInput {
            events,
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SIZE)),
            ..Default::default()
        };
        let mut shown = Shown {
            rows: Vec::new(),
            ids: Vec::new(),
            header: Vec::new(),
            content_width: 0.0,
        };
        let mut out = ctx.run_ui(input, |ui| {
            let area = ScrollArea::horizontal()
                .horizontal_scroll_offset(x)
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut widths = vec![Column::new(40.0, 40.0)];
                    widths.resize(columns, Column::new(100.0, 40.0));
                    let body =
                        Table::new(ui, "table", widths)
                            .keep_column(keep)
                            .header(20.0, |row| {
                                let mut drawn = Vec::new();
                                while let Some(c) = row.next_column() {
                                    let cell = row.col(|ui| {
                                        ui.label("Größe");
                                    });
                                    shown.header.push((c, cell.rect));
                                    drawn.push(c);
                                }
                                shown.rows.push(drawn);
                            });
                    body.rows(20.0, 1000, |row| {
                        let mut drawn = Vec::new();
                        while let Some(c) = row.next_column() {
                            let cell = row.col(|ui| {
                                ui.label("x");
                            });
                            shown.ids.push(((row.index(), c), cell.id));
                            drawn.push(c);
                        }
                        shown.rows.push(drawn);
                    });
                });
            shown.content_width = area.content_size.x;
        });
        out.textures_delta.clear();
        shown
    }

    fn settled(ctx: &Context, columns: usize, x: f32, keep: Option<usize>) -> Shown {
        frame(ctx, columns, x, keep, Vec::new());
        frame(ctx, columns, x, keep, Vec::new())
    }

    /// AGENTS.md, rule 2: what is drawn follows the window, not the number of columns.
    #[test]
    fn only_columns_in_sight_get_cells() {
        for columns in [20, 200, 5000] {
            for x in [0.0, 1200.0] {
                let shown = settled(&Context::default(), columns, x, None);
                assert!(shown.rows.len() > 10 && shown.rows.len() < 30);
                for drawn in &shown.rows {
                    assert_eq!(drawn[0], 0, "the first always");
                    assert!(drawn.len() <= 9, "{columns} at {x}: {drawn:?}");
                    assert!(drawn.windows(2).all(|w| w[0] < w[1]), "{drawn:?}");
                }
            }
        }
    }

    /// The columns left out still take their room: the sideways scroll bar is the same.
    #[test]
    fn columns_out_of_sight_keep_their_room() {
        let shown = settled(&Context::default(), 300, 0.0, None);
        let spacing = Context::default().global_style().spacing.item_spacing.x;
        let full = 40.0 + 299.0 * (100.0 + spacing);
        assert!(
            (shown.content_width - full).abs() < 2.0,
            "{}",
            shown.content_width
        );
    }

    /// Every column can be brought into sight, the last included.
    #[test]
    fn every_column_is_reached() {
        let ctx = Context::default();
        let mut seen = std::collections::BTreeSet::new();
        for step in 0..80 {
            let shown = settled(&ctx, 200, step as f32 * 300.0, None);
            seen.extend(shown.rows[1].iter().copied());
        }
        assert_eq!(seen.len(), 200);
    }

    /// A cell keeps its id while other columns come and go beside it, so that its menu, its
    /// editor and a click under way stay with it.
    #[test]
    fn a_cell_keeps_its_id_while_columns_scroll() {
        let ctx = Context::default();
        let at_0 = settled(&ctx, 200, 0.0, None).ids;
        let at_260 = settled(&ctx, 200, 260.0, None).ids;
        let mut both = 0;
        for (cell, id) in &at_260 {
            if let Some((_, before)) = at_0.iter().find(|(c, _)| c == cell) {
                assert_eq!(id, before, "{cell:?}");
                both += 1;
            }
        }
        assert!(both > 20, "{both} in both");
    }

    #[test]
    fn a_kept_column_is_drawn_out_of_sight() {
        let shown = settled(&Context::default(), 200, 0.0, Some(150));
        assert!(shown.rows.iter().all(|drawn| drawn.last() == Some(&150)));
    }

    /// A cell that was in sight is there one frame longer when the view jumps, as hover and
    /// clicks go by the last frame's rectangles.
    #[test]
    fn a_column_outlasts_a_jump_by_a_frame() {
        let ctx = Context::default();
        let moved = vec![Event::PointerMoved(pos2(300.0, 100.0))];
        frame(&ctx, 200, 0.0, None, moved);
        // A cell knows its rectangle from its second frame on.
        let before = settled(&ctx, 200, 0.0, None);
        let under = before
            .header
            .iter()
            .find(|(_, rect)| rect.x_range().contains(300.0))
            .unwrap()
            .0;
        frame(&ctx, 200, 0.0, None, Vec::new());
        let jumped = frame(&ctx, 200, 6000.0, None, Vec::new());
        assert!(
            jumped.rows[1].contains(&under),
            "{under}: {:?}",
            jumped.rows[1]
        );
        let after = frame(&ctx, 200, 6000.0, None, Vec::new());
        let later = frame(&ctx, 200, 6000.0, None, Vec::new());
        assert!(!later.rows[1].contains(&under));
        assert!(after.rows[1].len() < 12, "{:?}", after.rows[1]);
    }

    /// A border is dragged, also far to the side, and followed when it leaves the window.
    #[test]
    fn a_border_far_to_the_side_resizes() {
        for by in [45.0, 700.0] {
            let ctx = Context::default();
            let x = 9000.0;
            let before = settled(&ctx, 200, x, None);
            let &(column, rect) = before
                .header
                .iter()
                .find(|(_, r)| r.left() > 50.0 && r.right() < 500.0)
                .unwrap();
            let gap = ctx.global_style().spacing.item_spacing.x;
            let from = pos2(rect.right() + gap / 2.0, 100.0);
            let button = |pos, pressed| Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let to = from + vec2(by, 0.0);
            for events in [
                vec![Event::PointerMoved(from)],
                vec![button(from, true)],
                vec![Event::PointerMoved(from + vec2(20.0, 0.0))],
                // Several frames past the window's edge: the border is still followed.
                vec![Event::PointerMoved(from + vec2(by * 0.8, 0.0))],
                vec![Event::PointerMoved(from + vec2(by * 0.85, 0.0))],
                vec![Event::PointerMoved(from + vec2(by * 0.9, 0.0))],
                vec![Event::PointerMoved(to)],
                vec![button(to, false)],
            ] {
                frame(&ctx, 200, x, None, events);
            }
            let after = settled(&ctx, 200, x, None);
            let width = after
                .header
                .iter()
                .find(|(c, _)| *c == column)
                .unwrap()
                .1
                .width();
            assert!((width - rect.width() - by).abs() < 3.0, "by {by}: {width}");
        }
    }
}
