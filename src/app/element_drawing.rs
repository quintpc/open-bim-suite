//! Element drawings: a drawing of one modelled element on a sheet of its own, as a workshop
//! makes its elements from (a precast or CLT wall, a timber frame, a beam, a column, a floor
//! panel).
//!
//! - **A wall, a framed system, a beam, a column**: an elevation that looks square at its side
//!   and no deeper than the element is thick, cropped to it.
//! - **A slab**: a plan of its level, cropped to it.
//! - **Dimensions**: its length and its height (a slab's two sizes), and for a wall with
//!   openings a chain along its foot through their sides and one up through the first one's
//!   sill and head.
//! - **Reinforcement**: of a reinforced beam or column, its bars and stirrups in the elevation
//!   and beside it its section with the bars in it and what they are, each with its mark in the
//!   bending schedule ("(2) 3Ø16", "(3) Ø8-150"). Of a slab or a wall with nets, as a Dutch
//!   reinforcement drawing has them: of every row of equal bars one bar drawn (of the bottom
//!   net a whole line, of the top net a dashed one), the line it is spread along with a dot
//!   where the two cross, and its mark, diameter, spacing and net ("(1) Ø10-150 ondernet").
//!   Such a drawing is named a reinforcement drawing, and its text block lists the bars.
//! - **Alone**: the other elements of the model are hidden in the view.
//! - **A text block** under it: its name, its type or profile, its sizes in millimetres, its
//!   strength class and its volume.
//! - **The sheet**: A3, named after the element, the view at the largest usual scale that fits.
//!
//! The view and the sheet are ordinary ones: they follow the model and can be changed as any.

use super::App;
use crate::document::{PlanKind, SectionKind, ShapeKind, NOTE_HEIGHT};
use bim_core::element::PlanarForm;
use bim_core::{Element, PlanarDef, Shape};
use glam::DVec2;
use openaec_ui::Language;

/// The scales an element drawing can have (1 : this), the largest first.
const SCALES: [f64; 6] = [5.0, 10.0, 20.0, 50.0, 100.0, 200.0];
/// How much of an A3 sheet the element itself may take (metres on paper): the rest is for its
/// dimensions, its text block and the sheet's edge.
const ROOM: [f64; 2] = [0.30, 0.17];
/// Between dimension lines, and from the element to the first (metres on paper).
const STEP: f64 = 0.008;

/// What an element drawing shows: where it looks from, and the element's extent in its drawing.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Frame {
    /// An elevation along `from` → `to` looking `depth` deep to its left; `None`: a plan.
    pub side: Option<([f64; 2], [f64; 2], f64)>,
    /// The element in the drawing: lower left and upper right (an elevation's u, v; a plan's x, y).
    pub lo: DVec2,
    pub hi: DVec2,
    /// The sides of its openings along the drawing's first axis, and the first one's sill and head.
    pub across: Vec<f64>,
    pub up: Vec<f64>,
    /// Its sizes for the text block (metres): length, height (a slab's width), thickness.
    pub sizes: [f64; 3],
    pub volume: f64,
}

/// An elevation of what runs from `a` to `b` in plan, `half` to each side of that line and from
/// `low` to `high`: seen from its right, a little outside it.
fn side(a: DVec2, b: DVec2, half: f64, low: f64, high: f64) -> (Option<([f64; 2], [f64; 2], f64)>, DVec2, DVec2) {
    const OUT: f64 = 0.02;
    let along = (b - a).normalize_or_zero();
    let right = -along.perp() * (half + OUT);
    (Some(((a + right).to_array(), (b + right).to_array(), 2.0 * (half + OUT))), DVec2::new(0.0, low), DVec2::new((b - a).length(), high))
}

/// The frame of the drawing of `shape` (project coordinates); `None` for what has no drawing of
/// its own (an object, a join, a system that lies).
pub(super) fn frame(shape: &Shape) -> Option<Frame> {
    match shape {
        Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, base, height }, openings, .. }) => {
            let (t, length) = (p.total_thickness(), (*end - *start).length());
            let (side, lo, hi) = side(*start, *end, t / 2.0, *base, base + height);
            let across = openings.iter().flat_map(|o| [o.along, o.along + o.width]).collect();
            let up = openings.first().map_or(Vec::new(), |o| vec![base + o.sill, base + o.sill + o.height]);
            let holes: f64 = openings.iter().map(|o| o.width * o.height).sum();
            Some(Frame { side, lo, hi, across, up, sizes: [length, *height, t], volume: (length * height - holes) * t })
        }
        Shape::Planar(p @ PlanarDef { form: PlanarForm::Slab { outline, .. }, .. }) => {
            let lo = outline.iter().copied().reduce(DVec2::min)?;
            let hi = outline.iter().copied().reduce(DVec2::max)?;
            let area = (0..outline.len()).map(|i| outline[i].perp_dot(outline[(i + 1) % outline.len()])).sum::<f64>().abs() / 2.0;
            let t = p.total_thickness();
            Some(Frame { side: None, lo, hi, across: Vec::new(), up: Vec::new(), sizes: [hi.x - lo.x, hi.y - lo.y, t], volume: area * t })
        }
        Shape::System(s) if !s.lies() => {
            let length = (s.end - s.start).length();
            let t = s.member.max_element();
            let (side, lo, hi) = side(s.start, s.end, t / 2.0, s.base, s.base + s.height);
            let across = s.openings.iter().flat_map(|o| [o.along, o.along + o.width]).collect();
            let up = s.openings.first().map_or(Vec::new(), |o| vec![s.base + o.sill, s.base + o.sill + o.height]);
            Some(Frame { side, lo, hi, across, up, sizes: [length, s.height, t], volume: 0.0 })
        }
        Shape::Linear(l) => {
            let profile = l.profile.profile().ok()?;
            let (min, max) = (profile.outer().iter().copied().reduce(DVec2::min)?, profile.outer().iter().copied().reduce(DVec2::max)?);
            let (width, depth) = (max.x - min.x, max.y - min.y);
            let length = (l.end - l.start).length();
            let volume = profile.area() * length;
            let (a, b) = (l.start.truncate(), l.end.truncate());
            if (b - a).length() < 1e-6 {
                // a column: seen along the project's y, as wide as its profile
                let reach = width.max(0.2) / 2.0;
                let (side, _, _) = side(a - DVec2::X * reach, a + DVec2::X * reach, depth / 2.0, 0.0, 0.0);
                let (low, high) = (l.start.z.min(l.end.z), l.start.z.max(l.end.z));
                return Some(Frame { side, lo: DVec2::new(reach - width / 2.0, low), hi: DVec2::new(reach + width / 2.0, high), across: Vec::new(), up: Vec::new(), sizes: [width, length, depth], volume });
            }
            let z = l.start.z.min(l.end.z);
            let (side, lo, hi) = side(a, b, width / 2.0, z + min.y, l.start.z.max(l.end.z) + max.y);
            Some(Frame { side, lo, hi, across: Vec::new(), up: Vec::new(), sizes: [(b - a).length(), depth, width], volume })
        }
        _ => None,
    }
}

