//! View > Diagram (`docs/decisions.md`, D36; issue #121): the document's structure in the
//! grid's place.
//!
//! One box per distinct path of element names, with how many elements have it, their
//! attributes and whether they hold text, laid out as a tree from left to right. The
//! structure is read on a thread when the diagram is first shown, and again after a pause
//! when an edit changed the index; the old boxes stay until then.
//!
//! A click on a box selects the first element with its path; clicking it again, or F3, the
//! next one. The box of the element selected anywhere is marked and scrolled into view. A
//! box with children has a toggle that collapses them; each tab keeps what is collapsed.
//! The wheel and dragging pan, Ctrl+wheel zooms and Ctrl+0 goes back to 100 %.

use crate::lang::Lang;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use egui::{
    Color32, CursorIcon, FontId, Key, KeyboardShortcut, Modifiers, Pos2, Rect, RichText, Sense,
    Stroke, StrokeKind, TextStyle, Ui, Vec2, pos2, vec2,
};
use xml_atlas_engine::structure::{MAX_PATHS, PathId, Stopped, Structure};
use xml_atlas_engine::xml::{Index, NodeId};

use crate::app::Open;
use crate::diagram_svg::{Palette, Sizes, to_svg};

/// Shows and hides the diagram.
pub const TOGGLE: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::D);
const NEXT: KeyboardShortcut = KeyboardShortcut::new(Modifiers::NONE, Key::F3);
const RESET_ZOOM: KeyboardShortcut = KeyboardShortcut::new(Modifiers::COMMAND, Key::Num0);

/// Space inside a box around its text.
pub(crate) const PAD: Vec2 = vec2(8.0, 4.0);
/// Between a box and its children's column, room for the toggle and the lines.
pub(crate) const GAP_X: f32 = 44.0;
/// Between two boxes in a column.
const GAP_Y: f32 = 10.0;
/// Radius of a box's collapse toggle.
pub(crate) const TOGGLE_RADIUS: f32 = 5.5;
/// Around the whole diagram.
pub(crate) const MARGIN: f32 = 12.0;
/// Longest name and attribute line a box shows, in characters; the tooltip has all.
const NAME_CHARS: usize = 40;
const DETAIL_CHARS: usize = 60;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 4.0;
/// Below this font size, in pixels, boxes are drawn without text.
const MIN_TEXT_SIZE: f32 = 4.0;

/// One tab's diagram.
pub struct Diagram {
    /// The structure as last read, and the index it was read from.
    structure: Option<Arc<Structure>>,
    index: Weak<Index>,
    /// How long the last walk took.
    took: Duration,
    /// A walk still running.
    job: Option<Job>,
    /// Why the last walk failed.
    failed: Option<String>,
    /// When the index was first seen changed since the structure was read: the walk waits
    /// for a pause in the edits.
    changed: Option<Instant>,
    /// The keys ([`Structure::key`]) of the collapsed boxes: they stay when the structure
    /// is read again.
    collapsed: HashSet<Vec<u8>>,
    /// The depth the *Levels* control last collapsed to; `None` for all, or after a single
    /// box was toggled.
    levels: Option<u16>,
    /// How many of the XPath results each box holds (D36, #123).
    hits: Option<Hits>,
    layout: Option<Layout>,
    /// Counts changes to the structure and to what is collapsed, for the layout.
    generation: u64,
    /// Where the diagram's origin is, from the top left of its area, in pixels.
    pan: Vec2,
    zoom: f32,
    /// The box clicked last: F3 steps through its elements.
    current: Option<PathId>,
    /// The selection the marked box was found for, and that box.
    seen: Option<(Option<NodeId>, usize)>,
    marked: Option<PathId>,
    /// Scroll the marked box into view on the next frame.
    reveal: bool,
    /// *Export as SVG…* was clicked: the file is chosen and written on the next frame.
    export: bool,
    /// What the last export said, shown in the header.
    exported: Option<String>,
}

/// The XPath results counted per box, and what they were counted for.
struct Hits {
    counts: HashMap<PathId, u64>,
    /// The results' list, the structure and the index.
    made_for: (usize, usize, usize, usize),
}

struct Job {
    rx: Receiver<Result<Structure, Stopped>>,
    stop: Arc<AtomicBool>,
    index: Weak<Index>,
    started: Instant,
}

/// The boxes as placed, in units of 100 % zoom.
#[derive(Debug)]
pub(crate) struct Layout {
    /// The visible boxes, a parent before its children.
    pub(crate) boxes: Vec<Placed>,
    pub(crate) size: Vec2,
    /// What it was laid out for: the generation and the two font sizes.
    pub(crate) made_for: (u64, f32, f32),
}

#[derive(Debug)]
pub(crate) struct Placed {
    pub(crate) path: PathId,
    pub(crate) rect: Rect,
    /// The places in [`Layout::boxes`] of the children shown.
    pub(crate) kids: Vec<usize>,
    /// Whether the path has children, shown or collapsed.
    pub(crate) parent: bool,
    pub(crate) collapsed: bool,
    pub(crate) name: String,
    pub(crate) count: String,
    /// The attribute names, `@a @b`, cut to [`DETAIL_CHARS`].
    pub(crate) attributes: String,
    pub(crate) text: bool,
    /// The widths of the name, the count, the attributes and "text" at 100 %: the SVG
    /// export fixes each piece to its width, as another font would measure it differently.
    pub(crate) widths: [f32; 4],
}

/// What the text of a box measures, at 100 %.
trait Measure {
    fn name(&self, text: &str) -> Vec2;
    fn small(&self, text: &str) -> Vec2;
}

/// A click on the diagram.
enum Click {
    Toggle(PathId),
    Box(PathId),
}

