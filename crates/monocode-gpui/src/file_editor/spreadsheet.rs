//! A read-only spreadsheet, ported from `SpreadsheetView.tsx`: the sheet's
//! grid as the file draws it, a formula bar for the picked cell and a tab per
//! sheet. Only the cells in view are built, from the scroll offset, so a
//! 250,000-cell sheet scrolls as lightly as a small one.
use super::office::{self, Pictures};
use super::spreadsheet_chart::{self, Scene, parse_hex, readable_on};
use super::*;
use crate::editor_scrollbar::Metrics;
use gpui::{
    Bounds, ClipboardItem, CursorStyle, FontWeight, Hsla, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, ScrollHandle, ScrollWheelEvent, canvas,
    fill, point, relative, size,
};
use monocode_core::office::drawings::SheetDrawing;
use monocode_core::office::xlsx::{
    BorderStyle, CellBorder, HAlign, Sheet, SheetCell, VAlign, Workbook, cell_address, column_name,
};
use std::{cell::Cell, collections::HashMap, rc::Rc};

const HEADER_W: f32 = 44.;
const HEADER_H: f32 = 22.;
/// How far a line of text may spill into empty cells to its right.
const MAX_SPILL_COLS: usize = 24;
const SCROLLBAR: f32 = 8.;
/// Until the grid has been measured, the size it is built for.
const FIRST_VIEWPORT: (f32, f32) = (900., 600.);

/// Offsets where each column or row starts, plus where the last one ends.
pub(super) fn offsets(sizes: &[f64]) -> Vec<f32> {
    let mut out = Vec::with_capacity(sizes.len() + 1);
    out.push(0.);
    let mut total = 0.;
    for size in sizes {
        total += *size as f32;
        out.push(total);
    }
    out
}

/// The index whose span holds `position`, clamped to the grid.
pub(super) fn index_at(starts: &[f32], position: f32) -> usize {
    if starts.len() < 2 {
        return 0;
    }
    let (mut low, mut high) = (0usize, starts.len() - 2);
    while low < high {
        let middle = (low + high).div_ceil(2);
        if starts[middle] <= position {
            low = middle;
        } else {
            high = middle - 1;
        }
    }
    low
}

fn key(row: usize, col: usize) -> u64 {
    row as u64 * 16384 + col as u64
}

fn span(value: Option<u32>) -> usize {
    value.unwrap_or(1).max(1) as usize
}

fn is_merge(cell: &SheetCell) -> bool {
    span(cell.row_span) > 1 || span(cell.col_span) > 1
}

/// Whether a cell's text may run on past its right edge.
pub(super) fn spills(cell: &SheetCell) -> bool {
    let align = cell.style.as_ref().and_then(|s| s.align);
    !cell.text.is_empty()
        && !cell.style.as_ref().is_some_and(|s| s.wrap)
        && !cell.numeric
        && !cell.error
        && matches!(align, None | Some(HAlign::Left))
}

/// A cell, or the merged area it belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Area {
    pub row: usize,
    pub col: usize,
    pub rows: usize,
    pub cols: usize,
}

struct Grid {
    xs: Vec<f32>,
    ys: Vec<f32>,
    frozen_cols: usize,
    frozen_rows: usize,
    index: HashMap<u64, usize>,
    merges: Vec<usize>,
    covered: HashMap<u64, usize>,
    /// A chart's laid-out scene, by drawing.
    scenes: Vec<Option<Arc<Scene>>>,
    selected: (usize, usize),
    left: f32,
    top: f32,
}

impl Grid {
    fn build(sheet: &Sheet) -> Grid {
        let xs = offsets(&sheet.col_widths);
        let ys = offsets(&sheet.row_heights);
        let mut index = HashMap::with_capacity(sheet.cells.len());
        let mut merges = Vec::new();
        let mut covered = HashMap::new();
        for (at, cell) in sheet.cells.iter().enumerate() {
            index.insert(key(cell.row, cell.col), at);
            if is_merge(cell) {
                merges.push(at);
                for r in 0..span(cell.row_span) {
                    for c in 0..span(cell.col_span) {
                        covered.insert(key(cell.row + r, cell.col + c), at);
                    }
                }
            }
        }
        let scenes = sheet
            .drawings
            .iter()
            .flatten()
            .map(|drawing| match drawing {
                SheetDrawing::Chart(chart) => Some(Arc::new(spreadsheet_chart::build(chart))),
                SheetDrawing::Image(_) => None,
            })
            .collect();
        Grid {
            frozen_cols: sheet.frozen_cols.min(xs.len().saturating_sub(1)),
            frozen_rows: sheet.frozen_rows.min(ys.len().saturating_sub(1)),
            xs,
            ys,
            index,
            merges,
            covered,
            scenes,
            selected: (0, 0),
            left: 0.,
            top: 0.,
        }
    }

    fn cols(&self) -> usize {
        self.xs.len().saturating_sub(1)
    }

    fn rows(&self) -> usize {
        self.ys.len().saturating_sub(1)
    }

    fn x_at(&self, col: usize) -> f32 {
        self.xs[col.min(self.cols())]
    }

    fn y_at(&self, row: usize) -> f32 {
        self.ys[row.min(self.rows())]
    }

    fn area(&self, sheet: &Sheet, row: usize, col: usize) -> Area {
        match self.covered.get(&key(row, col)) {
            Some(master) => {
                let cell = &sheet.cells[*master];
                Area {
                    row: cell.row,
                    col: cell.col,
                    rows: span(cell.row_span),
                    cols: span(cell.col_span),
                }
            }
            None => Area {
                row,
                col,
                rows: 1,
                cols: 1,
            },
        }
    }

    fn cell<'a>(&self, sheet: &'a Sheet, row: usize, col: usize) -> Option<&'a SheetCell> {
        self.index.get(&key(row, col)).map(|at| &sheet.cells[*at])
    }
}

/// What the grid shows of the view's size, in its own pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Layout {
    width: f32,
    height: f32,
    frozen_w: f32,
    frozen_h: f32,
    body_w: f32,
    body_h: f32,
    max_left: f32,
    max_top: f32,
}