/// The largest scale at which an element of `size` (metres) fits the room a sheet has for it.
pub(super) fn scale_for(size: DVec2) -> f64 {
    SCALES.into_iter().find(|s| size.x / s <= ROOM[0] && size.y / s <= ROOM[1]).unwrap_or(SCALES[SCALES.len() - 1])
}

/// A line of a view's drawing.
type Line = [[f64; 2]; 2];

/// What the drawing of a reinforced member adds to its elevation.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct BarDrawing {
    /// Its bars and its stirrups in the elevation, each a line.
    pub lines: Vec<Line>,
    /// Its section beside the elevation: its outline, its stirrup, and a circle per bar.
    pub shapes: Vec<(ShapeKind, Vec<Line>)>,
    /// What the bars are, beside the section.
    pub notes: Vec<([f64; 2], String)>,
    /// How far all of it reaches to the right.
    pub right: f64,
}

/// The four sides of the rectangle from `lo` to `hi`.
fn rectangle(lo: DVec2, hi: DVec2) -> Vec<Line> {
    let c = [[lo.x, lo.y], [hi.x, lo.y], [hi.x, hi.y], [lo.x, hi.y]];
    (0..4).map(|i| [c[i], c[(i + 1) % 4]]).collect()
}

/// The reinforcement of `e` in its drawing at 1 : `scale`: the bars and stirrups of a level
/// beam or a standing column with a rectangular section, drawn in the elevation `frame`, and
/// its section `gap` to the right of it. `None` for what has none, or lies sloping.
pub(super) fn bar_drawing(e: &Element, shape: &Shape, frame: &Frame, scale: f64, gap: f64, mark: usize) -> Option<BarDrawing> {
    const SEGMENTS: usize = 24;
    let (Some(r), Shape::Linear(l)) = (&e.reinforcement, shape) else { return None };
    let profile = l.profile.profile().ok()?;
    let centres = r.centres(&profile).ok()?;
    let (min, max) = (profile.outer().iter().copied().reduce(DVec2::min)?, profile.outer().iter().copied().reduce(DVec2::max)?);
    let length = (l.end - l.start).length();
    let standing = (l.end - l.start).truncate().length() < 1e-6;
    if !standing && (l.end.z - l.start.z).abs() > 1e-6 {
        return None;
    }
    let (lo, hi) = (frame.lo, frame.hi);
    let leg = r.cover + r.stirrup / 2.0;
    let mut drawing = BarDrawing::default();
    // the bars drawn over a stretch, each once where they lie behind each other
    let mut extra = r.extra_bars(&profile, length, &e.extras);
    extra.dedup_by(|a, b| a.0 == b.0 && ((standing && (a.1.x - b.1.x).abs() < 1e-9) || (!standing && (a.1.y - b.1.y).abs() < 1e-9)));
    let rows = r.rows(&profile, length, &e.extras).ok()?;
    // the levels (a column: the places across) the bars are seen at, each once
    let mut seen: Vec<f64> = centres.iter().map(|(c, _)| if standing { c.x } else { c.y }).collect();
    seen.sort_by(f64::total_cmp);
    seen.dedup_by(|a, b| (*a - *b).abs() < 1e-9);
    // where the section's own axes stand in the drawing: beside the elevation, at its foot
    let origin = DVec2::new(hi.x + gap - min.x, lo.y - min.y);
    if standing {
        let x = lo.x - min.x;
        drawing.lines.extend(seen.iter().map(|at| [[x + at, lo.y + r.cover], [x + at, hi.y - r.cover]]));
        drawing.lines.extend(r.stirrups_in(length, &e.extras).iter().map(|(s, _, _)| [[lo.x + leg, lo.y + s], [hi.x - leg, lo.y + s]]));
        drawing.lines.extend(extra.iter().map(|(_, c, _, from, to)| [[x + c.x, lo.y + from], [x + c.x, lo.y + to]]));
    } else {
        let y = lo.y - min.y;
        drawing.lines.extend(seen.iter().map(|at| [[lo.x + r.cover, y + at], [hi.x - r.cover, y + at]]));
        drawing.lines.extend(r.stirrups_in(length, &e.extras).iter().map(|(s, _, _)| [[lo.x + s, lo.y + leg], [lo.x + s, hi.y - leg]]));
        drawing.lines.extend(extra.iter().map(|(_, c, _, from, to)| [[lo.x + from, y + c.y], [lo.x + to, y + c.y]]));
    }
    drawing.shapes.push((ShapeKind::Rectangle, rectangle(origin + min, origin + max)));
    drawing.shapes.push((ShapeKind::Rectangle, rectangle(origin + min + DVec2::splat(leg), origin + max - DVec2::splat(leg))));
    let drawn: Vec<(DVec2, f64)> = r.extra_bars(&profile, length, &e.extras).iter().map(|b| (b.1, b.2)).collect();
    for (c, d) in centres.iter().chain(&drawn) {
        let point = |i: usize| {
            let a = std::f64::consts::TAU * i as f64 / SEGMENTS as f64;
            (origin + *c + DVec2::new(a.cos(), a.sin()) * d / 2.0).to_array()
        };
        drawing.shapes.push((ShapeKind::Circle, (0..SEGMENTS).map(|i| [point(i), point(i + 1)]).collect()));
    }
    // what they are, beside the section and under each other: the top bars, the stirrups, the
    // bottom bars, about the middle of its height
    let mm = |v: f64| (v * 1000.0).round() as i64;
    let letter = scale * NOTE_HEIGHT;
    let x = origin.x + max.x + gap / 2.0;
    // their marks as the schedule gives them: the top bars, the bottom bars, the usual
    // stirrups (where a stretch has not taken them all), then what is drawn over a stretch
    use bim_core::reinforcement::Extra;
    let stretches: Vec<&Extra> = e.extras.iter().filter(|x| !matches!(x, Extra::Line { .. })).collect();
    let below = mark + usize::from(r.top.count > 0);
    let usual = below + usize::from(r.bottom.count > 0);
    let has_usual = rows.len() > usual - mark + stretches.len();
    let mut texts = Vec::new();
    if r.top.count > 0 {
        texts.push(format!("({mark}) {}Ø{}", r.top.count, mm(r.top.diameter)));
    }
    if has_usual {
        texts.push(format!("({usual}) Ø{}-{}", mm(r.stirrup), mm(r.spacing)));
    }
    if r.bottom.count > 0 {
        texts.push(format!("({below}) {}Ø{}", r.bottom.count, mm(r.bottom.diameter)));
    }
    for (k, stretch) in stretches.iter().enumerate() {
        let m = usual + usize::from(has_usual) + k;
        texts.push(match **stretch {
            Extra::Stirrups { from, to, diameter, spacing } => format!("({m}) Ø{}-{} x = {}-{}", mm(diameter), mm(spacing), mm(from), mm(to)),
            Extra::Bars { from, to, count, diameter, .. } => format!("({m}) {count}Ø{} x = {}-{}", mm(diameter), mm(from), mm(to)),
            Extra::Line { .. } => continue,
        });
    }
    let line = letter * 1.6;
    let top = origin.y + (min.y + max.y) / 2.0 + (texts.len() as f64 / 2.0 - 1.0) * line;
    let longest = texts.iter().map(|t| t.chars().count()).max().unwrap_or(0);
    drawing.notes.extend(texts.into_iter().enumerate().map(|(k, text)| ([x, top - k as f64 * line], text)));
    drawing.right = x + longest as f64 * letter * 0.7;
    Some(drawing)
}