impl Default for Diagram {
    fn default() -> Self {
        Self {
            structure: None,
            index: Weak::new(),
            took: Duration::ZERO,
            job: None,
            failed: None,
            changed: None,
            collapsed: HashSet::new(),
            levels: None,
            hits: None,
            layout: None,
            generation: 0,
            pan: Vec2::splat(MARGIN),
            zoom: 1.0,
            current: None,
            seen: None,
            marked: None,
            reveal: false,
            export: false,
            exported: None,
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Diagram {
    /// Whether F3 steps through the elements of a clicked box.
    pub fn has_current(&self) -> bool {
        self.current.is_some() && self.structure.is_some()
    }

    /// Whether the structure was read from `index`.
    fn fresh_for(&self, index: &Arc<Index>) -> bool {
        self.index.as_ptr() == Arc::as_ptr(index) && self.index.strong_count() > 0
    }

    /// Takes a finished walk, and starts one when there is no structure yet or, after a
    /// pause, when the index changed.
    fn refresh(&mut self, ctx: &egui::Context, open_index: &Arc<Index>, src: &Arc<Vec<u8>>) {
        if let Some(job) = &self.job {
            match job.rx.try_recv() {
                Err(TryRecvError::Empty) => return,
                Ok(Ok(s)) => {
                    self.index = job.index.clone();
                    self.took = job.started.elapsed();
                    self.structure = Some(Arc::new(s));
                    self.failed = None;
                    self.generation += 1;
                    self.seen = None;
                }
                Ok(Err(Stopped)) | Err(TryRecvError::Disconnected) => {
                    self.failed = Some("reading the structure stopped".into());
                }
            }
            self.job = None;
        }
        if self.fresh_for(open_index) {
            self.changed = None;
            return;
        }
        if self.structure.is_some() {
            // An edit: wait for a pause, as the references do (REF-4).
            let first = *self.changed.get_or_insert_with(Instant::now);
            let wait = xml_atlas_engine::REPARSE_PAUSE.saturating_sub(first.elapsed());
            if !wait.is_zero() {
                ctx.request_repaint_after(wait);
                return;
            }
        }
        self.changed = None;
        self.start(ctx, open_index, src);
    }

    fn start(&mut self, ctx: &egui::Context, index: &Arc<Index>, src: &Arc<Vec<u8>>) {
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = channel();
        let (index, src, flag, ctx) = (
            Arc::clone(index),
            Arc::clone(src),
            Arc::clone(&stop),
            ctx.clone(),
        );
        let weak = Arc::downgrade(&index);
        let spawned = std::thread::Builder::new()
            .name("structure".into())
            .spawn(move || {
                let t = Instant::now();
                let s = Structure::walk(&src, &index, &flag);
                log::debug!("structure: {:?}", t.elapsed());
                // The tab may be gone; then nobody waits for it.
                let _ = tx.send(s);
                ctx.request_repaint();
            });
        match spawned {
            Ok(_) => {
                self.job = Some(Job {
                    rx,
                    stop,
                    index: weak,
                    started: Instant::now(),
                })
            }
            Err(e) => self.failed = Some(format!("cannot read the structure: {e}")),
        }
    }

    /// Collapses or opens `path`'s children.
    fn toggle(&mut self, path: PathId) {
        let Some(s) = &self.structure else {
            return;
        };
        let key = s.key(path);
        if !self.collapsed.remove(&key) {
            self.collapsed.insert(key);
        }
        self.levels = None;
        self.generation += 1;
    }

    /// Collapses every box below depth `levels` (1 is the root element's), or opens all.
    fn set_levels(&mut self, levels: Option<u16>) {
        let Some(s) = &self.structure else {
            return;
        };
        self.collapsed = match levels {
            None => HashSet::new(),
            Some(n) => (0..s.paths.len() as PathId)
                .filter(|&p| {
                    let info = &s.paths[p as usize];
                    info.depth >= n && !info.children.is_empty()
                })
                .map(|p| s.key(p))
                .collect(),
        };
        self.levels = levels;
        self.generation += 1;
    }

    /// The deepest path's depth.
    fn max_depth(&self) -> u16 {
        self.structure
            .as_ref()
            .and_then(|s| s.paths.iter().map(|p| p.depth).max())
            .unwrap_or(0)
    }

    /// The box shown for `path`: its own, or that of the nearest ancestor shown when it is
    /// collapsed away.
    fn shown(&self, path: PathId) -> Option<usize> {
        let (s, layout) = (self.structure.as_ref()?, self.layout.as_ref()?);
        let mut p = Some(path);
        while let Some(q) = p {
            if let Some(i) = layout.boxes.iter().position(|b| b.path == q) {
                return Some(i);
            }
            p = s.paths.get(q as usize)?.parent;
        }
        None
    }
}

/// F3 while a box was clicked last and neither Find nor XPath takes it: the next element
/// with the box's path. Call before the find bar's shortcuts, which take F3 otherwise.
pub fn shortcuts(ctx: &egui::Context, open: &mut Open) {
    if open.diagram.has_current()
        && let Some(path) = open.diagram.current
        && ctx.input_mut(|i| i.consume_shortcut(&NEXT))
    {
        go(open, path, true);
    }
}

/// The diagram, in the grid's place.
pub fn show(ui: &mut Ui, open: &mut Open, lang: Lang) {
    open.diagram
        .refresh(ui.ctx(), &open.doc.index, &open.doc.src);
    header(ui, lang, &mut open.diagram);
    let Some(structure) = open.diagram.structure.clone() else {
        if let Some(e) = &open.diagram.failed {
            ui.colored_label(ui.visuals().error_fg_color, e);
        } else {
            ui.centered_and_justified(|ui| ui.spinner());
        }
        return;
    };
    if structure.paths.is_empty() {
        ui.centered_and_justified(|ui| ui.label(RichText::new("No elements").weak()));
        return;
    }
    update_hits(open, &structure);
    // Find's outlined match: the box of the element that holds it.
    let found = open
        .mark
        .as_ref()
        .and_then(|m| structure.path_at(&open.doc.src, &open.doc.index, m.start));
    let name_font = TextStyle::Body.resolve(ui.style());
    let small_font = TextStyle::Small.resolve(ui.style());
    let d = &mut open.diagram;
    let made_for = (d.generation, name_font.size, small_font.size);
    if d.layout.as_ref().is_none_or(|l| l.made_for != made_for) {
        let measure = Fonts {
            painter: ui.painter(),
            name: name_font.clone(),
            small: small_font.clone(),
        };
        let mut layout = lay_out(&structure, &d.collapsed, &measure);
        layout.made_for = made_for;
        d.layout = Some(layout);
    }
    if std::mem::take(&mut d.export) {
        export(ui, open.doc.path.as_deref(), d);
    }
    // The marked box: the selected element's, found again when the selection changes.
    let selection = (open.selected, Arc::as_ptr(&open.doc.index) as usize);
    if d.seen != Some(selection) {
        d.marked = open
            .selected
            .and_then(|e| structure.path_of(&open.doc.src, &open.doc.index, e));
        // A click in the diagram selects inside the box clicked: it is in view.
        if d.seen.is_some_and(|(sel, _)| sel != open.selected) {
            d.reveal = true;
        }
        d.seen = Some(selection);
    }

    let (area, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    pan_and_zoom(ui, d, area, &response);
    let marked = d.marked.and_then(|p| d.shown(p));
    let found = found.and_then(|p| d.shown(p));
    if std::mem::take(&mut d.reveal)
        && let Some(i) = marked
    {
        reveal(d, area, i);
    }
    let Some(layout) = &d.layout else {
        return;
    };
    let to_screen = |p: Pos2| area.min + d.pan + p.to_vec2() * d.zoom;
    let hovered = response
        .hover_pos()
        .and_then(|p| hit(layout, (p - area.min - d.pan) / d.zoom));
    let painter = ui.painter_at(area);
    let theme = crate::theme::current(ui);
    let visuals = ui.visuals();
    let line = Stroke::new(1.0, visuals.weak_text_color());
    let border = visuals.widgets.noninteractive.bg_stroke.color;
    let fill = visuals.extreme_bg_color;
    let zoom = d.zoom;
    let screen_rect = |r: Rect| Rect::from_min_max(to_screen(r.min), to_screen(r.max));
    let toggle_center = |r: Rect| to_screen(pos2(r.right() + TOGGLE_RADIUS + 3.0, r.center().y));

    // The lines: from a box to a bar between the columns, and from it to each child.
    for b in &layout.boxes {
        let Some((&first, &last)) = b.kids.first().zip(b.kids.last()) else {
            continue;
        };
        let (top, bottom) = (layout.boxes[first].rect, layout.boxes[last].rect);
        let bar_x = b.rect.right() + GAP_X * 0.6;
        let span = Rect::from_min_max(
            pos2(b.rect.right(), top.center().y.min(b.rect.center().y)),
            pos2(bar_x + GAP_X, bottom.center().y.max(b.rect.center().y)),
        );
        if !area.intersects(screen_rect(span)) {
            continue;
        }
        let from = to_screen(pos2(
            b.rect.right() + 2.0 * TOGGLE_RADIUS + 3.0,
            b.rect.center().y,
        ));
        let bar = to_screen(pos2(bar_x, b.rect.center().y));
        painter.line_segment([from, bar], line);
        painter.line_segment(
            [
                to_screen(pos2(bar_x, top.center().y)),
                to_screen(pos2(bar_x, bottom.center().y)),
            ],
            line,
        );
        for &k in &b.kids {
            let r = layout.boxes[k].rect;
            painter.line_segment(
                [
                    to_screen(pos2(bar_x, r.center().y)),
                    to_screen(pos2(r.left(), r.center().y)),
                ],
                line,
            );
        }
    }

    let name_font = FontId::new(name_font.size * zoom, name_font.family.clone());
    let small_font = FontId::new(small_font.size * zoom, small_font.family.clone());
    for (i, b) in layout.boxes.iter().enumerate() {
        let r = screen_rect(b.rect);
        let with_toggle = r.with_max_x(r.max.x + (2.0 * TOGGLE_RADIUS + 3.0) * zoom);
        if !area.intersects(with_toggle) {
            continue;
        }
        let rounding = 4.0 * zoom;
        let is_marked = marked == Some(i);
        let is_found = found == Some(i) && !is_marked;
        let hits = d.hits.as_ref().and_then(|h| h.counts.get(&b.path)).copied();
        let is_hovered = matches!(hovered, Some(Click::Box(p)) if p == b.path);
        painter.rect_filled(r, rounding, fill);
        if is_marked || is_found || hits.is_some() {
            painter.rect_filled(r, rounding, theme.marked_item);
        }
        let stroke = if is_marked {
            Stroke::new(2.0, visuals.selection.stroke.color)
        } else if is_found || hits.is_some() {
            Stroke::new(2.0, theme.marked_item_outline)
        } else if is_hovered {
            visuals.widgets.hovered.bg_stroke
        } else {
            Stroke::new(1.0, border)
        };
        painter.rect_stroke(r, rounding, stroke, StrokeKind::Inside);
        if name_font.size >= MIN_TEXT_SIZE {
            let at = r.min + PAD * zoom;
            let name = painter.layout_no_wrap(b.name.clone(), name_font.clone(), theme.syntax.tag);
            let name_size = name.size();
            painter.galley(at, name, theme.syntax.tag);
            let count = painter.layout_no_wrap(
                b.count.clone(),
                small_font.clone(),
                visuals.weak_text_color(),
            );
            let count_at = pos2(
                at.x + name_size.x + 6.0 * zoom,
                at.y + name_size.y - count.size().y,
            );
            painter.galley(count_at, count, visuals.weak_text_color());
            let mut x = at.x;
            let y = at.y + name_size.y + 1.0 * zoom;
            if !b.attributes.is_empty() {
                let attrs = painter.layout_no_wrap(
                    b.attributes.clone(),
                    small_font.clone(),
                    theme.syntax.attribute_name,
                );
                x += attrs.size().x + 6.0 * zoom;
                painter.galley(pos2(at.x, y), attrs, theme.syntax.attribute_name);
            }
            if b.text {
                painter.text(
                    pos2(x, y),
                    egui::Align2::LEFT_TOP,
                    "text",
                    small_font.clone(),
                    theme.syntax.text,
                );
            }
        }
        if let Some(n) = hits
            && name_font.size >= MIN_TEXT_SIZE
        {
            // Under the box's lower edge, right-aligned: how many results are in it.
            let text = format!("{} found", grouped(n));
            let galley =
                painter.layout_no_wrap(text, small_font.clone(), visuals.strong_text_color());
            let pad = vec2(4.0, 1.0) * zoom;
            let size = galley.size() + pad * 2.0;
            let badge =
                Rect::from_min_size(pos2(r.right() - size.x, r.bottom() + 2.0 * zoom), size);
            painter.rect_filled(badge, 3.0 * zoom, theme.marked_item_outline);
            painter.galley(badge.min + pad, galley, visuals.strong_text_color());
        }
        if b.parent {
            let c = toggle_center(b.rect);
            let radius = TOGGLE_RADIUS * zoom;
            let hot = matches!(hovered, Some(Click::Toggle(p)) if p == b.path);
            let color = if hot {
                visuals.strong_text_color()
            } else {
                visuals.weak_text_color()
            };
            painter.circle(c, radius, fill, Stroke::new(1.0, color));
            let arm = radius * 0.55;
            painter.line_segment(
                [c - vec2(arm, 0.0), c + vec2(arm, 0.0)],
                Stroke::new(1.2, color),
            );
            if b.collapsed {
                painter.line_segment(
                    [c - vec2(0.0, arm), c + vec2(0.0, arm)],
                    Stroke::new(1.2, color),
                );
            }
        }
    }

    let response = match &hovered {
        Some(_) => response.on_hover_cursor(CursorIcon::PointingHand),
        None => response,
    };
    if let Some(Click::Box(p)) = &hovered {
        let hits = d.hits.as_ref().and_then(|h| h.counts.get(p)).copied();
        response
            .clone()
            .on_hover_ui_at_pointer(|ui| tooltip(ui, &structure, *p, hits));
    }
    if response.clicked() {
        match hovered {
            Some(Click::Toggle(p)) => d.toggle(p),
            Some(Click::Box(p)) => {
                let again = d.current == Some(p);
                d.current = Some(p);
                go(open, p, again);
            }
            None => {}
        }
    }
}

/// Counts the XPath results per box, again only when the results, the structure or the
/// index are others than the counts were made for (#123).
fn update_hits(open: &mut Open, structure: &Arc<Structure>) {
    let index = Arc::as_ptr(&open.doc.index) as usize;
    let key = |items: &[xml_atlas_engine::xpath::Item]| {
        (
            items.as_ptr() as usize,
            items.len(),
            Arc::as_ptr(structure) as usize,
            index,
        )
    };
    let Some(items) = crate::xpath_bar::result_items(open) else {
        open.diagram.hits = None;
        return;
    };
    let made_for = key(items);
    if open
        .diagram
        .hits
        .as_ref()
        .is_some_and(|h| h.made_for == made_for)
    {
        return;
    }
    let idx = &open.doc.index;
    let elements = items.iter().filter_map(|item| {
        use xml_atlas_engine::xpath::Item;
        match *item {
            Item::Attribute(e, _) | Item::Namespace(e, _) | Item::Folded(e) => Some(e),
            // A text node, comment or instruction is in its parent element.
            Item::Node(n) if n == xml_atlas_engine::xml::ROOT => None,
            Item::Node(n) if idx.node(n).kind == xml_atlas_engine::xml::NodeKind::Element => {
                Some(n)
            }
            Item::Node(n) => Some(idx.node(n).parent),
        }
    });
    let counts = structure.count_by_path(&open.doc.src, idx, elements);
    open.diagram.hits = Some(Hits { counts, made_for });
}

/// Asks for a file name and writes the diagram as laid out, in the window's colours.
fn export(ui: &Ui, document: Option<&std::path::Path>, d: &mut Diagram) {
    let Some(layout) = &d.layout else {
        return;
    };
    let visuals = ui.visuals();
    let theme = crate::theme::current(ui);
    let palette = Palette {
        background: visuals.panel_fill,
        fill: visuals.extreme_bg_color,
        border: visuals.widgets.noninteractive.bg_stroke.color,
        line: visuals.weak_text_color(),
        name: theme.syntax.tag,
        weak: visuals.weak_text_color(),
        attributes: theme.syntax.attribute_name,
        text: theme.syntax.text,
    };
    let sizes = Sizes {
        name: TextStyle::Body.resolve(ui.style()).size,
        small: TextStyle::Small.resolve(ui.style()).size,
    };
    let svg = to_svg(layout, &palette, &sizes);
    let stem = document
        .and_then(|p| p.file_stem())
        .map_or("diagram".into(), |s| s.to_string_lossy().into_owned());
    let mut dialog = rfd::FileDialog::new()
        .add_filter("SVG image", &["svg"])
        .set_file_name(format!("{stem}-structure.svg"));
    if let Some(dir) = document.and_then(|p| p.parent()) {
        dialog = dialog.set_directory(dir);
    }
    let Some(path) = dialog.save_file() else {
        return;
    };
    d.exported = Some(match xml_atlas_engine::save_atomic(&path, svg.as_bytes()) {
        Ok(()) => format!("saved {}", path.display()),
        Err(e) => format!("could not save {}: {e}", path.display()),
    });
}

/// What the diagram shows, above it.
fn header(ui: &mut Ui, lang: Lang, d: &mut Diagram) {
    ui.horizontal(|ui| {
        if d.job.is_some() {
            ui.spinner();
            ui.label(RichText::new("reading the structure…").weak());
        } else if let Some(s) = &d.structure {
            let paths = match s.paths.len() {
                1 => "1 path".to_string(),
                n => format!("{} paths", grouped(n as u64)),
            };
            let ms = d.took.as_secs_f64() * 1000.0;
            ui.label(RichText::new(format!("{paths}  ·  {ms:.0} ms")).weak());
            if s.capped {
                ui.colored_label(
                    ui.visuals().warn_fg_color,
                    format!(
                        "the first {} paths: deeper elements are left out",
                        grouped(MAX_PATHS as u64)
                    ),
                );
            }
            if d.changed.is_some() {
                ui.label(RichText::new("out of date after the edit").weak());
            }
            levels_control(ui, d);
            if ui
                .add_enabled(d.layout.is_some(), egui::Button::new("Export as SVG…"))
                .on_hover_text("The diagram as it is collapsed now, as a vector image")
                .clicked()
            {
                d.export = true;
            }
            if let Some(note) = &d.exported {
                ui.label(RichText::new(note).weak());
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reset = crate::keys::text(ui.ctx(), lang, &RESET_ZOOM);
            let wheel = if ui.ctx().os().is_mac() {
                "Cmd"
            } else {
                "Ctrl"
            };
            ui.label(
                RichText::new(format!(
                    "{:.0} %  ·  {wheel}+wheel zooms, {reset} resets",
                    d.zoom * 100.0
                ))
                .weak(),
            );
        });
    });
}

/// *Levels*: collapses the diagram below a depth (#122); the toggles open single boxes again.
fn levels_control(ui: &mut Ui, d: &mut Diagram) {
    let deepest = d.max_depth();
    if deepest < 2 {
        return;
    }
    let shown = match (d.levels, d.collapsed.is_empty()) {
        (Some(n), _) => n.to_string(),
        (None, true) => "all".to_string(),
        (None, false) => "custom".to_string(),
    };
    let mut choice = None;
    egui::ComboBox::from_id_salt("diagram-levels")
        .selected_text(format!("Levels: {shown}"))
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(d.levels.is_none() && d.collapsed.is_empty(), "All")
                .clicked()
            {
                choice = Some(None);
            }
            // Showing every level is "all": the last level has nothing below it.
            for n in 1..deepest {
                if ui
                    .selectable_label(d.levels == Some(n), n.to_string())
                    .clicked()
                {
                    choice = Some(Some(n));
                }
            }
        });
    if let Some(levels) = choice {
        d.set_levels(levels);
    }
}

/// The wheel and dragging pan; Ctrl+wheel and pinching zoom around the pointer; Ctrl+0
/// goes back to 100 % at the top left.
fn pan_and_zoom(ui: &Ui, d: &mut Diagram, area: Rect, response: &egui::Response) {
    if response.dragged() {
        d.pan += response.drag_delta();
    }
    if response.hovered() {
        let (scroll, zoom, reset) = ui.input_mut(|i| {
            (
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.consume_shortcut(&RESET_ZOOM),
            )
        });
        d.pan += scroll;
        if reset {
            d.zoom = 1.0;
            d.pan = Vec2::splat(MARGIN);
        } else if zoom != 1.0 {
            let new = (d.zoom * zoom).clamp(MIN_ZOOM, MAX_ZOOM);
            let at = response.hover_pos().unwrap_or(area.center()) - area.min;
            let unit = (at - d.pan) / d.zoom;
            d.pan = at - unit * new;
            d.zoom = new;
        }
    }
    // Some of the diagram stays in view.
    if let Some(layout) = &d.layout {
        let size = layout.size * d.zoom;
        let keep = 40.0;
        d.pan.x = d
            .pan
            .x
            .clamp(keep - size.x, (area.width() - keep).max(keep - size.x));
        d.pan.y = d
            .pan
            .y
            .clamp(keep - size.y, (area.height() - keep).max(keep - size.y));
    }
}

/// Pans so that box `i` is in view, with a margin.
fn reveal(d: &mut Diagram, area: Rect, i: usize) {
    let Some(layout) = &d.layout else {
        return;
    };
    let r = layout.boxes[i].rect;
    let (min, max) = (
        r.min.to_vec2() * d.zoom + d.pan,
        r.max.to_vec2() * d.zoom + d.pan,
    );
    let room = area.size() - Vec2::splat(MARGIN);
    if min.x < MARGIN {
        d.pan.x += MARGIN - min.x;
    } else if max.x > room.x {
        d.pan.x -= (max.x - room.x).min(min.x - MARGIN);
    }
    if min.y < MARGIN {
        d.pan.y += MARGIN - min.y;
    } else if max.y > room.y {
        d.pan.y -= (max.y - room.y).min(min.y - MARGIN);
    }
}

/// Selects the first element with `path`, or with `again` the one after the selected
/// element, going round.
fn go(open: &mut Open, path: PathId, again: bool) {
    let Some(s) = open.diagram.structure.clone() else {
        return;
    };
    // Typing may have left the index behind the text (D19): read it again first.
    open.settle();
    let (src, index) = (&open.doc.src, &open.doc.index);
    let after = open
        .selected
        .filter(|&e| again && s.path_of(src, index, e) == Some(path));
    if let Some(e) = s.next(src, index, path, after) {
        open.select(e);
        // In view already: the box was clicked, or F3 was pressed on it.
        open.diagram.seen = Some((Some(e), Arc::as_ptr(&open.doc.index) as usize));
        open.diagram.marked = Some(path);
    }
}

/// The box or toggle at `p`, in units of 100 % zoom.
fn hit(layout: &Layout, p: Vec2) -> Option<Click> {
    let p = p.to_pos2();
    layout.boxes.iter().find_map(|b| {
        let c = pos2(b.rect.right() + TOGGLE_RADIUS + 3.0, b.rect.center().y);
        if b.parent && c.distance(p) <= TOGGLE_RADIUS + 2.0 {
            Some(Click::Toggle(b.path))
        } else if b.rect.contains(p) {
            Some(Click::Box(b.path))
        } else {
            None
        }
    })
}

/// A box's whole path, count and attributes.
fn tooltip(ui: &mut Ui, s: &Structure, path: PathId, hits: Option<u64>) {
    let info = &s.paths[path as usize];
    ui.label(RichText::new(format!("/{}", String::from_utf8_lossy(&s.key(path)))).strong());
    ui.label(format!(
        "{} {}",
        grouped(info.count),
        if info.count == 1 {
            "element"
        } else {
            "elements"
        }
    ));
    if !info.attributes.is_empty() {
        let names: Vec<String> = info
            .attributes
            .iter()
            .map(|a| format!("@{}", String::from_utf8_lossy(a)))
            .collect();
        ui.label(names.join(" "));
    }
    if info.text {
        ui.label("holds text");
    }
    if let Some(n) = hits {
        ui.label(format!("{} found by the XPath query", grouped(n)));
    }
    ui.label(RichText::new("Click: the first one; again or F3: the next").weak());
}

/// Places the boxes not collapsed away: each box's children in the column to its right,
/// stacked, and the box level with the middle of them where there is room. A loop rather
/// than recursion: a recursive document has paths [`MAX_PATHS`] deep.
fn lay_out(s: &Structure, collapsed: &HashSet<Vec<u8>>, measure: &dyn Measure) -> Layout {
    let mut boxes: Vec<Placed> = Vec::new();
    // Each depth's column: its boxes' widest width.
    let mut widths: Vec<f32> = Vec::new();
    // Depth first, a parent before its children, measuring each box.
    let mut todo: Vec<(PathId, Option<usize>)> = s.roots.iter().rev().map(|&r| (r, None)).collect();
    let mut depth_of: Vec<usize> = Vec::new();
    while let Some((p, parent)) = todo.pop() {
        let info = &s.paths[p as usize];
        let parent_box = !info.children.is_empty();
        let is_collapsed = parent_box && collapsed.contains(&s.key(p));
        let name = cut(&String::from_utf8_lossy(&info.name), NAME_CHARS);
        let count = format!("\u{d7}{}", grouped(info.count));
        let attributes = cut(
            &info
                .attributes
                .iter()
                .map(|a| format!("@{}", String::from_utf8_lossy(a)))
                .collect::<Vec<_>>()
                .join(" "),
            DETAIL_CHARS,
        );
        let line1 = measure.name(&name) + vec2(6.0 + measure.small(&count).x, 0.0);
        let mut line2 = Vec2::ZERO;
        if !attributes.is_empty() {
            line2 = measure.small(&attributes);
        }
        if info.text {
            let t = measure.small("text");
            line2 = vec2(
                line2.x + if line2.x > 0.0 { 6.0 } else { 0.0 } + t.x,
                t.y.max(line2.y),
            );
        }
        let size = vec2(line1.x.max(line2.x), line1.y + line2.y + 1.0) + PAD * 2.0;
        let depth = parent.map_or(0, |i| depth_of[i] + 1);
        if widths.len() <= depth {
            widths.push(0.0);
        }
        widths[depth] = widths[depth].max(size.x);
        let i = boxes.len();
        boxes.push(Placed {
            path: p,
            rect: Rect::from_min_size(Pos2::ZERO, size),
            kids: Vec::new(),
            parent: parent_box,
            collapsed: is_collapsed,
            widths: [
                measure.name(&name).x,
                measure.small(&count).x,
                if attributes.is_empty() {
                    0.0
                } else {
                    measure.small(&attributes).x
                },
                if info.text {
                    measure.small("text").x
                } else {
                    0.0
                },
            ],
            attributes,
            text: info.text,
            name,
            count,
        });
        depth_of.push(depth);
        if let Some(parent) = parent {
            boxes[parent].kids.push(i);
        }
        if !is_collapsed {
            todo.extend(info.children.iter().rev().map(|&c| (c, Some(i))));
        }
    }
    let mut columns = vec![0.0f32; widths.len()];
    for d in 1..widths.len() {
        columns[d] = columns[d - 1] + widths[d - 1] + GAP_X;
    }
    // A box is placed after its children: an explicit stack of (box, children placed, where
    // its subtree starts). `next_y` is where the next leaf goes.
    let mut next_y = 0.0f32;
    let mut size = Vec2::ZERO;
    let mut stack: Vec<(usize, bool, f32)> = (0..boxes.len())
        .filter(|&i| depth_of[i] == 0)
        .map(|i| (i, false, 0.0))
        .rev()
        .collect();
    while let Some((i, entered, top)) = stack.pop() {
        if !entered && !boxes[i].kids.is_empty() {
            stack.push((i, true, next_y));
            stack.extend(boxes[i].kids.iter().rev().map(|&k| (k, false, 0.0)));
            continue;
        }
        let h = boxes[i].rect.height();
        let y = match (boxes[i].kids.first(), boxes[i].kids.last()) {
            (Some(&a), Some(&b)) => {
                let middle = (boxes[a].rect.center().y + boxes[b].rect.center().y) / 2.0;
                let wanted = middle - h / 2.0;
                if wanted >= top {
                    wanted
                } else {
                    // A box taller than its children cannot rise above where they start, so
                    // they move down with everything below them, until its line meets
                    // theirs (#166). Its subtree follows it in `boxes`.
                    let by = top - wanted;
                    for k in i + 1..boxes.len() {
                        if depth_of[k] <= depth_of[i] {
                            break;
                        }
                        let moved = boxes[k].rect.translate(vec2(0.0, by));
                        boxes[k].rect = moved;
                        size = size.max(moved.max.to_vec2() + vec2(GAP_X, GAP_Y));
                    }
                    next_y += by;
                    top
                }
            }
            _ => next_y,
        };
        let x = columns[depth_of[i]];
        boxes[i].rect = Rect::from_min_size(pos2(x, y), boxes[i].rect.size());
        next_y = next_y.max(y + h + GAP_Y);
        size = size.max(boxes[i].rect.max.to_vec2() + vec2(GAP_X, GAP_Y));
    }
    Layout {
        boxes,
        size,
        made_for: (0, 0.0, 0.0),
    }
}

/// `n` with a comma between thousands: `63,543`.
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// `s` cut to `max` characters, with `…` where it was cut.
fn cut(s: &str, max: usize) -> String {
    match s.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s.to_string(),
    }
}

/// Measures with egui's fonts.
struct Fonts<'a> {
    painter: &'a egui::Painter,
    name: FontId,
    small: FontId,
}