fn layout(grid: &Grid, width: f32, height: f32) -> Layout {
    let frozen_w = grid.x_at(grid.frozen_cols);
    let frozen_h = grid.y_at(grid.frozen_rows);
    let body_w = (width - HEADER_W - frozen_w).max(0.);
    let body_h = (height - HEADER_H - frozen_h).max(0.);
    Layout {
        width,
        height,
        frozen_w,
        frozen_h,
        body_w,
        body_h,
        max_left: (grid.x_at(grid.cols()) - frozen_w - body_w).max(0.),
        max_top: (grid.y_at(grid.rows()) - frozen_h - body_h).max(0.),
    }
}

#[derive(Clone, Copy)]
enum Bar {
    Vertical,
    Horizontal,
}

struct Pane {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    origin_x: f32,
    origin_y: f32,
    cols: (usize, usize),
    rows: (usize, usize),
    /// The frozen pane's dividing line.
    edge: Option<Edge>,
}

#[derive(Clone, Copy)]
enum Edge {
    Right,
    Bottom,
}

struct Quad {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    color: Hsla,
}

pub(super) struct SpreadsheetView {
    path: String,
    workbook: Arc<Workbook>,
    active: usize,
    grids: Vec<Option<Grid>>,
    show_formulas: bool,
    focus: FocusHandle,
    viewport: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Every picture of every sheet, in sheet order; `picture_base` is where
    /// each sheet's start.
    pictures: Pictures,
    picture_base: Vec<usize>,
    formats: Vec<Option<ImageFormat>>,
    tabs: ScrollHandle,
    drag: Option<(Bar, f32)>,
}

impl SpreadsheetView {
    pub(super) fn new(path: &str, workbook: Workbook, cx: &mut Context<Self>) -> Self {
        let workbook = Arc::new(workbook);
        let mut base = Vec::new();
        let mut formats = Vec::new();
        let mut sources = Vec::new();
        for sheet in &workbook.sheets {
            base.push(formats.len());
            for drawing in sheet.drawings.iter().flatten() {
                let format = match drawing {
                    SheetDrawing::Image(image) => office::sniff(&image.bytes),
                    SheetDrawing::Chart(_) => None,
                };
                if let (Some(format), SheetDrawing::Image(image)) = (format, drawing) {
                    sources.push((formats.len(), image.bytes.clone(), format));
                }
                formats.push(format);
            }
        }
        office::decode(
            sources,
            |this: &mut SpreadsheetView, at, image| this.pictures.set(at, image),
            cx,
        );
        cx.on_release(|this, cx| this.pictures.release(cx)).detach();
        let mut this = Self {
            path: path.to_string(),
            grids: workbook.sheets.iter().map(|_| None).collect(),
            workbook,
            active: 0,
            show_formulas: false,
            focus: cx.focus_handle(),
            viewport: Rc::default(),
            pictures: Pictures::new(formats.len()),
            picture_base: base,
            formats,
            tabs: ScrollHandle::new(),
            drag: None,
        };
        this.ensure_grid();
        this
    }

    fn ensure_grid(&mut self) {
        if let Some(sheet) = self.workbook.sheets.get(self.active)
            && self.grids[self.active].is_none()
        {
            self.grids[self.active] = Some(Grid::build(sheet));
        }
    }

    fn viewport_size(&self) -> (f32, f32) {
        self.viewport.get().map_or(FIRST_VIEWPORT, |b| {
            (b.size.width.as_f32(), b.size.height.as_f32())
        })
    }

    fn select_sheet(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.workbook.sheets.len() || index == self.active {
            return;
        }
        self.active = index;
        self.ensure_grid();
        self.tabs.scroll_to_item(index);
        cx.notify();
    }

    pub(super) fn scroll_by(&mut self, dx: f32, dy: f32, cx: &mut Context<Self>) {
        let (width, height) = self.viewport_size();
        let Some(Some(grid)) = self.grids.get_mut(self.active) else {
            return;
        };
        let lay = layout(grid, width, height);
        let (left, top) = (
            (grid.left + dx).clamp(0., lay.max_left),
            (grid.top + dy).clamp(0., lay.max_top),
        );
        if (left, top) != (grid.left, grid.top) {
            grid.left = left;
            grid.top = top;
            cx.notify();
        }
    }

    /// Scroll so a cell outside the frozen panes is fully in view.
    fn reveal(&mut self, target: Area) {
        let (width, height) = self.viewport_size();
        let Some(Some(grid)) = self.grids.get_mut(self.active) else {
            return;
        };
        let lay = layout(grid, width, height);
        if target.col >= grid.frozen_cols {
            let left = grid.x_at(target.col) - lay.frozen_w;
            let right = grid.x_at(target.col + target.cols) - lay.frozen_w;
            if left < grid.left {
                grid.left = left;
            } else if right > grid.left + lay.body_w {
                grid.left = left.min(right - lay.body_w);
            }
        }
        if target.row >= grid.frozen_rows {
            let top = grid.y_at(target.row) - lay.frozen_h;
            let bottom = grid.y_at(target.row + target.rows) - lay.frozen_h;
            if top < grid.top {
                grid.top = top;
            } else if bottom > grid.top + lay.body_h {
                grid.top = top.min(bottom - lay.body_h);
            }
        }
        grid.left = grid.left.clamp(0., lay.max_left);
        grid.top = grid.top.clamp(0., lay.max_top);
    }

    fn pick(&mut self, row: usize, col: usize, cx: &mut Context<Self>) {
        let Some(sheet) = self.workbook.sheets.get(self.active) else {
            return;
        };
        let Some(Some(grid)) = self.grids.get_mut(self.active) else {
            return;
        };
        grid.selected = (row, col);
        let area = grid.area(sheet, row, col);
        self.reveal(area);
        cx.notify();
    }