/// The nets of slab or wall `e` in its drawing at 1 : `scale`, as a reinforcement drawing shows
/// them: of every row of equal bars (a row of the schedule, marked on from `mark`) one bar, a
/// whole line for the bottom net and a dashed one for the top net; square to it the line the
/// row is spread along, with a tick at each end and a dot where the two cross; and there its
/// mark, diameter, spacing and net. A line drawn in a slab is such a row too: its bar stands on
/// the line, named a hairpin or extra bars on top or below ("(5) hrsp Ø8-150"). `words`: the
/// bottom net, the top net, hairpins, extra bars, below and on top. A wall has `openings`.
/// `None` for what has no nets, or none that can be made.
pub(super) fn net_drawing(e: &Element, shape: &Shape, frame: &Frame, scale: f64, openings: &[bim_core::element::Opening], mark: usize, words: [&str; 6]) -> Option<BarDrawing> {
    use bim_core::reinforcement::{Extra, NetBar};
    const SEGMENTS: usize = 12;
    // on paper (metres): a dash and the gap after it, half a tick, the dot
    const DASH: (f64, f64) = (0.003, 0.0015);
    const TICK: f64 = 0.0012;
    const DOT: f64 = 0.0007;
    let (Some(n), Shape::Planar(p)) = (&e.nets, shape) else { return None };
    let bars = n.laid(p, openings, &e.extras).ok()?;
    // a slab's bars lie in its plan; a wall's stand along it, above its base
    let lift = match &p.form {
        PlanarForm::Wall { base, .. } => DVec2::new(0.0, *base),
        PlanarForm::Slab { .. } => DVec2::ZERO,
        PlanarForm::Stair { .. } => return None,
    };
    let mm = |v: f64| (v * 1000.0).round() as i64;
    let rows = bim_core::reinforcement::net_groups(&bars);
    let mut drawing = BarDrawing { right: frame.hi.x, ..Default::default() };
    for (k, row) in rows.iter().enumerate() {
        let first = row[0];
        let u = (first.b - first.a).normalize_or_zero();
        let v = u.perp();
        let at = |b: &NetBar| b.a.dot(v);
        let (lo, hi) = row.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), b| (lo.min(at(b)), hi.max(at(b))));
        // where along the bar its spread line crosses, and which bar of the row is drawn: each
        // net and way at a place of its own, so their texts stand apart
        let (along, spread) = match (first.extra, first.top, first.cross) {
            // a line's bars: at the line (a hairpin starts there, a straight bar lies across it)
            (Some(_), _, _) => (if first.drop > 0.0 { 0.0 } else { 0.5 }, 0.5),
            (None, false, false) => (0.2, 0.3),
            (None, false, true) => (0.15, 0.4),
            (None, true, false) => (0.8, 0.7),
            (None, true, true) => (0.85, 0.6),
        };
        let want = lo + (hi - lo) * spread;
        let Some(shown) = row.iter().min_by(|a, b| (at(a) - want).abs().total_cmp(&(at(b) - want).abs())) else { continue };
        let (a, b) = (shown.a + lift, shown.b + lift);
        if first.top && first.drop == 0.0 {
            let (dash, step) = (DASH.0 * scale, (DASH.0 + DASH.1) * scale);
            let length = a.distance(b);
            drawing.lines.extend((0..(length / step).ceil() as usize).map(|i| [(a + u * (i as f64 * step)).to_array(), (a + u * (i as f64 * step + dash).min(length)).to_array()]));
        } else {
            drawing.lines.push([a.to_array(), b.to_array()]);
        }
        let cross = a + (b - a) * along;
        if row.len() > 1 {
            let (from, to) = (cross + v * (lo - at(shown)), cross + v * (hi - at(shown)));
            drawing.lines.push([from.to_array(), to.to_array()]);
            drawing.lines.extend([from, to].map(|end| [(end - u * TICK * scale).to_array(), (end + u * TICK * scale).to_array()]));
        }
        let point = |i: usize| (cross + DVec2::from_angle(std::f64::consts::TAU * i as f64 / SEGMENTS as f64) * DOT * scale).to_array();
        drawing.shapes.push((ShapeKind::Circle, (0..SEGMENTS).map(|i| [point(i), point(i + 1)]).collect()));
        let letter = scale * NOTE_HEIGHT;
        let text = match first.extra.and_then(|x| e.extras.get(x)) {
            Some(Extra::Line { diameter, spacing, .. }) if first.drop > 0.0 => format!("({}) {} Ø{}-{}", mark + k, words[2], mm(*diameter), mm(*spacing)),
            Some(Extra::Line { diameter, spacing, .. }) => format!("({}) {} Ø{}-{} {}", mark + k, words[3], mm(*diameter), mm(*spacing), words[4 + usize::from(first.top)]),
            _ => {
                let net = if first.top { n.top.unwrap_or(n.bottom) } else { n.bottom };
                format!("({}) Ø{}-{} {}", mark + k, mm(net.diameter), mm(net.spacing), words[usize::from(first.top)])
            }
        };
        drawing.notes.push(((cross + DVec2::splat(letter * 0.5)).to_array(), text));
    }
    Some(drawing)
}