impl Measure for Fonts<'_> {
    fn name(&self, text: &str) -> Vec2 {
        self.painter
            .layout_no_wrap(text.to_string(), self.name.clone(), Color32::WHITE)
            .size()
    }

    fn small(&self, text: &str) -> Vec2 {
        self.painter
            .layout_no_wrap(text.to_string(), self.small.clone(), Color32::WHITE)
            .size()
    }
}

#[cfg(test)]
mod tests {
    use xml_atlas_engine::{Document, Edit};

    use super::*;

    /// Every character 7 wide, names 16 high and small text 12.
    struct Fixed;

    impl Measure for Fixed {
        fn name(&self, text: &str) -> Vec2 {
            vec2(7.0 * text.chars().count() as f32, 16.0)
        }
        fn small(&self, text: &str) -> Vec2 {
            vec2(6.0 * text.chars().count() as f32, 12.0)
        }
    }

    const ORDERS: &str = "\u{feff}<orders>\r\n\
        <order id=\"1\"><customer><name>Jörg</name><email>j@x</email></customer>\
        <items><item sku=\"a\"><price>1</price></item><item/></items></order>\r\n\
        <order id=\"2\" status=\"open\"><items><item/></items><note>größer</note></order>\r\n\
        </orders>";

    fn opened(src: &str) -> Open {
        Open::new(
            Document::from_bytes(src.as_bytes().to_vec()),
            egui::Id::unique("diagram"),
        )
    }