    fn mouse_down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        let (Some(bounds), Some(Some(grid))) = (self.viewport.get(), self.grids.get(self.active))
        else {
            return;
        };
        let (width, height) = self.viewport_size();
        let lay = layout(grid, width, height);
        let x = (e.position.x - bounds.origin.x).as_f32() - HEADER_W;
        let y = (e.position.y - bounds.origin.y).as_f32() - HEADER_H;
        let col =
            (x >= 0.).then(|| index_at(&grid.xs, if x < lay.frozen_w { x } else { x + grid.left }));
        let row =
            (y >= 0.).then(|| index_at(&grid.ys, if y < lay.frozen_h { y } else { y + grid.top }));
        // A header picks the first cell of its row or column.
        if row.is_none() && col.is_none() {
            return;
        }
        self.pick(row.unwrap_or(0), col.unwrap_or(0), cx);
    }

    fn key_down(&mut self, e: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let m = &e.keystroke.modifiers;
        let command = m.control || m.platform;
        let key = e.keystroke.key.as_str();
        if command && key == "`" {
            self.show_formulas = !self.show_formulas;
            cx.notify();
            cx.stop_propagation();
            return;
        }
        let Some(sheet) = self.workbook.sheets.get(self.active) else {
            return;
        };
        let Some(Some(grid)) = self.grids.get(self.active) else {
            return;
        };
        let (row, col) = grid.selected;
        let area = grid.area(sheet, row, col);
        if command && key == "c" {
            let text = grid
                .cell(sheet, area.row, area.col)
                .map(|cell| cell.text.clone())
                .unwrap_or_default();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            cx.stop_propagation();
            return;
        }
        let (_, height) = self.viewport_size();
        let page = (((height - HEADER_H) / 20.).floor() as isize - 1).max(1);
        let (dr, dc): (isize, isize) = match key {
            "up" => (-1, 0),
            "down" => (1, 0),
            "left" => (0, -1),
            "right" => (0, 1),
            "enter" => (if m.shift { -1 } else { 1 }, 0),
            "tab" => (0, if m.shift { -1 } else { 1 }),
            "pageup" => (-page, 0),
            "pagedown" => (page, 0),
            "home" => (0, isize::MIN),
            "end" => (0, isize::MAX),
            _ => return,
        };
        // Leave a merged area from its far edge.
        let next = |at: usize, step: isize, size: usize, current: usize| -> usize {
            if step > 0 {
                at.saturating_add(size.saturating_add(step as usize - 1))
            } else if step < 0 {
                at.saturating_sub(step.unsigned_abs())
            } else {
                current
            }
        };
        let new_row = next(area.row, dr, area.rows, row).min(grid.rows().saturating_sub(1));
        let new_col = if dc == isize::MIN {
            0
        } else if dc == isize::MAX {
            grid.cols().saturating_sub(1)
        } else {
            next(area.col, dc, area.cols, col).min(grid.cols().saturating_sub(1))
        };
        self.pick(new_row, new_col, cx);
        cx.stop_propagation();
    }

    fn wheel(&mut self, e: &ScrollWheelEvent, _: &mut Window, cx: &mut Context<Self>) {
        let delta = e.delta.pixel_delta(px(20.));
        let (mut dx, mut dy) = (delta.x.as_f32(), delta.y.as_f32());
        if e.modifiers.shift && dx == 0. {
            (dx, dy) = (dy, 0.);
        }
        self.scroll_by(-dx, -dy, cx);
        cx.stop_propagation();
    }

    fn drag_move(&mut self, e: &MouseMoveEvent, cx: &mut Context<Self>) {
        let (Some((bar, offset)), Some(bounds)) = (self.drag, self.viewport.get()) else {
            return;
        };
        if !e.dragging() {
            self.drag = None;
            return;
        }
        let (width, height) = self.viewport_size();
        let Some(Some(grid)) = self.grids.get(self.active) else {
            return;
        };
        let lay = layout(grid, width, height);
        let total_w = grid.x_at(grid.cols()) - lay.frozen_w;
        let total_h = grid.y_at(grid.rows()) - lay.frozen_h;
        let (left, top) = (grid.left, grid.top);
        match bar {
            Bar::Vertical => {
                let metrics = Metrics::new(top, total_h, lay.body_h, lay.body_h);
                let y = (e.position.y - bounds.origin.y).as_f32() - HEADER_H - lay.frozen_h;
                let wanted = metrics.scroll_from_pointer(y, offset);
                self.scroll_by(0., wanted - top, cx);
            }
            Bar::Horizontal => {
                let metrics = Metrics::new(left, total_w, lay.body_w, lay.body_w);
                let x = (e.position.x - bounds.origin.x).as_f32() - HEADER_W - lay.frozen_w;
                let wanted = metrics.scroll_from_pointer(x, offset);
                self.scroll_by(wanted - left, 0., cx);
            }
        }
    }

    fn formula_bar(&self, sheet: &Sheet, grid: &Grid, cx: &mut Context<Self>) -> AnyElement {
        let (row, col) = grid.selected;
        let area = grid.area(sheet, row, col);
        let cell = grid.cell(sheet, area.row, area.col);
        let name = if area.rows > 1 || area.cols > 1 {
            format!(
                "{}:{}",
                cell_address(area.row, area.col),
                cell_address(area.row + area.rows - 1, area.col + area.cols - 1)
            )
        } else {
            cell_address(area.row, area.col)
        };
        let on = self.show_formulas;
        div()
            .flex_shrink_0()
            .h(px(32.))
            .flex()
            .items_center()
            .border_b_1()
            .border_color(content(0.1))
            .text_size(px(12.))
            .child(
                div()
                    .id("sheet-name-box")
                    .debug_selector(|| "sheet-name-box".into())
                    .flex_shrink_0()
                    .w(px(80.))
                    .h_full()
                    .flex()
                    .items_center()
                    .px_2()
                    .border_r_1()
                    .border_color(content(0.1))
                    .font_family(crate::fonts::mono())
                    .text_size(px(11.))
                    .text_color(content(0.7))
                    .child(name),
            )
            .child(
                div()
                    .id("sheet-show-formulas")
                    .role(Role::Button)
                    .aria_label(if on {
                        "Show results in the cells (Ctrl+`)"
                    } else {
                        "Show formulas in the cells (Ctrl+`)"
                    })
                    .cursor_pointer()
                    .mx_1()
                    .px_1p5()
                    .italic()
                    .text_size(px(12.))
                    .text_color(if on { accent_color() } else { content(0.4) })
                    .when(on, |d| d.bg(accent_color().opacity(0.15)))
                    .when(!on, |d| {
                        d.hover(|s| s.bg(content(0.08)).text_color(content(1.)))
                    })
                    .child("fx")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.show_formulas = !this.show_formulas;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("sheet-formula-input")
                    .debug_selector(|| "sheet-formula-input".into())
                    .flex_1()
                    .min_w_0()
                    .pr_2()
                    .truncate()
                    .text_color(content(1.))
                    .child(cell.map(|c| c.input.clone()).unwrap_or_default()),
            )
            .child(div().pr_1p5().child(office::bar_actions(&self.path, cx)))
            .into_any_element()
    }

    fn column_headers(
        &self,
        grid: &Grid,
        pick: &Area,
        left: f32,
        width: f32,
        origin_x: f32,
        cols: (usize, usize),
    ) -> AnyElement {
        let mut strip = div()
            .absolute()
            .top_0()
            .left(px(left))
            .w(px(width))
            .h(px(HEADER_H))
            .overflow_hidden()
            .bg(background_base());
        for col in cols.0..=cols.1.min(grid.cols().saturating_sub(1)) {
            let size = grid.xs[col + 1] - grid.xs[col];
            if size <= 0. {
                continue;
            }
            let on = col >= pick.col && col < pick.col + pick.cols;
            strip = strip.child(
                div()
                    .absolute()
                    .top_0()
                    .left(px(grid.xs[col] - origin_x))
                    .w(px(size))
                    .h(px(HEADER_H))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_b_1()
                    .border_color(content(0.12))
                    .text_size(px(11.))
                    .text_color(content(if on { 1. } else { 0.5 }))
                    .when(on, |d| d.bg(content(0.1)))
                    .child(column_name(col)),
            );
        }
        strip.into_any_element()
    }

    fn row_headers(
        &self,
        grid: &Grid,
        pick: &Area,
        top: f32,
        height: f32,
        origin_y: f32,
        rows: (usize, usize),
    ) -> AnyElement {
        let mut strip = div()
            .absolute()
            .left_0()
            .top(px(top))
            .w(px(HEADER_W))
            .h(px(height))
            .overflow_hidden()
            .bg(background_base());
        for row in rows.0..=rows.1.min(grid.rows().saturating_sub(1)) {
            let size = grid.ys[row + 1] - grid.ys[row];
            if size <= 0. {
                continue;
            }
            let on = row >= pick.row && row < pick.row + pick.rows;
            strip = strip.child(
                div()
                    .absolute()
                    .left_0()
                    .top(px(grid.ys[row] - origin_y))
                    .w(px(HEADER_W))
                    .h(px(size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .border_r_1()
                    .border_b_1()
                    .border_color(content(0.12))
                    .text_size(px(11.))
                    .font_features(crate::right_panel::tabular())
                    .text_color(content(if on { 1. } else { 0.5 }))
                    .when(on, |d| d.bg(content(0.1)))
                    .child((row + 1).to_string()),
            );
        }
        strip.into_any_element()
    }

    /// One pane of the sheet, clipped to its box: gridlines, then fills and
    /// borders, then text, which may spill over empty neighbours as in Excel,
    /// then the pictures and charts and the picked cell's outline.
    fn pane(
        &self,
        sheet: &Sheet,
        grid: &Grid,
        pick: &Area,
        p: &Pane,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if p.width <= 0. || p.height <= 0. || p.cols.1 < p.cols.0 || p.rows.1 < p.rows.0 {
            return None;
        }
        let (ox, oy) = (p.origin_x, p.origin_y);
        let mut quads: Vec<Quad> = Vec::new();
        if sheet.gridlines {
            let line = content(0.1);
            for col in p.cols.0..=p.cols.1 {
                if grid.x_at(col + 1) > grid.x_at(col) {
                    quads.push(Quad {
                        x: grid.x_at(col + 1) - ox - 1.,
                        y: 0.,
                        w: 1.,
                        h: p.height,
                        color: line,
                    });
                }
            }
            for row in p.rows.0..=p.rows.1 {
                if grid.y_at(row + 1) > grid.y_at(row) {
                    quads.push(Quad {
                        x: 0.,
                        y: grid.y_at(row + 1) - oy - 1.,
                        w: p.width,
                        h: 1.,
                        color: line,
                    });
                }
            }
        }

        // Merged areas that reach into view, wherever their first cell is.
        let mut shown: Vec<&SheetCell> = grid
            .merges
            .iter()
            .map(|at| &sheet.cells[*at])
            .filter(|cell| {
                cell.row <= p.rows.1
                    && cell.row + span(cell.row_span) > p.rows.0
                    && cell.col <= p.cols.1
                    && cell.col + span(cell.col_span) > p.cols.0
            })
            .collect();
        // Text spills from cells left of the view too.
        let spill_from = p.cols.0.saturating_sub(MAX_SPILL_COLS);
        for row in p.rows.0..=p.rows.1 {
            for col in spill_from..=p.cols.1 {
                if let Some(cell) = grid.cell(sheet, row, col)
                    && !is_merge(cell)
                    && (col >= p.cols.0 || spills(cell))
                {
                    shown.push(cell);
                }
            }
        }

        let mut texts: Vec<AnyElement> = Vec::new();
        for cell in shown {
            let x = grid.x_at(cell.col) - ox;
            let y = grid.y_at(cell.row) - oy;
            let w = grid.x_at(cell.col + span(cell.col_span)) - grid.x_at(cell.col);
            let h = grid.y_at(cell.row + span(cell.row_span)) - grid.y_at(cell.row);
            if w <= 0. || h <= 0. {
                continue;
            }
            let style = cell.style.as_ref();
            let fill_hex = style.and_then(|s| s.fill.as_deref()).and_then(parse_hex);
            let ink_hex = style
                .and_then(|s| s.color.as_deref())
                .and_then(parse_hex)
                .or(fill_hex.map(readable_on));
            let auto_border = match fill_hex {
                Some(fill) => Hsla::from(gpui::rgb(readable_on(fill))),
                None => content(0.75),
            };
            if let Some(fill) = fill_hex {
                quads.push(Quad {
                    x,
                    y,
                    w,
                    h,
                    color: gpui::rgb(fill).into(),
                });
            }
            if let Some(style) = style {
                border_quads(&mut quads, style, (x, y, w, h), auto_border);
            }
            let formula = self.show_formulas && cell.input.starts_with('=');
            let shown_text = if formula { &cell.input } else { &cell.text };
            if shown_text.is_empty() {
                continue;
            }
            let align = if formula {
                HAlign::Left
            } else {
                style
                    .and_then(|s| s.align)
                    .unwrap_or(if cell.numeric || cell.error {
                        HAlign::Right
                    } else {
                        HAlign::Left
                    })
            };
            // Unwrapped text on the left runs on over empty cells.
            let mut room = w;
            if formula || spills(cell) {
                let last = (cell.col + MAX_SPILL_COLS)
                    .min(sheet.col_count)
                    .min(grid.cols());
                for next in cell.col + 1..last {
                    let blocked = grid
                        .cell(sheet, cell.row, next)
                        .is_some_and(|n| !n.text.is_empty())
                        || grid.covered.contains_key(&key(cell.row, next));
                    if blocked {
                        break;
                    }
                    room += grid.x_at(next + 1) - grid.x_at(next);
                }
            }
            let wrap = style.is_some_and(|s| s.wrap) && !formula;
            let (cell_row, cell_col) = (cell.row, cell.col);
            let mut text = div()
                .absolute()
                .debug_selector(move || format!("sheet-cell-{cell_row}-{cell_col}"))
                .left(px(x))
                .top(px(y))
                .w(px(room))
                .h(px(h))
                .overflow_hidden()
                .flex()
                .flex_col()
                .pl(px(
                    3. + style.and_then(|s| s.indent).unwrap_or(0) as f32 * 9.
                ))
                .pr(px(3.))
                .text_size(px(
                    style.and_then(|s| s.size).unwrap_or(11.) as f32 * 4. / 3.
                ))
                .line_height(relative(1.2))
                .text_color(ink_hex.map_or_else(|| content(1.), |c| gpui::rgb(c).into()));
            text = match style.and_then(|s| s.valign) {
                Some(VAlign::Top) => text.justify_start(),
                Some(VAlign::Middle) => text.justify_center(),
                _ => text.justify_end(),
            };
            if style.is_some_and(|s| s.bold) {
                text = text.font_weight(FontWeight::BOLD);
            }
            if !formula && (style.is_some_and(|s| s.italic) || cell.uncalculated) {
                text = text.italic();
            }
            if style.is_some_and(|s| s.underline) {
                text = text.underline();
            }
            if style.is_some_and(|s| s.strike) {
                text = text.line_through();
            }
            if cell.uncalculated && !formula {
                text = text.opacity(0.55);
            }
            let mut line = div().w_full().min_w_0();
            line = match align {
                HAlign::Right => line.text_right(),
                HAlign::Center => line.text_center(),
                _ => line.text_left(),
            };
            if !wrap {
                line = line.whitespace_nowrap();
            }
            texts.push(
                text.child(line.child(shown_text.clone()))
                    .into_any_element(),
            );
        }

        let mut pane = div()
            .absolute()
            .left(px(p.left))
            .top(px(p.top))
            .w(px(p.width))
            .h(px(p.height))
            .overflow_hidden()
            .child(
                canvas(
                    |_, _, _| (),
                    move |bounds: Bounds<Pixels>, _, window, _| {
                        for q in &quads {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(q.x), px(q.y)),
                                    size(px(q.w), px(q.h)),
                                ),
                                q.color,
                            ));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .children(texts);

        let base = self.picture_base.get(self.active).copied().unwrap_or(0);
        for (at, drawing) in sheet.drawings.iter().flatten().enumerate() {
            let (b, _) = match drawing {
                SheetDrawing::Chart(chart) => (chart.bounds, ()),
                SheetDrawing::Image(image) => (image.bounds, ()),
            };
            let (x, y, w, h) = (b.x as f32, b.y as f32, b.width as f32, b.height as f32);
            // Only what reaches into this pane.
            if x - ox > p.width || y - oy > p.height || x + w - ox < 0. || y + h - oy < 0. {
                continue;
            }
            let place = div()
                .absolute()
                .debug_selector(move || format!("sheet-drawing-{at}"))
                .left(px(x - ox))
                .top(px(y - oy))
                .w(px(w))
                .h(px(h));
            match drawing {
                SheetDrawing::Chart(_) => {
                    if let Some(Some(scene)) = grid.scenes.get(at) {
                        pane = pane.child(place.child(spreadsheet_chart::element(scene)));
                    }
                }
                SheetDrawing::Image(image) => {
                    let alt = image
                        .description
                        .clone()
                        .unwrap_or_else(|| image.name.clone());
                    let format = self.formats.get(base + at).copied().flatten();
                    match (self.pictures.get(base + at), format) {
                        (Some(picture), Some(format)) => {
                            let open = office::lightbox(&image.bytes, format, &alt);
                            pane = pane.child(
                                place
                                    .id(("sheet-picture", at))
                                    .aria_label(alt)
                                    .cursor(CursorStyle::PointingHand)
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .on_click(cx.listener(move |_, _, window, cx| {
                                        window.dispatch_action(Box::new(open.clone()), cx);
                                    }))
                                    .child(
                                        office::picture(picture)
                                            .size_full()
                                            .object_fit(ObjectFit::Fill),
                                    ),
                            );
                        }
                        (None, Some(_)) => {}
                        _ => {
                            let kind = if image.extension.is_empty() {
                                "This".to_string()
                            } else {
                                image.extension.to_uppercase()
                            };
                            pane = pane.child(
                                place
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .p_2()
                                    .border_1()
                                    .border_dashed()
                                    .border_color(content(0.25))
                                    .bg(background_base())
                                    .text_size(px(11.))
                                    .text_color(content(0.55))
                                    .text_center()
                                    .child(format!("{kind} picture — open the file to see it")),
                            );
                        }
                    }
                }
            }
        }

        // The picked cell's outline.
        let (px0, py0) = (grid.x_at(pick.col) - ox, grid.y_at(pick.row) - oy);
        pane = pane.child(
            div()
                .absolute()
                .left(px(px0 - 1.))
                .top(px(py0 - 1.))
                .w(px(
                    grid.x_at(pick.col + pick.cols) - grid.x_at(pick.col) + 1.
                ))
                .h(px(
                    grid.y_at(pick.row + pick.rows) - grid.y_at(pick.row) + 1.
                ))
                .border_2()
                .border_color(accent_color()),
        );
        match p.edge {
            Some(Edge::Right) => {
                pane = pane.child(
                    div()
                        .absolute()
                        .top_0()
                        .right_0()
                        .w(px(1.))
                        .h_full()
                        .bg(content(0.25)),
                )
            }
            Some(Edge::Bottom) => {
                pane = pane.child(
                    div()
                        .absolute()
                        .left_0()
                        .bottom_0()
                        .w_full()
                        .h(px(1.))
                        .bg(content(0.25)),
                )
            }
            None => {}
        }
        Some(pane.into_any_element())
    }

    fn scrollbar(
        &self,
        bar: Bar,
        lay: &Layout,
        grid: &Grid,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let (scroll, content_len, view, offset_main) = match bar {
            Bar::Vertical => (
                grid.top,
                grid.y_at(grid.rows()) - lay.frozen_h,
                lay.body_h,
                HEADER_H + lay.frozen_h,
            ),
            Bar::Horizontal => (
                grid.left,
                grid.x_at(grid.cols()) - lay.frozen_w,
                lay.body_w,
                HEADER_W + lay.frozen_w,
            ),
        };
        let metrics = Metrics::new(scroll, content_len, view, view);
        if !metrics.scrollable {
            return None;
        }
        let along = offset_main + metrics.thumb_top;
        let track_start = offset_main;
        let thumb = div()
            .absolute()
            .bg(content(0.25))
            .hover(|s| s.bg(content(0.4)))
            .cursor(CursorStyle::Arrow);
        let thumb = match bar {
            Bar::Vertical => thumb
                .id("sheet-scroll-vertical")
                .left(px(lay.width - SCROLLBAR))
                .top(px(along))
                .w(px(SCROLLBAR))
                .h(px(metrics.thumb_height)),
            Bar::Horizontal => thumb
                .id("sheet-scroll-horizontal")
                .top(px(lay.height - SCROLLBAR))
                .left(px(along))
                .h(px(SCROLLBAR))
                .w(px(metrics.thumb_height)),
        };
        let thumb_top = metrics.thumb_top;
        Some(
            thumb
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                        let Some(bounds) = this.viewport.get() else {
                            return;
                        };
                        // Where on the thumb it was grabbed.
                        let pointer = match bar {
                            Bar::Vertical => (e.position.y - bounds.origin.y).as_f32(),
                            Bar::Horizontal => (e.position.x - bounds.origin.x).as_f32(),
                        };
                        this.drag = Some((bar, pointer - track_start - thumb_top));
                        cx.stop_propagation();
                    }),
                )
                .into_any_element(),
        )
    }
}