impl App {
    /// The lines of the text block of `e`'s drawing.
    fn element_text(&self, e: &Element, shape: &Shape, frame: &Frame, l: Language) -> Vec<String> {
        let lang = super::library::lang(l);
        let kind = match shape {
            Shape::Planar(p) => p.build_up.as_ref().map(|b| self.library.name(&b.id, lang).unwrap_or_else(|| b.id.clone())),
            Shape::System(s) => s.system_type.as_ref().map(|t| self.library.system_name(&t.id, lang).unwrap_or_else(|| t.id.clone())),
            Shape::Linear(m) => Some(m.profile.name()),
            _ => None,
        };
        let mm = |v: f64| format!("{}", (v * 1000.0).round() as i64);
        let mut lines = vec![e.name.clone()];
        lines.extend(kind);
        lines.push(format!("{} × {} × {} mm", mm(frame.sizes[0]), mm(frame.sizes[1]), mm(frame.sizes[2])));
        if let Some(grade) = &e.grade {
            lines.push(format!("{}: {grade}", l.text("Sterkteklasse", "Grade")));
        }
        if frame.volume > 0.0 {
            lines.push(format!("{}: {:.3} m³", l.text("Volume", "Volume"), frame.volume));
        }
        if let Some(Ok(bars)) = super::element_props::bars_of(&self.modeller.model, e) {
            let weight: f64 = bars.iter().map(|b| b.weight()).sum();
            let what = match (&e.reinforcement, &e.nets) {
                (Some(r), _) => super::element_props::bars_text(r),
                (_, Some(n)) => super::element_props::nets_text(n, l.text("onder", "bottom"), l.text("boven", "top")),
                _ => String::new(),
            };
            lines.push(format!("{}: {what}", l.text("Wapening", "Reinforcement")));
            lines.push(format!("{}: {weight:.0} kg", l.text("Wapeningsstaal", "Reinforcing steel")));
            // the bars, as the bending schedule lists them
            let mark = self.first_mark(e.id);
            lines.push(format!("{}:", l.text("Buigstaat", "Bar schedule")));
            for (k, bar) in bars.iter().enumerate() {
                use bim_core::reinforcement::BarShape;
                let form = match bar.shape {
                    BarShape::Stirrup(a, b) => format!("{} {} × {}", l.text("beugel", "stirrup"), mm(a), mm(b)),
                    BarShape::Hairpin(a, b) => format!("{} {} × {}", l.text("haarspeld", "hairpin"), mm(a), mm(b)),
                    BarShape::Straight => l.text("recht", "straight").to_string(),
                };
                lines.push(format!("({}) {} Ø{} L = {} {form}", mark + k, bar.count, mm(bar.diameter), mm(bar.length)));
            }
        }
        lines
    }