    /// Reads the structure as showing the diagram does, and waits for the thread.
    fn walk(open: &mut Open) {
        let ctx = egui::Context::default();
        open.diagram.changed = Some(Instant::now() - Duration::from_secs(10));
        open.diagram.refresh(&ctx, &open.doc.index, &open.doc.src);
        let t = Instant::now();
        while open.diagram.job.is_some() {
            assert!(
                t.elapsed() < Duration::from_secs(10),
                "the walk did not end"
            );
            std::thread::sleep(Duration::from_millis(1));
            open.diagram.refresh(&ctx, &open.doc.index, &open.doc.src);
        }
    }

    fn path(open: &Open, key: &str) -> PathId {
        let s = open.diagram.structure.as_ref().unwrap();
        (0..s.paths.len() as PathId)
            .find(|&p| s.key(p) == key.as_bytes())
            .unwrap()
    }

    fn layout(open: &Open) -> Layout {
        let d = &open.diagram;
        lay_out(d.structure.as_ref().unwrap(), &d.collapsed, &Fixed)
    }

    #[test]
    fn boxes_sit_right_of_their_parents_and_never_overlap() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let l = layout(&open);
        assert_eq!(l.boxes.len(), 9);
        for b in &l.boxes {
            for &k in &b.kids {
                let kid = l.boxes[k].rect;
                assert!(kid.left() >= b.rect.right() + GAP_X - 0.1, "{b:?}");
            }
            for o in &l.boxes {
                assert!(
                    std::ptr::eq(b, o) || !b.rect.expand(-0.5).intersects(o.rect),
                    "{} and {} overlap",
                    b.name,
                    o.name
                );
            }
            assert!(l.size.x >= b.rect.right() && l.size.y >= b.rect.bottom());
        }
        let order = l.boxes.iter().find(|b| b.name == "order").unwrap();
        assert_eq!(order.count, "\u{d7}2");
        assert_eq!(order.attributes, "@id @status");
        let name = l.boxes.iter().find(|b| b.name == "name").unwrap();
        assert!(name.text);
        // A parent sits level with the middle of its children.
        let customer = l.boxes.iter().find(|b| b.name == "customer").unwrap();
        let (a, b) = (customer.kids[0], customer.kids[1]);
        let middle = (l.boxes[a].rect.center().y + l.boxes[b].rect.center().y) / 2.0;
        assert!((customer.rect.center().y - middle).abs() < 0.1);
    }

    /// #166: a box with attributes is taller than a plain child. It cannot rise above where the
    /// child starts, so the child moves down: their lines meet and nothing overlaps. Also with
    /// several children, grandchildren and a following sibling. Non-ASCII names, a byte order
    /// mark and CRLF, as real files have.
    #[test]
    fn a_tall_parent_and_its_shorter_children_share_a_line() {
        let src = "\u{feff}<röot>\r\n<größe a=\"1\"><b/></größe>\r\n\
                   <größe2 a=\"1\"><b/><c><d/></c></größe2>\r\n<last/>\r\n</röot>";
        let mut open = opened(src);
        walk(&mut open);
        let l = layout(&open);
        let mut tall = 0;
        for b in &l.boxes {
            let (Some(&a), Some(&z)) = (b.kids.first(), b.kids.last()) else {
                continue;
            };
            let middle = (l.boxes[a].rect.center().y + l.boxes[z].rect.center().y) / 2.0;
            assert!(
                (b.rect.center().y - middle).abs() < 0.1,
                "{} sits at {}, its children's middle at {middle}",
                b.name,
                b.rect.center().y
            );
            if b.rect.height() > l.boxes[a].rect.height() {
                tall += 1;
            }
        }
        assert!(tall >= 2, "the sample has no tall parent");
        for (i, a) in l.boxes.iter().enumerate() {
            assert!(l.size.x >= a.rect.right() && l.size.y >= a.rect.bottom());
            for b in &l.boxes[i + 1..] {
                assert!(!a.rect.intersects(b.rect), "{} overlaps {}", a.name, b.name);
            }
        }
    }

    #[test]
    fn a_collapsed_box_hides_its_children_and_stays_collapsed_after_an_edit() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let items = path(&open, "orders/order/items");
        open.diagram.toggle(items);
        let l = layout(&open);
        assert_eq!(l.boxes.len(), 7, "item and price are hidden");
        let b = l.boxes.iter().find(|b| b.path == items).unwrap();
        assert!(b.collapsed && b.parent && b.kids.is_empty());
        // An edit: the structure is read again, the box stays collapsed.
        let at = ORDERS.find("<note>").unwrap();
        open.doc
            .apply(Edit::new("Insert").replace(at..at, "<extra/>"))
            .unwrap();
        walk(&mut open);
        assert!(open.diagram.fresh_for(&open.doc.index));
        let l = layout(&open);
        assert_eq!(l.boxes.len(), 8, "the new box shows");
        assert!(l.boxes.iter().any(|b| b.name == "extra"));
        assert!(l.boxes.iter().all(|b| b.name != "price"));
        // Open again.
        let items = path(&open, "orders/order/items");
        open.diagram.toggle(items);
        assert_eq!(layout(&open).boxes.len(), 10);
    }

    #[test]
    fn levels_collapse_below_a_depth_and_toggles_open_single_boxes() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        assert_eq!(open.diagram.max_depth(), 5);
        let names = |open: &Open| -> Vec<String> {
            layout(open).boxes.iter().map(|b| b.name.clone()).collect()
        };
        open.diagram.set_levels(Some(1));
        assert_eq!(names(&open), ["orders"]);
        open.diagram.set_levels(Some(2));
        assert_eq!(names(&open), ["orders", "order"]);
        open.diagram.set_levels(Some(3));
        assert_eq!(layout(&open).boxes.len(), 5, "customer, items, note");
        open.diagram.set_levels(Some(4));
        assert_eq!(layout(&open).boxes.len(), 8, "name, email and item too");
        // One box opened again: the control no longer says a level.
        open.diagram.set_levels(Some(2));
        let order = path(&open, "orders/order");
        open.diagram.toggle(order);
        assert_eq!(open.diagram.levels, None);
        assert_eq!(layout(&open).boxes.len(), 5);
        // The levels stay after an edit reads the structure again.
        let at = ORDERS.find("<note>").unwrap();
        open.doc
            .apply(Edit::new("Insert").replace(at..at, "<extra/>"))
            .unwrap();
        walk(&mut open);
        open.diagram.set_levels(None);
        assert!(open.diagram.collapsed.is_empty());
        assert_eq!(layout(&open).boxes.len(), 10);
    }

    #[test]
    fn xpath_results_are_counted_in_their_boxes_and_go_with_the_query() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let s = open.diagram.structure.clone().unwrap();
        crate::xpath_bar::run_for_test(&mut open, "//item");
        update_hits(&mut open, &s);
        let item = path(&open, "orders/order/items/item");
        let hits = &open.diagram.hits.as_ref().unwrap().counts;
        assert_eq!((hits.len(), hits[&item]), (1, 3));
        // Attributes and text count for their element.
        crate::xpath_bar::run_for_test(&mut open, "//@sku | //name/text()");
        update_hits(&mut open, &s);
        let hits = &open.diagram.hits.as_ref().unwrap().counts;
        assert_eq!(hits[&item], 1);
        assert_eq!(hits[&path(&open, "orders/order/customer/name")], 1);
        // A query without nodes marks nothing.
        crate::xpath_bar::run_for_test(&mut open, "count(//item)");
        update_hits(&mut open, &s);
        assert!(open.diagram.hits.is_none());
    }

    #[test]
    fn finds_match_is_in_the_box_of_the_element_holding_it() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let s = open.diagram.structure.clone().unwrap();
        let at = ORDERS.find("größer").unwrap() as u64;
        open.mark = Some(at..at + 7);
        let found = s.path_at(&open.doc.src, &open.doc.index, at);
        assert_eq!(found, Some(path(&open, "orders/order/note")));
    }

    #[test]
    fn the_svg_is_well_formed_has_every_shown_box_and_leaves_collapsed_ones_out() {
        use crate::diagram_svg::{Palette, Sizes, to_svg};
        let mut open = opened(ORDERS);
        walk(&mut open);
        let palette = Palette {
            background: Color32::BLACK,
            fill: Color32::DARK_GRAY,
            border: Color32::GRAY,
            line: Color32::GRAY,
            name: Color32::LIGHT_BLUE,
            weak: Color32::GRAY,
            attributes: Color32::YELLOW,
            text: Color32::WHITE,
        };
        let sizes = Sizes {
            name: 16.0,
            small: 12.0,
        };
        let svg_of = |open: &Open| to_svg(&layout(open), &palette, &sizes);
        let check = |svg: &str| {
            let doc = Document::from_bytes(svg.as_bytes().to_vec());
            assert!(doc.problems().is_empty(), "{:?}\n{svg}", doc.problems());
        };
        let all = svg_of(&open);
        check(&all);
        // One <rect> for the background and one per box.
        assert_eq!(all.matches("<rect ").count(), 1 + 9);
        assert!(all.contains("@id"));
        assert!(all.contains("\u{d7}2"));
        // Collapsed: fewer boxes, and the collapsed one has a plus in its toggle.
        open.diagram.set_levels(Some(2));
        let two = svg_of(&open);
        check(&two);
        assert_eq!(two.matches("<rect ").count(), 1 + 2);
        assert!(
            two.contains("V"),
            "the toggle of a collapsed box has a plus"
        );
    }

    #[test]
    fn a_click_selects_the_first_element_and_again_or_f3_the_next() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let item = path(&open, "orders/order/items/item");
        let (src, index) = (open.doc.src.clone(), open.doc.index.clone());
        let items: Vec<NodeId> = (1..index.len() as NodeId)
            .filter(|&i| index.name(&src, i) == b"item")
            .collect();
        go(&mut open, item, false);
        assert_eq!(open.selected, Some(items[0]));
        go(&mut open, item, true);
        assert_eq!(open.selected, Some(items[1]));
        // F3, once the box was clicked.
        open.diagram.current = Some(item);
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::F3,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }],
            ..Default::default()
        };
        let mut out = ctx.run_ui(input, |ui| shortcuts(ui.ctx(), &mut open));
        out.textures_delta.clear();
        assert_eq!(open.selected, Some(items[2]));
        go(&mut open, item, true);
        assert_eq!(open.selected, Some(items[0]), "round to the first");
        // A click on another box starts at its first element.
        let note = path(&open, "orders/order/note");
        go(&mut open, note, false);
        assert_eq!(index.name(&src, open.selected.unwrap()), b"note");
    }

    #[test]
    fn the_selected_elements_box_is_marked_or_its_collapsed_ancestors() {
        let mut open = opened(ORDERS);
        walk(&mut open);
        let ctx = egui::Context::default();
        let price = path(&open, "orders/order/items/item/price");
        go(&mut open, price, false);
        // Shown once so the layout and the mark are made.
        let mut out = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut open, crate::lang::Lang::En)
        });
        out.textures_delta.clear();
        let d = &open.diagram;
        assert_eq!(d.marked, Some(price));
        let i = d.shown(price).unwrap();
        assert_eq!(d.layout.as_ref().unwrap().boxes[i].path, price);
        let items = path(&open, "orders/order/items");
        open.diagram.toggle(items);
        let mut out = ctx.run_ui(Default::default(), |ui| {
            show(ui, &mut open, crate::lang::Lang::En)
        });
        out.textures_delta.clear();
        let d = &open.diagram;
        let i = d.shown(price).unwrap();
        assert_eq!(d.layout.as_ref().unwrap().boxes[i].path, items);
    }

    #[test]
    fn a_deep_recursive_document_is_laid_out_without_recursion() {
        let depth = MAX_PATHS + 10;
        let src = format!("{}{}", "<s>".repeat(depth), "</s>".repeat(depth));
        let mut open = opened(&src);
        walk(&mut open);
        let l = layout(&open);
        assert_eq!(l.boxes.len(), MAX_PATHS);
        assert!(
            l.boxes
                .windows(2)
                .all(|w| w[1].rect.left() > w[0].rect.right())
        );
    }

    #[test]
    fn numbers_are_grouped_and_long_names_cut() {
        assert_eq!(grouped(0), "0");
        assert_eq!(grouped(999), "999");
        assert_eq!(grouped(1000), "1,000");
        assert_eq!(grouped(63_543), "63,543");
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(cut("größer", 3), "grö…");
        assert_eq!(cut("abc", 3), "abc");
    }
}