/// A cell's borders as quads. The box reaches one pixel up and left, so
/// neighbours share a line.
fn border_quads(
    quads: &mut Vec<Quad>,
    style: &monocode_core::office::xlsx::CellStyle,
    cell: (f32, f32, f32, f32),
    auto: Hsla,
) {
    let (x, y, w, h) = cell;
    let (bx, by, bw, bh) = (x - 1., y - 1., w + 1., h + 1.);
    let color_of = |border: &CellBorder| -> Hsla {
        border
            .color
            .as_deref()
            .and_then(parse_hex)
            .map_or(auto, |c| gpui::rgb(c).into())
    };
    if let Some(b) = &style.top {
        stroke_line(quads, (bx, by), bw, b, true, color_of(b));
    }
    if let Some(b) = &style.bottom {
        stroke_line(
            quads,
            (bx, by + bh - b.width.max(1) as f32),
            bw,
            b,
            true,
            color_of(b),
        );
    }
    if let Some(b) = &style.left {
        stroke_line(quads, (bx, by), bh, b, false, color_of(b));
    }
    if let Some(b) = &style.right {
        stroke_line(
            quads,
            (bx + bw - b.width.max(1) as f32, by),
            bh,
            b,
            false,
            color_of(b),
        );
    }
}

/// One side of a cell: solid, dashed, dotted or double, `width` thick.
fn stroke_line(
    quads: &mut Vec<Quad>,
    at: (f32, f32),
    length: f32,
    border: &CellBorder,
    horizontal: bool,
    color: Hsla,
) {
    let thick = border.width.max(1) as f32;
    let mut piece = |offset: f32, run: f32, across: f32, shift: f32| {
        let (x, y, w, h) = if horizontal {
            (at.0 + offset, at.1 + shift, run, across)
        } else {
            (at.0 + shift, at.1 + offset, across, run)
        };
        quads.push(Quad { x, y, w, h, color });
    };
    match border.style {
        BorderStyle::Solid => piece(0., length, thick, 0.),
        BorderStyle::Double if thick >= 3. => {
            piece(0., length, 1., 0.);
            piece(0., length, 1., thick - 1.);
        }
        BorderStyle::Double => piece(0., length, thick, 0.),
        BorderStyle::Dashed | BorderStyle::Dotted => {
            let (on, off) = if border.style == BorderStyle::Dashed {
                (3. * thick, 2. * thick)
            } else {
                (thick, thick)
            };
            let mut offset = 0.;
            while offset < length {
                piece(offset, on.min(length - offset), thick, 0.);
                offset += on + off;
            }
        }
    }
}