    /// Makes the element drawing of the one element selected and shows its sheet: a view of the
    /// element alone with its dimensions and its text block, on a sheet named after it.
    pub(super) fn element_drawing(&mut self, l: Language) {
        let Some(e) = self.defined().cloned() else { return };
        let shape = e.shape_in_project(&self.modeller.model.grid);
        let Some(frame) = frame(&shape) else { return };
        let reinforced = e.reinforcement.is_some() || e.nets.is_some();
        let word = if reinforced { l.text("Wapeningstekening", "Reinforcement drawing") } else { l.text("Elementtekening", "Element drawing") };
        let mut name = format!("{word} {}", e.name);
        let mut n = 1;
        while self.document.sections().iter().any(|s| s.name == name) || self.document.plans().iter().any(|p| p.name == name) {
            n += 1;
            name = format!("{word} {} ({n})", e.name);
        }
        let view = match frame.side {
            Some((from, to, depth)) => self.document.add_section_of(SectionKind::Elevation, &name, from, to, depth),
            None => {
                let level = e.level.clone().or_else(|| self.document.levels().first().map(|l| l.path.clone()));
                let plan = level.and_then(|level| self.document.add_plan_of(&level, PlanKind::Floor, word));
                if let Some(plan) = &plan {
                    self.document.set_view_name(plan, &name);
                }
                plan
            }
        };
        let Some(view) = view else { return };
        // a reinforced member: its section stands beside it, so the scale leaves room for both
        let mut scale = scale_for(frame.hi - frame.lo);
        let mark = self.first_mark(e.id);
        let mut bars = bar_drawing(&e, &shape, &frame, scale, 3.0 * scale * STEP, mark);
        if let Some(b) = &bars {
            scale = scale_for(DVec2::new(b.right - frame.lo.x, frame.hi.y - frame.lo.y));
            bars = bar_drawing(&e, &shape, &frame, scale, 3.0 * scale * STEP, mark);
        }
        // a slab's or a wall's nets
        if bars.is_none() {
            let words = match &shape {
                Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => [l.text("net links", "left net"), l.text("net rechts", "right net"), "", "", "", ""],
                _ => [l.text("ondernet", "bottom net"), l.text("bovennet", "top net"), l.text("hrsp", "hairpins"), l.text("bijleg", "extra"), l.text("onder", "bottom"), l.text("boven", "top")],
            };
            bars = net_drawing(&e, &shape, &frame, scale, &self.modeller.model.openings_of(e.id), mark, words);
        }
        self.document.set_view_scale(&view, scale);
        let step = scale * STEP;
        let (lo, hi) = (frame.lo, frame.hi);
        let right = bars.as_ref().map_or(hi.x, |b| b.right);
        if let Some(b) = &bars {
            self.document.add_view_lines(&view, &b.lines);
            for (kind, lines) in &b.shapes {
                self.document.add_view_shape(&view, *kind, lines);
            }
            for (at, text) in &b.notes {
                self.document.add_note(&view, *at, text);
            }
        }
        // a dimension's line lies to the left of the way it is measured, a step from its points:
        // along the foot it is measured from the end back, so its line lies under the element
        // along its foot: through its openings, then its whole length
        let mut rows = 1.0;
        if !frame.across.is_empty() {
            let mut at = vec![lo.x];
            at.extend(frame.across.iter().map(|a| lo.x + a));
            at.push(hi.x);
            at.sort_by(f64::total_cmp);
            at.dedup_by(|a,b| (*a-*b).abs() < 0.001);
            let points = at.into_iter().rev().map(|x| [x,lo.y]).collect::<Vec<_>>();
            self.document.add_dimension_chain(&view, &points);
            rows = 2.0;
        }
        self.document.add_dimension(&view, [hi.x, lo.y - (rows - 1.0) * step], [lo.x, lo.y - (rows - 1.0) * step]);
        // up its start: through its first opening, then its whole height
        let mut columns = 1.0;
        if !frame.up.is_empty() {
            let mut at = vec![lo.y, hi.y];
            at.extend(&frame.up);
            at.sort_by(f64::total_cmp);
            at.dedup_by(|a,b| (*a-*b).abs() < 0.001);
            let points = at.into_iter().map(|y| [lo.x,y]).collect::<Vec<_>>();
            self.document.add_dimension_chain(&view, &points);
            columns = 2.0;
        }
        self.document.add_dimension(&view, [lo.x - (columns - 1.0) * step, lo.y], [lo.x - (columns - 1.0) * step, hi.y]);
        // its text block under the dimensions
        let line = scale * NOTE_HEIGHT * 1.6;
        let text = self.element_text(&e, &shape, &frame, l);
        let top = lo.y - (rows + 2.0) * step;
        for (k, row) in text.iter().enumerate() {
            self.document.add_note(&view, [lo.x, top - (k + 1) as f64 * line], row);
        }
        let low = top - (text.len() + 1) as f64 * line;
        self.document.set_crop(&view, Some(([lo.x - (columns + 2.0) * step, low], [right + 2.0 * step, hi.y + 2.0 * step])));
        // the element alone: the others of the model are hidden in this view (those drawn later
        // are not, as in any view)
        let entry = if self.models.len() <= 1 { 0 } else { self.models.iter().position(|m| Some(m.model.id) == self.modeller.shown).unwrap_or(0) as u32 };
        let others = self.modeller.model.iter().filter(|o| o.id != e.id).map(|o| crate::units::UnitKey::Element(crate::model::ElementKey { model: entry, source: o.id.0 }));
        self.document.set_view_hidden(&view, std::sync::Arc::new(others.collect()));
        let sheet = self.document.add_sheet(&name);
        if let Some(i) = self.document.place_plan(&sheet, &view) {
            self.document.set_scale(&sheet, i, scale);
            // a placed view stands on the middle of all it cuts: moved so that what the crop
            // keeps is in the middle of the paper
            if let ([item], Some(paper)) = (&self.layout_items(std::slice::from_ref(&view))[..], self.document.sheet(&sheet).map(|s| s.paper)) {
                let at = [paper[0] / 2.0 - (item.lo[0] + item.hi[0]) / 2.0 / scale, paper[1] / 2.0 - (item.lo[1] + item.hi[1]) / 2.0 / scale];
                self.document.put_placed(&sheet, i, at);
            }
        }
        self.tag_plans();
        self.open_doc_sheet(&sheet);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Message;
    use bim_core::{LinearDef, Opening, ProfileDef};
    use glam::DVec3;

    fn wall() -> Shape {
        Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::new(1.0, 2.0), end: DVec2::new(7.0, 2.0), base: 0.0, height: 3.0 },
            thickness: 0.14,
            openings: vec![Opening { along: 2.0, sill: 0.9, width: 1.2, height: 1.5, framing: None }],
            build_up: None,
        })
    }

    #[test]
    fn a_wall_is_seen_from_its_right_no_deeper_than_it_is_thick() {
        let f = frame(&wall()).unwrap();
        let (from, to, depth) = f.side.unwrap();
        // drawn along +x: its right is -y
        assert!((from[1] - (2.0 - 0.09)).abs() < 1e-9 && (to[0] - 7.0).abs() < 1e-9 && (depth - 0.18).abs() < 1e-9, "{f:?}");
        assert_eq!((f.lo, f.hi), (DVec2::new(0.0, 0.0), DVec2::new(6.0, 3.0)));
        assert_eq!((f.across.clone(), f.up.clone()), (vec![2.0, 3.2], vec![0.9, 2.4]));
        assert!((f.volume - (18.0 - 1.8) * 0.14).abs() < 1e-9);
        // 6 m by 3 m fits an A3 sheet at 1 : 20; a 12 m beam needs 1 : 50
        assert_eq!((scale_for(f.hi - f.lo), scale_for(DVec2::new(12.0, 0.4)), scale_for(DVec2::new(0.3, 0.3))), (20.0, 50.0, 5.0));
    }

    #[test]
    fn a_beam_a_column_and_a_slab_have_frames_and_an_object_none() {
        let beam = Shape::Linear(LinearDef { start: DVec3::new(0.0, 0.0, 2.8), end: DVec3::new(0.0, 5.0, 2.8), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0, role: Default::default() });
        let f = frame(&beam).unwrap();
        assert!((f.lo - DVec2::new(0.0, 2.6)).length() < 1e-9 && (f.hi - DVec2::new(5.0, 3.0)).length() < 1e-9 && (f.volume - 0.4).abs() < 1e-9, "{f:?}");
        let column = Shape::Linear(LinearDef { start: DVec3::new(4.0, 4.0, 0.0), end: DVec3::new(4.0, 4.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0, role: Default::default() });
        let f = frame(&column).unwrap();
        assert!((f.hi.x - f.lo.x - 0.3).abs() < 1e-9 && (f.lo.y, f.hi.y) == (0.0, 3.0) && f.sizes == [0.3, 3.0, 0.5], "{f:?}");
        let slab = Shape::Planar(PlanarDef {
            form: PlanarForm::Slab { outline: vec![DVec2::ZERO, DVec2::new(2.4, 0.0), DVec2::new(2.4, 6.0), DVec2::new(0.0, 6.0)], holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: bim_core::SlabRole::Floor, slope: None },
            thickness: 0.2,
            openings: Vec::new(),
            build_up: None,
        });
        let f = frame(&slab).unwrap();
        assert!(f.side.is_none() && (f.volume - 2.88).abs() < 1e-9, "{f:?}");
        let object = Shape::Point(bim_core::PointDef { at: DVec3::ZERO, rotation: 0.0, size: DVec3::ONE, opening: None, mirrored: false, role: Default::default() });
        assert!(frame(&object).is_none());
    }

    #[test]
    fn a_reinforced_beam_is_drawn_with_its_bars_its_stirrups_and_its_section() {
        let mut app = App::default();
        app.new_model("Level 0");
        let beam = Shape::Linear(LinearDef { role: Default::default(), start: DVec3::new(0.0, 0.0, 3.0), end: DVec3::new(6.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 });
        let id = app.add_element("Beam", None, beam).unwrap();
        app.select_modelled(id);
        let _ = app.update(Message::Reinforce(true));
        app.select_modelled(id);
        let _ = app.update(Message::ElementDrawing(Language::English));
        let [view] = app.document.sections() else { panic!() };
        // two levels of bars along it and 40 stirrups up it; then the section: its outline,
        // its stirrup and five bars
        let level = |l: &&Line| (l[0][1] - l[1][1]).abs() < 1e-9;
        let own = &view.lines[..42];
        assert_eq!((own.iter().filter(level).count(), own.iter().filter(|l| !level(l)).count()), (2, 40));
        assert!(own.iter().filter(level).all(|l| ((l[1][0] - l[0][0]).abs() - 5.94).abs() < 1e-9), "bars as long as the beam less its cover");
        assert_eq!(view.shapes.iter().map(|s| s.kind).collect::<Vec<_>>(), [ShapeKind::Rectangle, ShapeKind::Rectangle, ShapeKind::Circle, ShapeKind::Circle, ShapeKind::Circle, ShapeKind::Circle, ShapeKind::Circle]);
        // the section stands to the right of the beam, at its height, and the crop keeps it
        let section = &view.lines[42..46];
        let (left, right) = (section.iter().flatten().map(|p| p[0]).fold(f64::INFINITY, f64::min), section.iter().flatten().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max));
        assert!(left > 6.0 && (right - left - 0.3).abs() < 1e-9, "{left} {right}");
        let (_, high) = view.crop.unwrap();
        assert!(high[0] > right);
        let text: Vec<&str> = view.notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(&text[..3], ["(1) 2Ø12", "(3) Ø8-150", "(2) 3Ø16"]);
        assert!(text.contains(&"(3) 40 Ø8 L = 1488 stirrup 232 × 432") && view.name.starts_with("Reinforcement drawing"), "{text:?}");
        assert!(text.contains(&"Reinforcement: 2Ø12 + 3Ø16, Ø8-150"), "{text:?}");
        // without reinforcement: no lines of its own
        app.select_modelled(id);
        let _ = app.update(Message::Reinforce(false));
        app.select_modelled(id);
        let _ = app.update(Message::ElementDrawing(Language::English));
        assert!(app.document.sections()[1].lines.is_empty());
    }

    #[test]
    fn a_reinforced_column_has_its_bars_upright_and_its_stirrups_across() {
        let column = Shape::Linear(LinearDef { role: Default::default(), start: DVec3::new(4.0, 4.0, 0.0), end: DVec3::new(4.0, 4.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 });
        let mut e = Element::in_project(&bim_core::TileGrid::default(), "Column", None, column.clone());
        e.reinforcement = Some(Default::default());
        let f = frame(&column).unwrap();
        let b = bar_drawing(&e, &column, &f, 20.0, 0.1, 1).unwrap();
        // seen across its width: the bars at 44, 46, 150, 254 and 256 from its side, upright
        let upright: Vec<&Line> = b.lines.iter().filter(|l| (l[0][0] - l[1][0]).abs() < 1e-9).collect();
        assert_eq!(upright.len(), 5);
        assert!(upright.iter().all(|l| (l[0][1] - 0.03).abs() < 1e-9 && (l[1][1] - 2.97).abs() < 1e-9));
        assert_eq!(b.lines.len() - 5, 20, "a stirrup every 150 over 2.94");
        // a sloping member has none drawn, nor one without reinforcement
        let rafter = Shape::Linear(LinearDef { role: Default::default(), start: DVec3::ZERO, end: DVec3::new(4.0, 0.0, 2.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 });
        assert!(bar_drawing(&e, &rafter, &frame(&rafter).unwrap(), 20.0, 0.1, 1).is_none());
        e.reinforcement = None;
        assert!(bar_drawing(&e, &column, &f, 20.0, 0.1, 1).is_none());
    }

    #[test]
    fn the_drawing_of_a_wall_is_an_elevation_with_dimensions_and_a_text_block_on_a_sheet() {
        let mut app = App::default();
        app.new_model("Level 0");
        let id = app.add_element("Wall", None, wall()).unwrap();
        let other = app.add_element("Wall", None, Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(1.0, 2.0), end: DVec2::new(1.0, 6.0), base: 0.0, height: 3.0 }, thickness: 0.14, openings: Vec::new(), build_up: None })).unwrap();
        app.select_modelled(id);
        let _ = app.update(Message::ElementDrawing(Language::English));
        let [view] = app.document.sections() else { panic!() };
        // the other wall is hidden in it, the wall itself not
        let key = |id: bim_core::ElementId| crate::units::UnitKey::Element(crate::model::ElementKey { model: 0, source: id.0 });
        assert!(view.hidden.contains(&key(other)) && !view.hidden.contains(&key(id)));
        // the dimension lines lie outside it: under its foot and left of its start
        assert!(view.dimensions.iter().all(|d| d.from[1] <= 1e-9 || d.from[0] <= 1e-9), "{:?}", view.dimensions);
        assert_eq!((view.name.as_str(), view.kind, view.scale), ("Element drawing Wall 1", SectionKind::Elevation, 20.0));
        // three along its foot through the opening and its length; three up and its height
        let lengths: Vec<i64> = view.dimensions.iter().flat_map(|d| d.intervals()).map(|(_, [a,b])| ((b[0]-a[0]).hypot(b[1]-a[1]) * 1000.).round() as i64).collect();
        assert_eq!(lengths, [2800, 1200, 2000, 6000, 900, 1500, 600, 3000]);
        let text: Vec<&str> = view.notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(text, ["Wall 1", "6000 × 3000 × 140 mm", "Volume: 2.268 m³"]);
        assert!(view.crop.is_some());
        let [sheet] = app.document.sheets() else { panic!() };
        assert_eq!((sheet.name.as_str(), sheet.views.len(), sheet.views[0].scale), ("Element drawing Wall 1", 1, 20.0));
        // a second one of the same element gets a name of its own
        app.select_modelled(id);
        let _ = app.update(Message::ElementDrawing(Language::English));
        assert_eq!(app.document.sections()[1].name, "Element drawing Wall 1 (2)");
    }

    #[test]
    fn a_slab_with_nets_gets_a_reinforcement_drawing_and_rows_in_the_schedule() {
        let mut app = App::default();
        app.new_model("Level 0");
        let p = DVec2::new;
        let slab = PlanarForm::Slab { outline: vec![p(0.0, 0.0), p(6.0, 0.0), p(6.0, 4.0), p(0.0, 4.0)], holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None };
        let level = app.work_plane().2;
        let id = app.add_element("Slab", level, Shape::Planar(PlanarDef { form: slab, thickness: 0.25, openings: Vec::new(), build_up: None })).unwrap();
        app.select_modelled(id);
        let _ = app.update(Message::Reinforce(true));
        assert!(app.modeller.refusal.is_none() && app.modeller.model.get(id).unwrap().nets.is_some());
        // the schedule: the bottom net's bars both ways, then the top net's
        let listed = app.schedules.iter().find(|s| s.name == "Bar schedule").expect("the bar schedule");
        let cells: Vec<Vec<String>> = listed.rows.iter().map(|r| r.cells[1..9].iter().map(|c| c.clone().unwrap_or_default()).collect()).collect();
        assert_eq!(cells, [
            ["1", "bottom net", "10", "27", "straight", "", "", "5940"],
            ["2", "bottom net", "10", "40", "straight", "", "", "3940"],
            ["3", "top net", "8", "27", "straight", "", "", "5940"],
            ["4", "top net", "8", "40", "straight", "", "", "3940"],
        ]);
        app.select_modelled(id);
        let _ = app.update(Message::ElementDrawing(Language::English));
        let view = app.document.plans().iter().find(|v| v.name.starts_with("Reinforcement drawing")).expect("the drawing");
        let text: Vec<&str> = view.notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(&text[..4], ["(1) Ø10-150 bottom net", "(2) Ø10-150 bottom net", "(3) Ø8-150 top net", "(4) Ø8-150 top net"]);
        assert!(text.contains(&"Reinforcement: bottom Ø10-150 #, top Ø8-150 #") && text.contains(&"(2) 40 Ø10 L = 3940 straight"), "{text:?}");
        // a dot per row; the bottom bars whole lines, as long as the slab less its cover
        assert_eq!(view.shapes.iter().filter(|s| s.kind == ShapeKind::Circle).count(), 4);
        let long = |l: &&Line| (DVec2::from(l[0]).distance(DVec2::from(l[1])) - 5.94).abs() < 1e-9;
        assert_eq!(view.lines.iter().filter(long).count(), 1, "the top bar along it is dashed");
        // a top diameter of 0 takes the top net away
        app.select_modelled(id);
        app.type_element_field(crate::app::element_props::ElementField::TopBar, "0".into());
        assert!(app.modeller.model.get(id).unwrap().nets.unwrap().top.is_none());
        assert_eq!(app.schedules.iter().find(|s| s.name == "Bar schedule").unwrap().rows.len(), 2);
    }

    #[test]
    fn a_wall_with_nets_has_its_bars_drawn_in_its_elevation_round_its_opening() {
        let wall = wall();
        let mut e = Element::in_project(&bim_core::TileGrid::default(), "Wall", None, wall.clone());
        e.nets = Some(Default::default());
        let f = frame(&wall).unwrap();
        let Shape::Planar(def) = &wall else { panic!() };
        let d = net_drawing(&e, &wall, &f, 50.0, &def.openings, 5, ["left net", "right net", "", "", "", ""]).unwrap();
        assert!(d.notes.iter().any(|(_, t)| t == "(5) Ø10-150 left net") && d.notes.iter().any(|(_, t)| t.ends_with("right net")), "{:?}", d.notes);
        // all of it stays in the wall: 6 long, 3 high
        assert!(d.lines.iter().flatten().all(|q| q[0] > -0.1 && q[0] < 6.1 && q[1] > -0.1 && q[1] < 3.1));
        // beside the window the bars are short: more rows than the four of a wall without one
        assert!(d.notes.len() > 4, "{}", d.notes.len());
        e.nets = None;
        assert!(net_drawing(&e, &wall, &f, 50.0, &def.openings, 5, [""; 6]).is_none());
    }

    #[test]
    fn the_reinforcement_tool_reinforces_what_is_clicked_and_esc_lets_it_go() {
        let mut app = App::default();
        app.new_model("Level 0");
        let level = app.work_plane().2;
        let beam = Shape::Linear(LinearDef { role: Default::default(), start: DVec3::new(0.0, 0.0, 3.0), end: DVec3::new(6.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0 });
        let id = app.add_element("Beam", level, beam).unwrap();
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = glam::Vec3::ZERO;
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        let _ = app.update(Message::RebarTool(crate::app::rebar::RebarTool::Usual));
        assert!(app.modeller.rebar_tool.is_some() && !app.selecting());
        let (x, y) = app.on_screen(DVec3::new(3.0, 0.0, 3.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Pick { x, y, width: 1000.0, height: 800.0, add: false }));
        assert!(app.modeller.model.get(id).unwrap().reinforcement.is_some(), "the beam clicked is reinforced");
        assert!(app.modeller.rebar_tool.is_some(), "the tool stays on");
        // a second click leaves it as it is
        let _ = app.update(Message::Navigate(crate::viewport::Navigate::Pick { x, y, width: 1000.0, height: 800.0, add: false }));
        assert_eq!(app.modeller.model.get(id).unwrap().reinforcement, Some(Default::default()));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.rebar_tool.is_none() && app.selecting());
    }
}