/// `1234567` as `1,234,567`.
pub(super) fn grouped(value: usize) -> String {
    let digits = value.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

impl gpui::Render for SpreadsheetView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.workbook.sheets.is_empty() {
            return div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(12.))
                .text_color(content(0.45))
                .child("This workbook has no sheets.")
                .into_any_element();
        }
        self.ensure_grid();
        let (width, height) = self.viewport_size();
        // Keep the offset inside what the size allows before drawing.
        if let Some(Some(grid)) = self.grids.get_mut(self.active) {
            let lay = layout(grid, width, height);
            grid.left = grid.left.clamp(0., lay.max_left);
            grid.top = grid.top.clamp(0., lay.max_top);
        }
        let workbook = self.workbook.clone();
        let sheet = &workbook.sheets[self.active];
        let Some(Some(grid)) = self.grids.get(self.active) else {
            return div().into_any_element();
        };
        let lay = layout(grid, width, height);
        let (row, col) = grid.selected;
        let pick = grid.area(sheet, row, col);

        let scroll_cols = (
            index_at(&grid.xs, lay.frozen_w + grid.left),
            index_at(&grid.xs, lay.frozen_w + grid.left + lay.body_w),
        );
        let scroll_rows = (
            index_at(&grid.ys, lay.frozen_h + grid.top),
            index_at(&grid.ys, lay.frozen_h + grid.top + lay.body_h),
        );
        let has_cols = grid.frozen_cols > 0;
        let has_rows = grid.frozen_rows > 0;
        let frozen_cols = (0, grid.frozen_cols.saturating_sub(1));
        let frozen_rows = (0, grid.frozen_rows.saturating_sub(1));

        let mut panes: Vec<AnyElement> = Vec::new();
        let mut add = |pane: Option<AnyElement>| panes.extend(pane);
        add(self.pane(
            sheet,
            grid,
            &pick,
            &Pane {
                left: HEADER_W + lay.frozen_w,
                top: HEADER_H + lay.frozen_h,
                width: lay.body_w,
                height: lay.body_h,
                origin_x: lay.frozen_w + grid.left,
                origin_y: lay.frozen_h + grid.top,
                cols: scroll_cols,
                rows: scroll_rows,
                edge: None,
            },
            cx,
        ));
        if has_rows {
            add(self.pane(
                sheet,
                grid,
                &pick,
                &Pane {
                    left: HEADER_W + lay.frozen_w,
                    top: HEADER_H,
                    width: lay.body_w,
                    height: lay.frozen_h,
                    origin_x: lay.frozen_w + grid.left,
                    origin_y: 0.,
                    cols: scroll_cols,
                    rows: frozen_rows,
                    edge: Some(Edge::Bottom),
                },
                cx,
            ));
        }
        if has_cols {
            add(self.pane(
                sheet,
                grid,
                &pick,
                &Pane {
                    left: HEADER_W,
                    top: HEADER_H + lay.frozen_h,
                    width: lay.frozen_w,
                    height: lay.body_h,
                    origin_x: 0.,
                    origin_y: lay.frozen_h + grid.top,
                    cols: frozen_cols,
                    rows: scroll_rows,
                    edge: Some(Edge::Right),
                },
                cx,
            ));
        }
        if has_cols && has_rows {
            add(self.pane(
                sheet,
                grid,
                &pick,
                &Pane {
                    left: HEADER_W,
                    top: HEADER_H,
                    width: lay.frozen_w,
                    height: lay.frozen_h,
                    origin_x: 0.,
                    origin_y: 0.,
                    cols: frozen_cols,
                    rows: frozen_rows,
                    edge: None,
                },
                cx,
            ));
        }
        panes.push(self.column_headers(
            grid,
            &pick,
            HEADER_W + lay.frozen_w,
            lay.body_w,
            lay.frozen_w + grid.left,
            scroll_cols,
        ));
        if has_cols {
            panes.push(self.column_headers(grid, &pick, HEADER_W, lay.frozen_w, 0., frozen_cols));
        }
        panes.push(self.row_headers(
            grid,
            &pick,
            HEADER_H + lay.frozen_h,
            lay.body_h,
            lay.frozen_h + grid.top,
            scroll_rows,
        ));
        if has_rows {
            panes.push(self.row_headers(grid, &pick, HEADER_H, lay.frozen_h, 0., frozen_rows));
        }
        panes.push(
            div()
                .absolute()
                .top_0()
                .left_0()
                .w(px(HEADER_W))
                .h(px(HEADER_H))
                .border_r_1()
                .border_b_1()
                .border_color(content(0.12))
                .bg(background_base())
                .into_any_element(),
        );
        panes.extend(self.scrollbar(Bar::Vertical, &lay, grid, cx));
        panes.extend(self.scrollbar(Bar::Horizontal, &lay, grid, cx));

        let entity = cx.entity();
        let probe = canvas(
            move |bounds: Bounds<Pixels>, _, cx| {
                let known = entity.read(cx).viewport.get();
                if known != Some(bounds) {
                    entity.read(cx).viewport.set(Some(bounds));
                    let entity = entity.clone();
                    cx.defer(move |cx| entity.update(cx, |_, cx| cx.notify()));
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        let grid_view = div()
            .id("sheet-grid")
            .role(Role::Grid)
            .aria_label(format!("{} sheet", sheet.name))
            .debug_selector(|| "sheet-grid".into())
            .track_focus(&self.focus)
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .on_key_down(cx.listener(Self::key_down))
            .on_scroll_wheel(cx.listener(Self::wheel))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| this.drag_move(e, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| this.drag = None),
            )
            .child(probe)
            .children(panes);

        let formula_bar = self.formula_bar(sheet, grid, cx);
        let tabs = (workbook.sheets.len() > 1 || sheet.truncated).then(|| self.tab_bar(sheet, cx));
        div()
            .id("spreadsheet-view")
            .size_full()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(background_base())
            .child(formula_bar)
            .child(grid_view)
            .children(tabs)
            .into_any_element()
    }
}

impl SpreadsheetView {
    fn tab_bar(&self, sheet: &Sheet, cx: &mut Context<Self>) -> AnyElement {
        let tabs = self
            .workbook
            .sheets
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let active = index == self.active;
                div()
                    .id(("sheet-tab", index))
                    .debug_selector(move || format!("sheet-tab-{index}"))
                    .role(Role::Tab)
                    .aria_label(entry.name.clone())
                    .aria_selected(active)
                    .cursor_pointer()
                    .h(px(24.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .px(px(10.))
                    .whitespace_nowrap()
                    .text_color(content(if active { 1. } else { 0.6 }))
                    .when(active, |d| {
                        d.bg(content(0.1)).font_weight(FontWeight::MEDIUM)
                    })
                    .when(!active, |d| {
                        d.hover(|s| s.bg(content(0.06)).text_color(content(1.)))
                    })
                    .child(entry.name.clone())
                    .on_click(cx.listener(move |this, _, _, cx| this.select_sheet(index, cx)))
            });
        let notice = sheet.truncated.then(|| {
            div()
                .flex_shrink_0()
                .px_3()
                .whitespace_nowrap()
                .text_size(px(11.))
                .text_color(content(0.45))
                .child(format!(
                    "Showing the first {} rows · open in Excel for the rest",
                    grouped(sheet.row_count)
                ))
        });
        div()
            .flex_shrink_0()
            .h(px(32.))
            .flex()
            .items_center()
            .border_t_1()
            .border_color(content(0.1))
            .text_size(px(12.))
            .child(
                div()
                    .id("sheet-tabs")
                    .role(Role::TabList)
                    .aria_label("Sheets")
                    .flex_1()
                    .min_w_0()
                    .px_1p5()
                    .overflow_x_scroll()
                    .track_scroll(&self.tabs)
                    .on_scroll_wheel(cx.listener(|this, e: &ScrollWheelEvent, _, cx| {
                        // A wheel turned over the tabs moves them sideways.
                        let delta = e.delta.pixel_delta(px(20.));
                        let by = if delta.x != px(0.) { delta.x } else { delta.y };
                        let mut offset = this.tabs.offset();
                        offset.x = (offset.x + by).clamp(-this.tabs.max_offset().x, px(0.));
                        this.tabs.set_offset(offset);
                        cx.notify();
                        cx.stop_propagation();
                    }))
                    .child(div().flex().items_center().gap_0p5().children(tabs)),
            )
            .children(notice)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use monocode_core::office::xlsx::CellStyle;

    #[test]
    fn offsets_and_lookups_follow_the_sizes() {
        let xs = offsets(&[10., 20., 0., 30.]);
        assert_eq!(xs, [0., 10., 30., 30., 60.]);
        assert_eq!(index_at(&xs, 0.), 0);
        assert_eq!(index_at(&xs, 9.9), 0);
        assert_eq!(index_at(&xs, 10.), 1);
        assert_eq!(index_at(&xs, 29.), 1);
        // A hidden column (width 0) is skipped over.
        assert_eq!(index_at(&xs, 30.), 3);
        assert_eq!(index_at(&xs, 5000.), 3);
        assert_eq!(index_at(&[0.], 5.), 0);
    }

    #[test]
    fn text_spills_only_from_plain_left_aligned_cells() {
        let mut cell = SheetCell {
            text: "hola".into(),
            ..SheetCell::default()
        };
        assert!(spills(&cell));
        cell.numeric = true;
        assert!(!spills(&cell));
        cell.numeric = false;
        cell.style = Some(CellStyle {
            wrap: true,
            ..CellStyle::default()
        });
        assert!(!spills(&cell));
        cell.style = Some(CellStyle {
            align: Some(HAlign::Center),
            ..CellStyle::default()
        });
        assert!(!spills(&cell));
        cell.style = None;
        cell.text.clear();
        assert!(!spills(&cell));
    }

    #[test]
    fn merged_areas_know_their_master_and_extent() {
        let sheet = Sheet {
            name: "S".into(),
            row_count: 5,
            col_count: 5,
            col_widths: vec![50.; 5],
            row_heights: vec![20.; 5],
            cells: vec![SheetCell {
                row: 1,
                col: 1,
                text: "m".into(),
                row_span: Some(2),
                col_span: Some(3),
                ..SheetCell::default()
            }],
            gridlines: true,
            ..Sheet::default()
        };
        let grid = Grid::build(&sheet);
        assert_eq!(
            grid.area(&sheet, 2, 3),
            Area {
                row: 1,
                col: 1,
                rows: 2,
                cols: 3
            }
        );
        assert_eq!(grid.area(&sheet, 0, 0).rows, 1);
        assert_eq!(grid.merges, [0]);
        assert_eq!(layout(&grid, 300., 200.).body_w, 256.);
    }

    #[test]
    fn dashed_borders_break_into_pieces_and_double_ones_into_two() {
        let mut quads = Vec::new();
        let mut border = CellBorder {
            width: 1,
            style: BorderStyle::Dashed,
            color: None,
        };
        stroke_line(&mut quads, (0., 0.), 30., &border, true, content(1.));
        assert!(quads.len() > 3);
        assert!(quads.iter().all(|q| q.h == 1.));
        quads.clear();
        border.style = BorderStyle::Double;
        border.width = 3;
        stroke_line(&mut quads, (0., 0.), 30., &border, true, content(1.));
        assert_eq!(quads.len(), 2);
        assert_eq!(quads[1].y, 2.);
        assert_eq!(grouped(1_234_567), "1,234,567");
        assert_eq!(grouped(12), "12");
    }
}
