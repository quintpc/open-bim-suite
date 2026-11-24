//! The sketch of a floor's outline: the Slab tool opens an empty one to draw a new floor in, and
//! Tab on a selected slab opens its outline as loose lines, to draw on and to trim, as the
//! boundary sketch of the reference modeller.
//!
//! - **A new floor.** The Slab tool draws no rectangle of its own: it opens the sketch with the
//!   Line tool on, at the height its strip gives. What is drawn is a floor (or a roof, or a
//!   ceiling: the strip says which) of the strip's type and thickness once it is finished.
//! - **A roof.** The Roof tool opens the sketch with the Slab tool making a roof. Every line of
//!   the outline is an eave the roof rises from at the angle of the strip; a picked line can be
//!   given an angle of its own, and **H** takes its slope away or gives it back: a line without
//!   a slope is a gable (or, with none sloping, the roof is flat). Finished, the roof is a roof
//!   slab for every plane, each rising from its eave (see `bim_core::element::roof_planes`).
//! - **A stair.** The Stair tool's Sketch opens the same sketch for a stair. **P** draws the line
//!   it climbs along, from its foot: each click a point. With an outline drawn, the stair is the
//!   outline cut into treads across that line, from its first point to its last. With no outline
//!   the stair runs along the line itself, as wide as the tool's strip says, with a landing on
//!   every corner of it.
//! - **C** draws a circle with two clicks: its centre and a point on it. **O** takes a line over
//!   from what is there: a click on a side of a wall, an edge of a floor, a beam or a grid line
//!   adds that line to the sketch, and where it meets a line taken before the two are made to
//!   end in their corner.
//!
//! - **The lines.** Each edge of the outline is a line of its own, straight or an arc; the edges
//!   of the floor's openings too. While the sketch is open they need not close: lines are added,
//!   cut and removed freely, and the floor stays as it was.
//! - **L** draws lines, each click a point, one line running on from the last. **R** draws a
//!   rectangle with two clicks. **A** draws an arc with three: its start, its end, and a point
//!   it runs through. A click near an end of a line, or near a point of another element, lands on
//!   it; else it is rounded as other moves are.
//! - **T** trims: a click on a line takes the stretch under it away, between the two nearest
//!   places where other lines cross or touch it; a line nothing crosses goes whole.
//! - **With no tool** a click picks a line (Delete removes it) and a drag moves a point, with the
//!   ends of every line that met there.
//! - **Tab finishes.** The lines must make closed loops: every point the end of two lines. A gap
//!   or a branch is told and the sketch stays open. The largest loop becomes the floor's outline,
//!   its arcs kept as arcs; every other loop an opening in it (an opening's arcs become chords).
//!   It is one step to take back; what the kernel refuses (loops that cross, an opening outside
//!   the outline) is told too.
//! - **Esc** lets a tool go, then leaves the sketch with the floor unchanged.
//! - **A framed wall.** Tab on a selected timber frame, steel frame or metal stud wall opens the
//!   same sketch in the wall's own face, upright: its outline and its openings as lines. A
//!   rectangle drawn inside it is an opening, framed as the wall's type frames openings; a
//!   corner cut off the outline is cut off the wall. Tab makes the wall as long and as high as
//!   the outline reaches. An opening that is not a rectangle with level and plumb sides is told.
//!   In brickwork an opening's sill and head go to the nearest course.

use crate::app::edit::{EditEvent, EditOverlay};
use crate::app::App;
use bim_core::element::{arc_of, arc_points, bowed, PlanarForm};
use bim_core::{ElementId, PlanarDef, Shape};
use glam::{DVec2, DVec3};
use iced::Color;
use openaec_ui::Language;
use std::f64::consts::{PI, TAU};

/// The pitch the Roof tool starts with (degrees).
pub const ROOF_PITCH: f64 = 30.0;

/// Points this close are one point (metres).
const MEET: f64 = 1e-4;
/// A click this near a point or a line takes it (logical pixels).
const REACH: f32 = 12.0;
/// A point clicked this near a line of the sketch is put on it (logical pixels).
const ON_LINE: f32 = 4.0;
/// A click is rounded to this (metres).
const ROUND: f64 = 0.01;

/// A line of the sketch, in plan: from `a` to `b`, straight or an arc with `bulge` (the tangent
/// of a quarter of its angle, above zero where it bows to the right of its way).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub a: DVec2,
    pub b: DVec2,
    pub bulge: f64,
}

impl Segment {
    pub fn line(a: DVec2, b: DVec2) -> Self {
        Self { a, b, bulge: 0.0 }
    }

    /// The arc from `a` to `b` through `p`; a straight line where the three lie on one.
    pub fn through(a: DVec2, b: DVec2, p: DVec2) -> Self {
        let side = (b - a).perp_dot(p - a);
        if side.abs() < MEET * a.distance(b) {
            return Self::line(a, b);
        }
        // the angle the chord is seen under from `p` gives how far the arc turns
        let seen = (a - p).angle_to(b - p).abs();
        Self { a, b, bulge: ((PI - seen) / 2.0).tan() * -side.signum() }
    }

    fn length(&self) -> f64 {
        arc_of(self.a, self.b, self.bulge).map_or(self.a.distance(self.b), |(_, radius, _, turn)| radius * turn.abs())
    }

    /// The point `t` of the way along it (0 its start, 1 its end).
    fn at(&self, t: f64) -> DVec2 {
        match arc_of(self.a, self.b, self.bulge) {
            Some((centre, radius, start, turn)) => centre + radius * DVec2::from_angle(start + turn * t),
            None => self.a.lerp(self.b, t),
        }
    }

    /// How far along it (0 to 1) its point nearest `p` lies.
    fn along(&self, p: DVec2) -> f64 {
        let Some((centre, _, start, turn)) = arc_of(self.a, self.b, self.bulge) else {
            let r = self.b - self.a;
            return ((p - self.a).dot(r) / r.length_squared().max(1e-300)).clamp(0.0, 1.0);
        };
        let q = p - centre;
        let angle = q.y.atan2(q.x);
        let t = if turn > 0.0 { (angle - start).rem_euclid(TAU) / turn } else { (start - angle).rem_euclid(TAU) / -turn };
        match t <= 1.0 {
            true => t,
            // past the arc: its nearer end
            false if p.distance(self.a) <= p.distance(self.b) => 0.0,
            false => 1.0,
        }
    }

    fn distance(&self, p: DVec2) -> f64 {
        self.at(self.along(p)).distance(p)
    }

    /// The stretch of it from `from` to `to` of the way along.
    fn part(&self, from: f64, to: f64) -> Self {
        let bulge = arc_of(self.a, self.b, self.bulge).map_or(0.0, |(.., turn)| (turn * (to - from) / 4.0).tan());
        Self { a: self.at(from), b: self.at(to), bulge }
    }

    /// Its points to draw it by: its ends for a straight line, chords along an arc.
    fn chords(&self) -> Vec<DVec2> {
        let mut points = arc_points(self.a, self.b, self.bulge);
        points.push(self.b);
        points
    }

    /// Where the line or the circle it lies on meets the one `other` lies on.
    fn carriers_meet(&self, other: &Segment) -> Vec<DVec2> {
        let circle = |s: &Segment| arc_of(s.a, s.b, s.bulge).map(|(centre, radius, ..)| (centre, radius));
        // a straight line through `a` along `r` against a circle
        let line_circle = |a: DVec2, r: DVec2, (centre, radius): (DVec2, f64)| -> Vec<DVec2> {
            let u = r.normalize_or_zero();
            let foot = a + u * (centre - a).dot(u);
            let off = foot.distance(centre);
            if off > radius + MEET {
                return Vec::new();
            }
            let half = (radius * radius - off * off).max(0.0).sqrt();
            vec![foot - u * half, foot + u * half]
        };
        match (circle(self), circle(other)) {
            (None, None) => {
                let (r, q) = (self.b - self.a, other.b - other.a);
                let denom = r.perp_dot(q);
                if denom.abs() < 1e-9 * r.length() * q.length() {
                    return Vec::new();
                }
                vec![self.a + r * ((other.a - self.a).perp_dot(q) / denom)]
            }
            (None, Some(c)) => line_circle(self.a, self.b - self.a, c),
            (Some(c), None) => line_circle(other.a, other.b - other.a, c),
            (Some((c1, r1)), Some((c2, r2))) => {
                let d = c1.distance(c2);
                if d < MEET || d > r1 + r2 + MEET || d < (r1 - r2).abs() - MEET {
                    return Vec::new();
                }
                let along = (d * d + r1 * r1 - r2 * r2) / (2.0 * d);
                let half = (r1 * r1 - along * along).max(0.0).sqrt();
                let u = (c2 - c1) / d;
                let foot = c1 + u * along;
                vec![foot + u.perp() * half, foot - u.perp() * half]
            }
        }
    }

    /// How far along it `other` crosses or touches it, between its ends.
    fn crossings(&self, other: &Segment) -> Vec<f64> {
        let length = self.length();
        self.carriers_meet(other)
            .into_iter()
            .filter(|p| self.distance(*p) <= MEET && other.distance(*p) <= MEET)
            .map(|p| self.along(p))
            .filter(|t| t * length > MEET && t * length < length - MEET)
            .collect()
    }
}

/// What a click in the sketch does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SketchTool {
    /// Picks a line; a drag moves a point.
    #[default]
    Pick,
    Copy,
    Line,
    Rectangle,
    Circle,
    Arc,
    /// Takes over the line under the click: a wall's side, a floor's edge, a grid line.
    Trace,
    Trim,
    /// The line a stair climbs along, from its foot: each click a point of it.
    Path,
}

/// Why the lines of a sketch make no outline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchError {
    /// No lines.
    Empty,
    /// A line ends where no other begins.
    Gap,
    /// More than two lines meet in a point.
    Branch,
    /// A stair's sketch without the line it climbs along.
    NoPath,
    /// A loop inside a wall's outline is not a rectangle with level and plumb sides.
    OpeningNotSquare,
    /// A loop inside a system floor's outline is not a rectangle along its laying direction.
    OpeningNotAlong,
}

impl SketchError {
    pub fn reason(self, l: Language) -> &'static str {
        match self {
            Self::Empty => l.text("De schets heeft geen lijnen", "The sketch has no lines"),
            Self::Gap => l.text("De omtrek is niet gesloten", "The outline is not closed"),
            Self::Branch => l.text("In een punt komen meer dan twee lijnen samen", "More than two lines meet in a point"),
            Self::NoPath => l.text("Teken de looplijn (P) van de voet naar de kop van de trap", "Draw the line it climbs along (P), from the foot of the stair to its head"),
            Self::OpeningNotAlong => l.text("Een opening in een systeemvloer is een rechthoek langs de legrichting", "An opening in a system floor is a rectangle along its laying direction"),
            Self::OpeningNotSquare => l.text("Een opening in een wand is een rechthoek, waterpas en te lood", "An opening in a wall is a rectangle, level and plumb"),
        }
    }
}

/// The sketch of a floor's outline, open.
#[derive(Debug, Clone)]
pub struct Sketch {
    /// The floor whose outline it is; none for a new floor, made when the sketch is finished.
    pub element: Option<ElementId>,
    /// A new floor's name and level, as the Slab tool gives them.
    pub new: Option<(&'static str, Option<String>)>,
    /// The sketch is a stair's: what is drawn becomes a stair, not a floor.
    pub stair: bool,
    /// A stair's line: where it climbs along, from its foot.
    pub path: Vec<DVec2>,
    /// The lines of a roof's outline with an angle of their own (degrees; 0: no slope, a gable),
    /// each by its two ends.
    pub slopes: Vec<(DVec2, DVec2, f64)>,
    /// The height the sketch lies at: the floor's top (scene coordinates).
    pub top: f64,
    pub segments: Vec<Segment>,
    pub tool: SketchTool,
    /// The point clicked first, of a line, a rectangle or an arc under way.
    pub first: Option<DVec2>,
    /// The end of an arc under way, clicked second.
    pub second: Option<DVec2>,
    /// Where the next click would land, while the cursor is over the viewport.
    pub cursor: Option<DVec2>,
    /// The line picked.
    pub picked: Option<usize>,
    /// The point being dragged, where it is now.
    pub held: Option<DVec2>,
    /// Why Tab could not finish it.
    pub error: Option<SketchError>,
    /// The sketch of a wall: where its face stands. None for a floor's, which lies at `top`.
    pub face: Option<Face>,
}

/// The face of a standing wall a sketch is drawn in: a point of the sketch is so far `along` it
/// from `origin` and so far up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    pub origin: DVec3,
    pub along: DVec3,
}

impl Sketch {
    /// Where point `p` of the sketch is in the scene.
    fn lift(&self, p: DVec2) -> DVec3 {
        match self.face {
            Some(f) => f.origin + f.along * p.x + DVec3::Z * p.y,
            None => p.extend(self.top),
        }
    }
}

/// A closed loop of a sketch: its corners in order, each with the bulge of the edge that leaves it.
pub type Loop = Vec<(DVec2, f64)>;

/// The closed loops the lines make. Every point must be the end of two lines.
pub fn loops(segments: &[Segment]) -> Result<Vec<Loop>, SketchError> {
    let segments: Vec<Segment> = segments.iter().copied().filter(|s| s.length() > MEET).collect();
    if segments.is_empty() {
        return Err(SketchError::Empty);
    }
    // the points, and for each line the two it runs between
    let mut points: Vec<DVec2> = Vec::new();
    let mut node = |p: DVec2| match points.iter().position(|q| q.distance(p) <= MEET) {
        Some(i) => i,
        None => {
            points.push(p);
            points.len() - 1
        }
    };
    let ends: Vec<[usize; 2]> = segments.iter().map(|s| [node(s.a), node(s.b)]).collect();
    let degree = |n: usize| ends.iter().flatten().filter(|&&e| e == n).count();
    if (0..points.len()).any(|n| degree(n) > 2) {
        return Err(SketchError::Branch);
    }
    if (0..points.len()).any(|n| degree(n) < 2) {
        return Err(SketchError::Gap);
    }
    // every point has two lines: walk each loop from a line not yet walked, round to its start
    let mut used = vec![false; segments.len()];
    let mut out = Vec::new();
    while let Some(first) = used.iter().position(|u| !u) {
        used[first] = true;
        let start = ends[first][0];
        let mut ring: Loop = vec![(points[start], segments[first].bulge)];
        let mut at = ends[first][1];
        while at != start {
            let Some(next) = (0..segments.len()).find(|&k| !used[k] && ends[k].contains(&at)) else { return Err(SketchError::Gap) };
            used[next] = true;
            // walked from its end to its start, a line bows the other way
            let forward = ends[next][0] == at;
            ring.push((points[at], if forward { segments[next].bulge } else { -segments[next].bulge }));
            at = ends[next][if forward { 1 } else { 0 }];
        }
        out.push(ring);
    }
    Ok(out)
}

/// The corners and the bulges of a loop apart.
fn apart(ring: &Loop) -> (Vec<DVec2>, Vec<f64>) {
    ring.iter().copied().unzip()
}

/// Twice the area inside a ring of points, whichever way it runs.
fn area2(ring: &[DVec2]) -> f64 {
    let n = ring.len();
    (0..n).map(|k| ring[k].perp_dot(ring[(k + 1) % n])).sum::<f64>().abs()
}

/// The lines with the stretch of line `index` under point `at` taken away: between the two
/// nearest places where other lines cross or touch it, or the whole line where none does.
pub fn trimmed(segments: &[Segment], index: usize, at: DVec2) -> Vec<Segment> {
    let Some(&s) = segments.get(index) else { return segments.to_vec() };
    let t0 = s.along(at);
    let cuts: Vec<f64> = segments.iter().enumerate().filter(|(k, _)| *k != index).flat_map(|(_, o)| s.crossings(o)).collect();
    let from = cuts.iter().copied().filter(|t| *t <= t0).fold(0.0, f64::max);
    let to = cuts.iter().copied().filter(|t| *t >= t0).fold(1.0, f64::min);
    let mut left = Vec::new();
    if from > 0.0 {
        left.push(s.part(0.0, from));
    }
    if to < 1.0 {
        left.push(s.part(to, 1.0));
    }
    let mut out = segments.to_vec();
    out.splice(index..=index, left);
    out
}

/// The circle about `centre` through `on`, as four quarter arcs that run round anticlockwise.
fn circle(centre: DVec2, on: DVec2) -> [Segment; 4] {
    let r = on - centre;
    let at = |k: usize| centre + DVec2::from_angle(k as f64 * PI / 2.0).rotate(r);
    // a quarter turn: the tangent of a quarter of it; it bows to the right of its way
    let bulge = (PI / 8.0).tan();
    [0, 1, 2, 3].map(|k| Segment { a: at(k), b: at(k + 1), bulge })
}

/// How far the end of a line taken over may lie from the corner it is made to end in (metres):
/// the sides of two walls that meet end short of their corner or run past it by a wall's
/// thickness.
const CORNER: f64 = 1.0;

/// Whether `line` runs between `a` and `b`, either way.
fn same_line(a: DVec2, b: DVec2, line: &Segment) -> bool {
    (line.a.distance(a) <= MEET && line.b.distance(b) <= MEET) || (line.a.distance(b) <= MEET && line.b.distance(a) <= MEET)
}

/// The four sides of the rectangle between corners `a` and `b`.
fn rectangle(a: DVec2, b: DVec2) -> [Segment; 4] {
    let (c, d) = (DVec2::new(b.x, a.y), DVec2::new(a.x, b.y));
    [Segment::line(a, c), Segment::line(c, b), Segment::line(b, d), Segment::line(d, a)]
}

impl Sketch {
    /// The points of the sketch, each once: the ends of its lines.
    fn points(&self) -> Vec<DVec2> {
        let mut points: Vec<DVec2> = Vec::new();
        for p in self.segments.iter().flat_map(|s| [s.a, s.b]) {
            if !points.iter().any(|q| q.distance(p) <= MEET) {
                points.push(p);
            }
        }
        points
    }

    fn copy_at(&self, base: DVec2, target: DVec2) -> Option<Segment> {
        let s = self.segments.get(self.picked?)?;
        let delta = target - base;
        Some(Segment { a: s.a + delta, b: s.b + delta, bulge: s.bulge })
    }

    /// The lines a click would add now: from the points clicked to the cursor.
    fn preview(&self) -> Vec<Segment> {
        match (self.tool, self.first, self.second, self.cursor) {
            (SketchTool::Copy, Some(a), _, Some(b)) => self.copy_at(a, b).into_iter().collect(),
            (SketchTool::Line, Some(a), _, Some(b)) | (SketchTool::Arc, Some(a), None, Some(b)) => vec![Segment::line(a, b)],
            (SketchTool::Rectangle, Some(a), _, Some(b)) => rectangle(a, b).to_vec(),
            (SketchTool::Path, _, _, Some(b)) => self.path.last().map(|a| vec![Segment::line(*a, b)]).unwrap_or_default(),
            (SketchTool::Circle, Some(a), _, Some(b)) if a.distance(b) > MEET => circle(a, b).to_vec(),
            (SketchTool::Arc, Some(a), Some(b), Some(p)) => vec![Segment::through(a, b, p)],
            _ => Vec::new(),
        }
    }

    /// A click of the Line, the Rectangle or the Arc tool at `p`.
    fn append_contour_edge(&mut self, edge: Segment) {
        let meets_end = self.segments.iter().any(|s| s.a.distance(edge.b) <= MEET || s.b.distance(edge.b) <= MEET);
        self.segments.push(edge);
        self.first = Some(edge.b);
        self.second = None;
        if meets_end && loops(&self.segments).is_ok() {
            self.first = None;
            self.tool = SketchTool::Pick;
        }
    }

    fn place(&mut self, p: DVec2) {
        self.error = None;
        if self.tool == SketchTool::Path {
            if self.path.last().is_none_or(|last| last.distance(p) > MEET) {
                self.path.push(p);
            }
            return;
        }
        match (self.tool, self.first, self.second) {
            (SketchTool::Copy, Some(a), _) if a.distance(p) > MEET => {
                if let Some(copy) = self.copy_at(a, p) {
                    self.picked = Some(self.segments.len());
                    self.segments.push(copy);
                    self.tool = SketchTool::Pick;
                    self.first = None;
                }
            }
            (SketchTool::Copy, None, _) if self.picked.is_some() => self.first = Some(p),
            (SketchTool::Line, Some(a), _) if a.distance(p) > MEET => {
                self.append_contour_edge(Segment::line(a, p));
            }
            (SketchTool::Rectangle, Some(a), _) if (a.x - p.x).abs() > MEET && (a.y - p.y).abs() > MEET => {
                self.segments.extend(rectangle(a, p));
                self.first = None;
            }
            (SketchTool::Circle, Some(a), _) if a.distance(p) > MEET => {
                self.segments.extend(circle(a, p));
                self.first = None;
            }
            (SketchTool::Arc, Some(a), None) if a.distance(p) > MEET => self.second = Some(p),
            (SketchTool::Arc, Some(a), Some(b)) => {
                self.append_contour_edge(Segment::through(a, b, p));
            }
            (SketchTool::Line | SketchTool::Rectangle | SketchTool::Circle | SketchTool::Arc, None, _) => self.first = Some(p),
            _ => {}
        }
    }

    /// Takes straight line `line` over into the sketch (not twice). Where its carrier crosses
    /// that of a straight line already there near an end of both, the two end in that corner.
    fn take_over(&mut self, mut line: Segment) {
        self.error = None;
        let same = |s: &Segment| (s.a.distance(line.a) <= MEET && s.b.distance(line.b) <= MEET) || (s.a.distance(line.b) <= MEET && s.b.distance(line.a) <= MEET);
        if self.segments.iter().any(same) {
            return;
        }
        for mine in 0..2 {
            let end = if mine == 0 { line.a } else { line.b };
            // the nearest corner with another straight line
            let mut best: Option<(f64, usize, usize, DVec2)> = None;
            for (k, other) in self.segments.iter().enumerate().filter(|(_, s)| s.bulge == 0.0) {
                let (u, v) = (line.b - line.a, other.b - other.a);
                let det = u.perp_dot(v);
                if det.abs() < 1e-9 * u.length() * v.length() {
                    continue;
                }
                let corner = line.a + u * ((other.a - line.a).perp_dot(v) / det);
                for (theirs, their_end) in [other.a, other.b].into_iter().enumerate() {
                    let far = corner.distance(end).max(corner.distance(their_end));
                    if far <= CORNER && best.is_none_or(|(d, ..)| far < d) {
                        best = Some((far, k, theirs, corner));
                    }
                }
            }
            if let Some((_, k, theirs, corner)) = best {
                if mine == 0 { line.a = corner } else { line.b = corner }
                if theirs == 0 { self.segments[k].a = corner } else { self.segments[k].b = corner }
            }
        }
        if line.a.distance(line.b) > MEET {
            self.segments.push(line);
        }
    }
}

impl App {
    /// Whether `id` is a floor whose outline Tab opens as a sketch.
    pub(in crate::app) fn sketchable(&self, id: ElementId) -> bool {
        self.modeller.model.get(id).is_some_and(|e| match &e.shape {
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. }) => true,
            // a floor or a ceiling made as a system is laid in its outline
            // a framed wall is sketched in its own face
            Shape::System(s) => s.system_type.is_some(),
            _ => false,
        })
    }

    /// Opens the sketch of floor `element`: its outline and its openings as lines.
    pub(in crate::app) fn open_sketch(&mut self, element: ElementId) {
        self.drop_grab();
        let Some(shape) = self.modeller.model.get(element).map(|e| e.shape_in_project(&self.modeller.model.grid)) else { return };
        let mut face = None;
        let (outline, holes, top, bulges) = match shape {
            // a framed wall: its outline in its face, and its openings as rectangles
            Shape::System(s) if !s.lies() => {
                let Some(dir) = (s.end - s.start).try_normalize() else { return };
                let p = DVec2::new;
                let length = (s.end - s.start).length();
                let outline = if s.outline.len() >= 3 { s.outline.clone() } else { vec![p(0.0, 0.0), p(length, 0.0), p(length, s.height), p(0.0, s.height)] };
                let holes = s.openings.iter().map(|o| vec![p(o.along, o.sill), p(o.along + o.width, o.sill), p(o.along + o.width, o.sill + o.height), p(o.along, o.sill + o.height)]).collect();
                face = Some(Face { origin: s.start.extend(s.base), along: dir.extend(0.0) });
                (outline, holes, s.base, Vec::new())
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, top, bulges, .. }, .. }) => (outline, holes, top, bulges),
            // a floor made as a system: its outline, and its openings (but the drilled ones) as
            // rectangles along its line
            Shape::System(s) if s.lies() => match super::slab_handles::lying_outline(&s) {
                Some(outline) => {
                    let dir = (s.end - s.start).try_normalize().unwrap_or(DVec2::X);
                    let at = |u: f64, v: f64| s.start + dir * u + dir.perp() * v;
                    let holes = s.openings.iter().filter(|o| !o.framing.is_some_and(|f| f.round)).map(|o| vec![at(o.along, o.sill), at(o.along + o.width, o.sill), at(o.along + o.width, o.sill + o.height), at(o.along, o.sill + o.height)]).collect();
                    (outline, holes, s.base, Vec::new())
                }
                None => return,
            },
            _ => return,
        };
        let n = outline.len();
        let mut segments: Vec<Segment> = (0..n).map(|i| Segment { a: outline[i], b: outline[(i + 1) % n], bulge: bulges.get(i).copied().unwrap_or(0.0) }).collect();
        for hole in &holes {
            segments.extend((0..hole.len()).map(|i| Segment::line(hole[i], hole[(i + 1) % hole.len()])));
        }
        self.select_tool();
        self.modeller.refusal = None;
        self.measure.tool = None;
        self.modeller.sketch = Some(Sketch { element: Some(element), new: None, stair: false, path: Vec::new(), slopes: Vec::new(), top, segments, tool: SketchTool::Pick, first: None, second: None, cursor: None, picked: None, held: None, error: None, face });
    }

    /// A tool of the Modeller is chosen (from the ribbon or by its keys): as
    /// [`App::set_model_tool`], and the Slab tool opens the sketch of a new floor.
    pub(in crate::app) fn start_model_tool(&mut self, tool: Option<(super::ModelTool, &'static str)>) {
        self.set_model_tool(tool);
        if let Some((super::ModelTool::Slab, name)) = self.modeller.tool {
            self.open_new_sketch(name);
        }
    }

    /// Opens an empty sketch for a new floor named `name`, with the Line tool on. The Slab tool
    /// stays on beside it: its strip gives the floor's type, thickness and height.
    fn open_new_sketch(&mut self, name: &'static str) {
        let (z, _, level) = self.work_plane();
        let top = z + self.modeller.placement.of(super::ModelTool::Slab).offset;
        self.modeller.sketch = Some(Sketch { element: None, new: Some((name, level)), stair: false, path: Vec::new(), slopes: Vec::new(), top, segments: Vec::new(), tool: SketchTool::Line, first: None, second: None, cursor: None, picked: None, held: None, error: None, face: None });
    }

    /// The Stair tool's Sketch: opens an empty sketch for a new stair, with the Line tool on, or
    /// closes the one that is open. The Stair tool stays on beside it: its strip gives the
    /// stair's width, height and steps.
    pub(in crate::app) fn toggle_stair_sketch(&mut self) {
        let Some((super::ModelTool::Stair, name)) = self.modeller.tool else { return };
        if self.modeller.sketch.take().is_some() {
            return;
        }
        let (z, _, level) = self.work_plane();
        let top = z + self.modeller.placement.of(super::ModelTool::Stair).offset;
        self.modeller.points.clear();
        self.modeller.sketch = Some(Sketch { element: None, new: Some((name, level)), stair: true, path: Vec::new(), slopes: Vec::new(), top, segments: Vec::new(), tool: SketchTool::Line, first: None, second: None, cursor: None, picked: None, held: None, error: None, face: None });
    }

    /// The sketch of a new floor or stair leaves, and its tool with it.
    fn leave_sketch(&mut self) {
        if self.modeller.sketch.take().is_some_and(|s| s.new.is_some()) && self.modeller.tool.is_some_and(|(t, _)| matches!(t, super::ModelTool::Slab | super::ModelTool::Stair)) {
            self.modeller.tool = None;
        }
        self.modeller.refusal = None;
    }

    /// The straight line of the model under viewport point (x, y), to take over into the sketch:
    /// a side of a wall, an edge of a floor, the axis of a beam or a grid line; the nearest on the
    /// screen within reach.
    fn traceable_under(&self, x: f32, y: f32, width: f32, height: f32) -> Option<Segment> {
        let sketch = self.modeller.sketch.as_ref()?;
        let at = self.plane_point(x, y, width, height, sketch.top)?.truncate();
        let grid = self.modeller.model.grid;
        let mut lines: Vec<Segment> = Vec::new();
        for e in self.modeller.model.iter().filter(|e| Some(e.id) != sketch.element) {
            match e.shape_in_project(&grid) {
                Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) => {
                    let Some(along) = (end - start).try_normalize() else { continue };
                    let side = along.perp() * (p.total_thickness() / 2.0);
                    lines.extend([Segment::line(start + side, end + side), Segment::line(start - side, end - side)]);
                }
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) => {
                    let n = outline.len();
                    lines.extend((0..n).filter(|&i| bulges.get(i).copied().unwrap_or(0.0) == 0.0).map(|i| Segment::line(outline[i], outline[(i + 1) % n])));
                    for hole in &holes {
                        lines.extend((0..hole.len()).map(|i| Segment::line(hole[i], hole[(i + 1) % hole.len()])));
                    }
                }
                Shape::Linear(l) if (l.end - l.start).truncate().length() > MEET => lines.push(Segment::line(l.start.truncate(), l.end.truncate())),
                Shape::System(s) if !s.lies() => lines.push(Segment::line(s.start, s.end)),
                _ => {}
            }
        }
        lines.extend(self.modeller.model.grids().iter().map(|g| Segment::line(g.start, g.end)));
        let reach = |s: &Segment| {
            let (sx, sy) = self.on_screen(s.at(s.along(at)).extend(sketch.top), width, height)?;
            Some((sx - x).hypot(sy - y)).filter(|d| *d <= REACH)
        };
        lines.into_iter().filter(|s| s.length() > MEET).filter_map(|s| reach(&s).map(|d| (s, d))).min_by(|a, b| a.1.total_cmp(&b.1)).map(|(s, _)| s)
    }

    /// Where the cursor's ray at viewport point (x, y) meets the sketch's plane, as a point of
    /// the sketch: none where the view looks along the plane.
    fn sketch_plane_point(&self, x: f32, y: f32, width: f32, height: f32) -> Option<DVec2> {
        let sketch = self.modeller.sketch.as_ref()?;
        let Some(f) = sketch.face else { return self.plane_point(x, y, width, height, sketch.top).map(|p| p.truncate()) };
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(glam::Vec3::new(nx, ny, 0.0)).as_dvec3();
        let b = inv.project_point3(glam::Vec3::new(nx, ny, 1.0)).as_dvec3();
        let (d, normal) = (b - a, f.along.cross(DVec3::Z));
        if d.dot(normal).abs() < 1e-9 * d.length() {
            return None;
        }
        let t = (f.origin - a).dot(normal) / d.dot(normal);
        let hit = a + d * t - f.origin;
        (t >= 0.0).then(|| DVec2::new(hit.dot(f.along), hit.z))
    }

    /// Where a click at viewport point (x, y) lands in the sketch: on a point of the sketch or of
    /// another element near the cursor, else on the sketch's plane, rounded.
    fn sketch_point(&self, x: f32, y: f32, width: f32, height: f32, but: Option<DVec2>) -> Option<DVec2> {
        let sketch = self.modeller.sketch.as_ref()?;
        let own = sketch.points().into_iter().filter(|p| but.is_none_or(|b| b.distance(*p) > MEET));
        // a floor's sketch takes the points of other elements too, in plan; a wall's its own
        let others: Vec<DVec2> = if sketch.face.is_none() { self.snap_points_except(sketch.element.unwrap_or(ElementId(0))).into_iter().map(|q| q.truncate()).collect() } else { Vec::new() };
        let near = own
            .chain(others)
            .filter_map(|q| self.on_screen(sketch.lift(q), width, height).map(|s| (q, (s.0 - x).hypot(s.1 - y))))
            .filter(|(_, d)| *d <= REACH)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        let free = match near {
            Some((q, _)) => Some(q),
            // on a line of the sketch under the cursor, so a line drawn to it meets it; a point
            // that is dragged stays free of its own lines
            None => {
                let rounded = self.sketch_plane_point(x, y, width, height).map(|p| DVec2::new((p.x / ROUND).round() * ROUND, (p.y / ROUND).round() * ROUND));
                match self.sketch_line_within(x, y, width, height, ON_LINE).filter(|_| but.is_none()) {
                    // the rounded point brought onto the line, so it stays on round figures
                    Some((index, at)) => sketch.segments.get(index).map(|s| s.at(s.along(rounded.unwrap_or(at)))),
                    None => rounded,
                }
            }
        }?;
        // square only: a line, and a stair's climb line, run along x or y from their last point
        let from = match sketch.tool {
            SketchTool::Line => sketch.first,
            SketchTool::Path => sketch.path.last().copied(),
            _ => None,
        };
        Some(match from.filter(|_| self.modeller.snaps.square && but.is_none() && sketch.face.is_none()) {
            Some(from) => super::snaps::squared(from, free),
            None => free,
        })
    }

    /// The line of the sketch under viewport point (x, y), and the point of the sketch's plane
    /// there.
    fn sketch_line_under(&self, x: f32, y: f32, width: f32, height: f32) -> Option<(usize, DVec2)> {
        self.sketch_line_within(x, y, width, height, REACH)
    }

    /// The line of the sketch within `reach` pixels of viewport point (x, y), and the point of
    /// the sketch's plane there.
    fn sketch_line_within(&self, x: f32, y: f32, width: f32, height: f32, reach: f32) -> Option<(usize, DVec2)> {
        let sketch = self.modeller.sketch.as_ref()?;
        let at = self.sketch_plane_point(x, y, width, height)?;
        let (index, s) = sketch.segments.iter().enumerate().min_by(|a, b| a.1.distance(at).total_cmp(&b.1.distance(at)))?;
        // near enough on the screen: the nearest point of the line against the cursor
        let (sx, sy) = self.on_screen(sketch.lift(s.at(s.along(at))), width, height)?;
        ((sx - x).hypot(sy - y) <= reach).then_some((index, at))
    }

    /// A point of the sketch given in metres (see `place_model_point`): the tool on takes it as
    /// it takes a click there, the point as given and not rounded to the centimetre a cursor is
    /// rounded to. Gives the number of lines the sketch has then; None when no sketch is open or
    /// its tool takes no points.
    pub(in crate::app) fn sketch_place_at(&mut self, p: DVec2) -> Option<usize> {
        let sketch = self.modeller.sketch.as_mut()?;
        if !matches!(sketch.tool, SketchTool::Line | SketchTool::Rectangle | SketchTool::Circle | SketchTool::Arc | SketchTool::Path) {
            return None;
        }
        self.modeller.refusal = None;
        sketch.place(p);
        Some(sketch.segments.len())
    }

    /// A click in the viewport while the sketch is open. False when none is.
    pub(in crate::app) fn sketch_click(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        let Some(tool) = self.modeller.sketch.as_ref().map(|s| s.tool) else { return false };
        self.modeller.refusal = None;
        match tool {
            SketchTool::Copy if self.modeller.sketch.as_ref().is_some_and(|s| s.picked.is_none()) => {
                let under = self.sketch_line_under(x, y, width, height).map(|(index, _)| index);
                self.modeller.sketch.as_mut().unwrap().picked = under;
            }
            SketchTool::Copy | SketchTool::Line | SketchTool::Rectangle | SketchTool::Circle | SketchTool::Arc | SketchTool::Path => {
                let p = self.sketch_point(x, y, width, height, None);
                if let (Some(p), Some(sketch)) = (p, self.modeller.sketch.as_mut()) {
                    sketch.place(p);
                }
            }
            SketchTool::Trace => {
                let line = self.traceable_under(x, y, width, height);
                if let (Some(line), Some(sketch)) = (line, self.modeller.sketch.as_mut()) {
                    sketch.take_over(line);
                }
            }
            SketchTool::Trim => {
                let under = self.sketch_line_under(x, y, width, height);
                if let (Some((index, at)), Some(sketch)) = (under, self.modeller.sketch.as_mut()) {
                    sketch.segments = trimmed(&sketch.segments, index, at);
                    sketch.picked = None;
                    sketch.error = None;
                }
            }
            SketchTool::Pick => {
                let under = self.sketch_line_under(x, y, width, height).map(|(index, _)| index);
                if let Some(sketch) = self.modeller.sketch.as_mut() {
                    sketch.picked = under;
                }
            }
        }
        true
    }

    /// The cursor rests at (x, y) over the sketch: the line under way runs to it. False when no
    /// sketch is open.
    pub(in crate::app) fn sketch_hover(&mut self, x: f32, y: f32, width: f32, height: f32) -> bool {
        if self.modeller.sketch.is_none() {
            return false;
        }
        // a new floor lies where the strip of the Slab tool says now
        let z = self.work_plane().0;
        let (floor, stair) = (z + self.modeller.placement.of(super::ModelTool::Slab).offset, z + self.modeller.placement.of(super::ModelTool::Stair).offset);
        if let Some(sketch) = self.modeller.sketch.as_mut().filter(|s| s.new.is_some()) {
            sketch.top = if sketch.stair { stair } else { floor };
        }
        let p = self.sketch_point(x, y, width, height, None);
        if let Some(sketch) = self.modeller.sketch.as_mut() {
            sketch.cursor = p;
        }
        true
    }

    /// A key in the sketch: L, R, A and T take a tool (the same key again lets it go), S lets
    /// the tool go.
    pub(in crate::app) fn sketch_key(&mut self, key: char) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let tool = match key.to_ascii_lowercase() {
            'l' => SketchTool::Line,
            'r' => SketchTool::Rectangle,
            'c' => SketchTool::Circle,
            'a' => SketchTool::Arc,
            'o' => SketchTool::Trace,
            'p' if sketch.stair => SketchTool::Path,
            't' => SketchTool::Trim,
            // H: the picked line of a roof's outline loses its slope, or gets it back
            'h' => {
                if let Some(line) = sketch.picked.and_then(|k| sketch.segments.get(k)).copied() {
                    match sketch.slopes.iter().position(|(a, b, _)| same_line(*a, *b, &line)) {
                        Some(k) if sketch.slopes[k].2 == 0.0 => drop(sketch.slopes.remove(k)),
                        Some(k) => sketch.slopes[k].2 = 0.0,
                        None => sketch.slopes.push((line.a, line.b, 0.0)),
                    }
                }
                return;
            }
            's' => SketchTool::Pick,
            _ => return,
        };
        let continues = matches!((sketch.tool, tool), (SketchTool::Line, SketchTool::Arc) | (SketchTool::Arc, SketchTool::Line));
        sketch.tool = if sketch.tool == tool { SketchTool::Pick } else { tool };
        if !continues { sketch.first = None; }
        sketch.second = None;
        sketch.picked = None;
    }

    /// Delete in the sketch: the picked line goes. False when no sketch is open.
    pub(in crate::app) fn sketch_delete(&mut self) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        if let Some(index) = sketch.picked.take().filter(|i| *i < sketch.segments.len()) {
            sketch.segments.remove(index);
            sketch.error = None;
        }
        true
    }

    /// Esc in the sketch: a line under way stops, then the tool goes, then the sketch leaves with
    /// the floor unchanged. False when no sketch is open.
    pub(in crate::app) fn sketch_escape(&mut self) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        if sketch.first.is_some() {
            (sketch.first, sketch.second) = (None, None);
        } else if sketch.tool == SketchTool::Path && !sketch.path.is_empty() {
            // the line as far as it is drawn stays; the tool lets go
            sketch.tool = SketchTool::Pick;
        } else if sketch.tool != SketchTool::Pick {
            sketch.tool = SketchTool::Pick;
        } else {
            self.leave_sketch();
        }
        true
    }

    /// Tab in the sketch: its largest loop becomes the floor's outline and every other loop an
    /// opening, if the lines close and the kernel can make the floor; else the sketch stays open
    /// and says why.
    pub(in crate::app) fn finish_sketch(&mut self) {
        if self.modeller.sketch.as_ref().is_some_and(|s| s.stair) {
            return self.finish_stair_sketch();
        }
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let mut rings = match loops(&sketch.segments) {
            Ok(rings) => rings,
            Err(error) => {
                sketch.error = Some(error);
                return;
            }
        };
        let (element, new, top) = (sketch.element, sketch.new.clone(), sketch.top);
        let grid = self.modeller.model.grid;
        // the largest loop is the outline, the others are openings in it
        let size = |ring: &Loop| {
            let (corners, bulges) = apart(ring);
            area2(&bowed(&corners, &bulges))
        };
        let Some(largest) = (0..rings.len()).max_by(|a, b| size(&rings[*a]).total_cmp(&size(&rings[*b]))) else { return };
        let (corners, bows) = apart(&rings.remove(largest));
        // a new floor: what the Slab tool makes over the outline's box, given the outline drawn
        let Some(id) = element else {
            let Some((name, level)) = new else { return };
            let (lo, hi) = corners.iter().fold((DVec2::MAX, DVec2::MIN), |(lo, hi), c| (lo.min(*c), hi.max(*c)));
            let _ = top;
            let (z, ..) = self.work_plane();
            let mut shape = self.shape_for(super::ModelTool::Slab, lo.extend(z), hi.extend(z));
            // a roof: a plane from every line of the outline that slopes, each a roof slab
            let settings = self.modeller.placement.of(super::ModelTool::Slab);
            if settings.role == bim_core::SlabRole::Roof {
                let ring = bowed(&corners, &bows);
                let own = self.modeller.sketch.as_ref().map(|s| s.slopes.clone()).unwrap_or_default();
                let angles: Vec<f64> = (0..ring.len())
                    .map(|k| {
                        let edge = Segment::line(ring[k], ring[(k + 1) % ring.len()]);
                        own.iter().find(|(a, b, _)| same_line(*a, *b, &edge)).map_or(settings.sill, |(_, _, degrees)| *degrees).to_radians()
                    })
                    .collect();
                if angles.iter().any(|a| *a > 1e-9) {
                    let planes = match bim_core::element::roof_planes(&ring, &angles) {
                        Ok(planes) => planes,
                        Err(refusal) => {
                            self.modeller.refusal = Some(refusal);
                            return;
                        }
                    };
                    let name = self.placed_name(super::ModelTool::Slab, name);
                    let before = self.modeller.model.clone();
                    let mut made = Vec::new();
                    for (plane, slope) in planes {
                        let mut part = shape.clone();
                        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, slope: rises, .. }, .. }) = &mut part else { return };
                        (*outline, *rises) = (plane, slope);
                        holes.clear();
                        bulges.clear();
                        match self.add_element(name, level.clone(), part) {
                            Ok(id) => made.push(id),
                            // a plane the kernel cannot make: none of the roof is made
                            Err(refusal) => {
                                self.modeller.model = before;
                                self.show_modelled();
                                self.modeller.refusal = Some(refusal);
                                return;
                            }
                        }
                    }
                    self.leave_sketch();
                    if let Some(first) = made.first() {
                        self.select_modelled(*first);
                    }
                    return;
                }
            }
            match &mut shape {
                Shape::System(s) => s.outline = bowed(&corners, &bows),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) => {
                    *outline = corners;
                    *bulges = if bows.iter().any(|b| *b != 0.0) { bows } else { Vec::new() };
                    *holes = rings
                        .iter()
                        .map(|ring| {
                            let (corners, bows) = apart(ring);
                            bowed(&corners, &bows)
                        })
                        .collect();
                }
                _ => return,
            }
            let name = self.placed_name(super::ModelTool::Slab, name);
            match self.add_element(name, level, shape) {
                Ok(made) => {
                    self.leave_sketch();
                    self.select_modelled(made);
                }
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
            return;
        };
        let Some(old) = self.modeller.model.get(id).cloned() else {
            self.modeller.sketch = None;
            return;
        };
        let mut shape = old.shape_in_project(&grid);
        // a framed wall: as long and as high as the outline reaches, cut to it where it is not
        // that rectangle; every other loop an opening, framed as the wall's type frames them
        if let Shape::System(s) = &mut shape {
            if !s.lies() {
                let ring = bowed(&corners, &bows);
                let (lo, hi) = ring.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(lo, hi), p| (lo.min(*p), hi.max(*p)));
                let mut openings = Vec::with_capacity(rings.len());
                for hole in &rings {
                    let (hole, bows) = apart(hole);
                    let (a, b) = hole.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
                    let square = bows.iter().all(|b| *b == 0.0) && hole.iter().all(|p| ((p.x - a.x).abs() < MEET || (p.x - b.x).abs() < MEET) && ((p.y - a.y).abs() < MEET || (p.y - b.y).abs() < MEET)) && (area2(&hole).abs() / 2.0 - (b.x - a.x) * (b.y - a.y)).abs() < MEET;
                    if !square {
                        if let Some(sketch) = self.modeller.sketch.as_mut() {
                            sketch.error = Some(SketchError::OpeningNotSquare);
                        }
                        return;
                    }
                    // an opening that was there keeps how it is framed
                    let was = s.openings.iter().find(|o| (o.along - a.x).abs() < MEET && (o.sill - a.y).abs() < MEET && (o.width - (b.x - a.x)).abs() < MEET && (o.height - (b.y - a.y)).abs() < MEET);
                    openings.push(bim_core::Opening { along: a.x - lo.x, sill: a.y - lo.y, width: b.x - a.x, height: b.y - a.y, framing: was.and_then(|o| o.framing) });
                }
                openings.sort_by(|a, b| a.along.total_cmp(&b.along));
                let Some(dir) = (s.end - s.start).try_normalize() else { return };
                let start = s.start;
                (s.start, s.end) = (start + dir * lo.x, start + dir * hi.x);
                s.base += lo.y;
                s.height = hi.y - lo.y;
                s.breaks = s.breaks.iter().map(|b| b - lo.x).filter(|b| *b > 0.0 && *b < hi.x - lo.x).collect();
                // in brickwork an opening stands on a course and ends under one: its sill and
                // its head go to the nearest course, so no sliver of brick is left at them
                // (elements are cut to fit an opening: only courses of bricks hold one)
                if s.system_type.as_ref().is_some_and(|t| t.lintel.is_some()) && s.spacing.y > 0.0 && s.spacing.y <= 0.2 {
                    let rows = (s.height / s.spacing.y).round().max(1.0);
                    let course = s.height / rows;
                    for o in openings.iter_mut() {
                        let sill = (o.sill / course).round() * course;
                        let head = (((o.sill + o.height) / course).round() * course).max(sill + course);
                        (o.sill, o.height) = (sill, head - sill);
                    }
                }
                s.openings = openings;
                let (length, height) = (hi.x - lo.x, hi.y - lo.y);
                let corner = |p: &DVec2| ((p.x - lo.x).abs() < MEET || (p.x - lo.x - length).abs() < MEET) && ((p.y - lo.y).abs() < MEET || (p.y - lo.y - height).abs() < MEET);
                s.outline = if ring.len() == 4 && ring.iter().all(corner) { Vec::new() } else { ring.iter().map(|p| *p - lo).collect() };
                match self.modeller.model.replace(old.reshaped(&grid, shape)) {
                    Ok(()) => {
                        self.modeller.sketch = None;
                        self.modeller.refusal = None;
                        self.show_modelled();
                        self.select_modelled(id);
                    }
                    Err(refusal) => self.modeller.refusal = Some(refusal),
                }
                return;
            }
        }
        // a lying system takes the outline, its bows as chords; every other loop is an opening in
        // it, a rectangle along its line, cut and trimmed as its type says. Its drilled openings
        // stay as they are.
        if let Shape::System(s) = &mut shape {
            let Some(dir) = (s.end - s.start).try_normalize() else { return };
            let mut openings: Vec<bim_core::Opening> = s.openings.iter().filter(|o| o.framing.is_some_and(|f| f.round)).copied().collect();
            for hole in &rings {
                let (hole, bows) = apart(hole);
                let local: Vec<DVec2> = hole.iter().map(|p| DVec2::new((*p - s.start).dot(dir), (*p - s.start).dot(dir.perp()))).collect();
                let (a, b) = local.iter().fold((DVec2::splat(f64::INFINITY), DVec2::splat(f64::NEG_INFINITY)), |(a, b), p| (a.min(*p), b.max(*p)));
                let tol = 10.0 * MEET;
                let square = bows.iter().all(|b| *b == 0.0) && local.iter().all(|p| ((p.x - a.x).abs() < tol || (p.x - b.x).abs() < tol) && ((p.y - a.y).abs() < tol || (p.y - b.y).abs() < tol)) && (area2(&local).abs() / 2.0 - (b.x - a.x) * (b.y - a.y)).abs() < tol;
                if !square {
                    if let Some(sketch) = self.modeller.sketch.as_mut() {
                        sketch.error = Some(SketchError::OpeningNotAlong);
                    }
                    return;
                }
                let was = s.openings.iter().find(|o| (o.along - a.x).abs() < tol && (o.sill - a.y).abs() < tol && (o.width - (b.x - a.x)).abs() < tol && (o.height - (b.y - a.y)).abs() < tol);
                openings.push(bim_core::Opening { along: a.x, sill: a.y, width: b.x - a.x, height: b.y - a.y, framing: was.and_then(|o| o.framing) });
            }
            s.openings = openings;
            s.outline = bowed(&corners, &bows);
            match self.modeller.model.replace(old.reshaped(&grid, shape)) {
                Ok(()) => {
                    self.modeller.sketch = None;
                    self.modeller.refusal = None;
                    self.show_modelled();
                    self.select_modelled(id);
                }
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
            return;
        }
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) = &mut shape else { return };
        *outline = corners;
        *bulges = if bows.iter().any(|b| *b != 0.0) { bows } else { Vec::new() };
        *holes = rings
            .iter()
            .map(|ring| {
                let (corners, bows) = apart(ring);
                bowed(&corners, &bows)
            })
            .collect();
        match self.modeller.model.replace(old.reshaped(&grid, shape)) {
            Ok(()) => {
                self.modeller.sketch = None;
                self.modeller.refusal = None;
                self.show_modelled();
                self.select_modelled(id);
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// Tab in the sketch of a stair: with a line it climbs along and an outline, the stair is the
    /// outline cut into treads across the line from its first point to its last; with the line
    /// alone it runs along the line, as wide as the Stair tool's strip says, a landing on every
    /// corner. Its height and steps are the strip's.
    fn finish_stair_sketch(&mut self) {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let (path, name, level) = match (&sketch.path[..], sketch.new.clone()) {
            ([_, _, ..], Some((name, level))) => (sketch.path.clone(), name, level),
            _ => {
                sketch.error = Some(SketchError::NoPath);
                return;
            }
        };
        let (foot, head) = (path[0], path[path.len() - 1]);
        let drawn = match loops(&sketch.segments) {
            Ok(rings) => {
                // the largest loop is its outline
                let size = |ring: &Loop| {
                    let (corners, bulges) = apart(ring);
                    area2(&bowed(&corners, &bulges)).abs()
                };
                let Some(largest) = rings.iter().max_by(|a, b| size(a).total_cmp(&size(b))) else { return };
                let (corners, bows) = apart(largest);
                Some(bim_core::StairShape::Outline { outline: bowed(&corners, &bows), foot, head })
            }
            Err(SketchError::Empty) => None,
            Err(error) => {
                sketch.error = Some(error);
                return;
            }
        };
        let (z, ..) = self.work_plane();
        let mut shape = self.shape_for(super::ModelTool::Stair, foot.extend(z), head.extend(z));
        let Shape::Planar(PlanarDef { form: PlanarForm::Stair { shape: lies, .. }, .. }) = &mut shape else { return };
        *lies = match drawn {
            Some(outline) => outline,
            None => bim_core::StairShape::Run { path, width: self.modeller.placement.of(super::ModelTool::Stair).width },
        };
        let name = self.placed_name(super::ModelTool::Stair, name);
        match self.add_element(name, level, shape) {
            Ok(made) => {
                self.leave_sketch();
                self.select_modelled(made);
            }
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// Something the overlay reports while the sketch is open: with no tool a press takes point
    /// `index` and a drag moves it, with every line's end that lay there; with a drawing tool a
    /// press on a point is a click on it. False when no sketch is open.
    pub(in crate::app) fn sketch_event(&mut self, event: EditEvent) -> bool {
        let Some(sketch) = self.modeller.sketch.as_mut() else { return false };
        match event {
            EditEvent::Press { index, .. } => {
                let Some(&p) = sketch.points().get(index) else { return true };
                match sketch.tool {
                    SketchTool::Pick => sketch.held = Some(p),
                    SketchTool::Copy | SketchTool::Line | SketchTool::Rectangle | SketchTool::Circle | SketchTool::Arc | SketchTool::Path => sketch.place(p),
                    SketchTool::Trim | SketchTool::Trace => {}
                }
            }
            EditEvent::Drag { x, y, width, height } => {
                let Some(from) = sketch.held else { return true };
                let Some(to) = self.sketch_point(x, y, width, height, Some(from)) else { return true };
                if let Some(sketch) = self.modeller.sketch.as_mut() {
                    for s in sketch.segments.iter_mut() {
                        for end in [&mut s.a, &mut s.b] {
                            if end.distance(from) <= MEET {
                                *end = to;
                            }
                        }
                    }
                    sketch.held = Some(to);
                    sketch.error = None;
                }
            }
            EditEvent::Release => sketch.held = None,
        }
        true
    }

    /// The sketch over the viewport: its lines, the line under way, and a point at every end.
    pub(in crate::app) fn sketch_overlay(&self, ink: Color, fill: Color) -> Option<EditOverlay> {
        let sketch = self.modeller.sketch.as_ref()?;
        let up = |p: DVec2| sketch.lift(p).as_vec3();
        let points = sketch.points();
        let picked = sketch.picked.and_then(|i| sketch.segments.get(i));
        // a stair's line shows with the lines of its outline
        let path = sketch.path.windows(2).map(|w| Segment::line(w[0], w[1]));
        let lines = sketch.segments.iter().copied().chain(path).chain(sketch.preview()).flat_map(|s| s.chords().windows(2).map(|w| [up(w[0]), up(w[1])]).collect::<Vec<_>>()).collect();
        Some(EditOverlay {
            selected_sketch_line: Some(picked.map(|s| s.chords().into_iter().map(up).collect()).unwrap_or_default()),
            // the ends of the picked line, and the point held, show filled
            picked: points.iter().map(|p| sketch.held.is_some_and(|h| h.distance(*p) <= MEET) || picked.is_some_and(|s| [s.a, s.b].iter().any(|e| e.distance(*p) <= MEET))).collect(),
            points: points.iter().map(|p| up(*p)).collect(),
            closed: false,
            lines: Some(lines),
            view_proj: self.camera.view_proj(self.aspect()) * self.model_matrix(),
            ink,
            fill,
        })
    }

    /// The sketch's tools as buttons, over the view's top left: the tool that is on shows so, and
    /// Finish and Cancel end the sketch as Tab and Esc do.
    pub(in crate::app) fn sketch_bar(&self, p: crate::theme::Palette, l: Language) -> Option<iced::Element<'_, crate::app::Message>> {
        use crate::app::Message;
        use iced::widget::{button, container, text, Row};
        let sketch = self.modeller.sketch.as_ref()?;
        // each button under its name, so it is found by it (a screen reader, a remote click)
        use openaec_ui::design::a11y::{node as named, Node, Role};
        let tool = |label: &'static str, key: char, on: bool| named(button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(on, s)).on_press(Message::EditKey(key)), Node::new(Role::ToggleButton, label).checked(on));
        let is = |t: SketchTool| sketch.tool == t;
        let bar = Row::new()
            .spacing(4)
            .push(tool(l.text("Kiezen (S)", "Pick (S)"), 's', is(SketchTool::Pick)))
            .push(tool(l.text("Lijn (L)", "Line (L)"), 'l', is(SketchTool::Line)))
            .push(tool(l.text("Rechthoek (R)", "Rectangle (R)"), 'r', is(SketchTool::Rectangle)))
            .push(tool(l.text("Cirkel (C)", "Circle (C)"), 'c', is(SketchTool::Circle)))
            .push(tool(l.text("Boog (A)", "Arc (A)"), 'a', is(SketchTool::Arc)))
            .push(tool(l.text("Overnemen (O)", "Pick lines (O)"), 'o', is(SketchTool::Trace)))
            .push(tool(l.text("Trimmen (T)", "Trim (T)"), 't', is(SketchTool::Trim)))
            .push(sketch.stair.then(|| tool(l.text("Looplijn (P)", "Climb line (P)"), 'p', is(SketchTool::Path))))
            .push(self.sketch_slope_row(p, l))
            .push(named(button(text(l.text("Klaar (Tab)", "Finish (Tab)")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::EditMode), Node::new(Role::Button, l.text("Schets klaar", "Finish sketch"))))
            .push(named(button(text(l.text("Annuleren", "Cancel")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(false, s)).on_press(Message::SketchCancel), Node::new(Role::Button, l.text("Schets annuleren", "Cancel sketch"))));
        Some(container(bar.wrap()).padding([2, 8]).into())
    }

    /// Whether the sketch open is a new roof's: the Slab tool makes a roof.
    fn sketching_roof(&self) -> bool {
        self.modeller.sketch.as_ref().is_some_and(|s| s.new.is_some() && !s.stair) && self.modeller.placement.of(super::ModelTool::Slab).role == bim_core::SlabRole::Roof
    }

    /// The slope of the picked line of a roof's outline: a field for its angle, and a button
    /// that takes its slope away or gives it back (H).
    fn sketch_slope_row(&self, p: crate::theme::Palette, l: Language) -> Option<iced::Element<'_, crate::app::Message>> {
        use crate::app::Message;
        use iced::widget::{button, row, text, text_input};
        let sketch = self.modeller.sketch.as_ref().filter(|_| self.sketching_roof())?;
        let line = sketch.picked.and_then(|k| sketch.segments.get(k))?;
        let own = sketch.slopes.iter().find(|(a, b, _)| same_line(*a, *b, line)).map(|(_, _, degrees)| *degrees);
        let angle = own.unwrap_or(self.modeller.placement.of(super::ModelTool::Slab).sill);
        let field = text_input("", &format!("{angle}")).on_input(Message::SketchSlope).size(12).padding([2, 6]).width(60);
        let none = angle == 0.0;
        let toggle = button(text(l.text("Geen helling (H)", "No slope (H)")).size(12)).padding([3, 10]).style(move |_, s| p.ribbon_button(none, s)).on_press(Message::EditKey('h'));
        Some(row![text(l.text("Helling van deze rand (°)", "Slope of this edge (°)")).size(12).color(p.text_secondary), field, toggle].spacing(6).align_y(iced::Center).into())
    }

    /// The angle typed for the picked line of a roof's outline (degrees, 0 to 85).
    pub(in crate::app) fn sketch_slope_typed(&mut self, typed: String) {
        let Ok(degrees) = typed.trim().replace(',', ".").parse::<f64>() else { return };
        if !(0.0..=85.0).contains(&degrees) || !self.sketching_roof() {
            return;
        }
        let Some(sketch) = self.modeller.sketch.as_mut() else { return };
        let Some(line) = sketch.picked.and_then(|k| sketch.segments.get(k)).copied() else { return };
        match sketch.slopes.iter_mut().find(|(a, b, _)| same_line(*a, *b, &line)) {
            Some(own) => own.2 = degrees,
            None => sketch.slopes.push((line.a, line.b, degrees)),
        }
    }

    /// The Roof tool: the Slab tool making a roof, its sketch open; a roof that would be flat
    /// gets the usual pitch.
    pub(in crate::app) fn start_roof_tool(&mut self, name: &'static str) {
        let mut s = self.modeller.placement.of(super::ModelTool::Slab);
        s.role = bim_core::SlabRole::Roof;
        s.kind = None;
        if s.sill <= 0.0 {
            s.sill = ROOF_PITCH;
        }
        self.modeller.placement.set_tool(super::ModelTool::Slab, s);
        if self.modeller.tool.is_some_and(|(t, _)| t == super::ModelTool::Slab) {
            self.set_model_tool(None);
        }
        self.start_model_tool(Some((super::ModelTool::Slab, name)));
    }

    /// Cancel: the sketch leaves with the floor unchanged, whatever tool is on.
    pub(in crate::app) fn cancel_sketch(&mut self) {
        self.leave_sketch();
    }

    /// The banner while the sketch is open: why it could not finish, and what the keys do.
    pub(in crate::app) fn sketch_hint(&self, l: Language) -> Option<String> {
        let sketch = self.modeller.sketch.as_ref()?;
        let hint = match sketch.tool {
            SketchTool::Pick => l.text(
                "Sleep een punt · klik een lijn: kiezen, Del: weg · L: lijn · R: rechthoek · C: cirkel · A: boog · O: overnemen · T: trimmen · Tab: klaar · Esc: annuleren",
                "Drag a point · click a line: pick, Del: remove · L: line · R: rectangle · C: circle · A: arc · O: pick lines · T: trim · Tab: done · Esc: cancel",
            ),
            SketchTool::Copy if sketch.picked.is_none() => l.text("Kopiëren: kies een contourlijn", "Copy: select an outline segment"),
            SketchTool::Copy if sketch.first.is_none() => l.text("Kopiëren: kies het basispunt", "Copy: pick the base point"),
            SketchTool::Copy => l.text("Kopiëren: kies het doelpunt", "Copy: pick the destination point"),
            SketchTool::Line => l.text("Lijn: klik de punten · Esc: stoppen · Tab: klaar", "Line: click the points · Esc: stop · Tab: done"),
            SketchTool::Rectangle => l.text("Rechthoek: klik twee hoeken · Esc: stoppen · Tab: klaar", "Rectangle: click two corners · Esc: stop · Tab: done"),
            SketchTool::Path => l.text(
                "Looplijn: klik de voet, dan de punten tot de kop (een knik wordt een bordes) · Esc: klaar met de lijn · Tab: trap maken",
                "Climb line: click the foot, then the points up to the head (a corner becomes a landing) · Esc: line done · Tab: make the stair",
            ),
            SketchTool::Circle => l.text("Cirkel: klik het middelpunt en een punt op de cirkel · Esc: stoppen · Tab: klaar", "Circle: click its centre and a point on it · Esc: stop · Tab: done"),
            SketchTool::Trace => l.text(
                "Overnemen: klik een wandzijde, vloerrand, balk of stramien · Esc: stoppen · Tab: klaar",
                "Pick lines: click a side of a wall, an edge of a floor, a beam or a grid line · Esc: stop · Tab: done",
            ),
            SketchTool::Arc if sketch.second.is_some() => l.text("Klik een punt op de boog", "Click a point on the arc"),
            SketchTool::Arc if sketch.first.is_some() => l.text("Klik het eindpunt van de boog", "Click the end of the arc"),
            SketchTool::Arc => l.text(
                "Boog: klik het begin, het eind en een punt op de boog · Esc: stoppen · Tab: klaar",
                "Arc: click its start, its end and a point on it · Esc: stop · Tab: done",
            ),
            SketchTool::Trim => l.text("Trimmen: klik het stuk dat weg moet · Esc: stoppen · Tab: klaar", "Trim: click the stretch to take away · Esc: stop · Tab: done"),
        };
        let why = sketch.error.map(|e| e.reason(l)).or_else(|| self.modeller.refusal.map(|r| l.translate(r.reason())));
        let name = if sketch.stair { l.text("Trapschets", "Stair sketch") } else if sketch.face.is_some() { l.text("Wandschets", "Wall sketch") } else { l.text("Vloerschets", "Floor sketch") };
        Some(match why {
            Some(why) => format!("{name}: {why} · {hint}"),
            None => format!("{name}: {hint}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::modelling;
    use super::*;
    use crate::app::Message;
    use crate::viewport::Navigate;

    fn seg(a: (f64, f64), b: (f64, f64)) -> Segment {
        Segment::line(DVec2::new(a.0, a.1), DVec2::new(b.0, b.1))
    }

    fn near(a: &[Segment], b: &[Segment]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(s, o)| s.a.distance(o.a) < 1e-9 && s.b.distance(o.b) < 1e-9 && (s.bulge - o.bulge).abs() < 1e-9)
    }

    #[test]
    fn loops_finds_the_outline_in_any_order_and_direction() {
        // a 4 × 3 rectangle, its sides shuffled and two of them turned round
        let sides = [seg((4.0, 3.0), (0.0, 3.0)), seg((0.0, 0.0), (4.0, 0.0)), seg((0.0, 0.0), (0.0, 3.0)), seg((4.0, 3.0), (4.0, 0.0))];
        let rings = loops(&sides).unwrap();
        assert_eq!(rings.len(), 1);
        assert!((area2(&apart(&rings[0]).0) - 24.0).abs() < 1e-9, "{rings:?}");
        // a line of no length counts for nothing; a second rectangle is a loop of its own
        let mut more = sides.to_vec();
        more.push(seg((1.0, 1.0), (1.0, 1.0)));
        more.extend(rectangle(DVec2::new(1.0, 1.0), DVec2::new(2.0, 2.0)));
        assert_eq!(loops(&more).unwrap().len(), 2);
    }

    #[test]
    fn loops_says_what_is_wrong() {
        assert_eq!(loops(&[]), Err(SketchError::Empty));
        let open = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0)), seg((4.0, 3.0), (0.0, 3.0))];
        assert_eq!(loops(&open), Err(SketchError::Gap));
        let mut branch = rectangle(DVec2::ZERO, DVec2::new(4.0, 3.0)).to_vec();
        branch.push(seg((4.0, 3.0), (6.0, 5.0)));
        assert_eq!(loops(&branch), Err(SketchError::Branch));
    }

    #[test]
    fn trimmed_takes_the_stretch_between_crossings() {
        // a cross: the horizontal line trimmed left of the vertical keeps its right half
        let cross = [seg((0.0, 0.0), (4.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0))];
        assert!(near(&trimmed(&cross, 0, DVec2::new(0.5, 0.0)), &[seg((2.0, 0.0), (4.0, 0.0)), cross[1]]));
        // between two crossings: the middle goes, both ends stay
        let two = [seg((0.0, 0.0), (6.0, 0.0)), seg((2.0, -1.0), (2.0, 1.0)), seg((4.0, 0.0), (4.0, 2.0))];
        assert!(near(&trimmed(&two, 0, DVec2::new(3.0, 0.1)), &[seg((0.0, 0.0), (2.0, 0.0)), seg((4.0, 0.0), (6.0, 0.0)), two[1], two[2]]));
        // a line nothing crosses goes whole; one that only meets others at its ends too
        assert!(trimmed(&[seg((0.0, 0.0), (4.0, 0.0))], 0, DVec2::new(1.0, 0.0)).is_empty());
        let corner = [seg((0.0, 0.0), (4.0, 0.0)), seg((4.0, 0.0), (4.0, 3.0))];
        assert!(near(&trimmed(&corner, 0, DVec2::new(1.0, 0.0)), &[corner[1]]));
    }

    #[test]
    fn an_arc_runs_through_its_three_points_and_is_trimmed_where_a_line_crosses_it() {
        // from (0, 0) to (2, 0) through (1, -1): half a circle about (1, 0), counter-clockwise
        let arc = Segment::through(DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(1.0, -1.0));
        assert!((arc.bulge - 1.0).abs() < 1e-9, "{}", arc.bulge);
        assert!(arc.at(0.5).distance(DVec2::new(1.0, -1.0)) < 1e-9);
        assert!((arc.length() - PI).abs() < 1e-9);
        // through a point on the other side it bows the other way
        assert!((Segment::through(DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(1.0, 1.0)).bulge + 1.0).abs() < 1e-9);
        // a vertical line through its lowest point: trimmed below, the line starts on the arc
        let both = [arc, seg((1.0, -2.0), (1.0, 2.0))];
        assert!(near(&trimmed(&both, 1, DVec2::new(1.0, -1.5)), &[arc, seg((1.0, -1.0), (1.0, 2.0))]));
        // the arc trimmed on its first half: its second half is left, a quarter circle
        let left = trimmed(&both, 0, DVec2::new(0.2, -0.6));
        let quarter = Segment { a: DVec2::new(1.0, -1.0), b: DVec2::new(2.0, 0.0), bulge: (PI / 8.0).tan() };
        assert!(near(&left, &[quarter, both[1]]), "{left:?}");
    }

    /// A floor 4 × 3 m with its top at 0, selected.
    fn floor() -> (App, ElementId) {
        let mut app = modelling();
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let shape = Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None });
        let id = app.add_element("Floor", None, shape).unwrap();
        app.keep_model_history();
        app.select_modelled(id);
        (app, id)
    }

    fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    /// The outline, the openings and the bulges of floor `id`.
    fn slab_of(app: &App, id: ElementId) -> (Vec<DVec2>, Vec<Vec<DVec2>>, Vec<f64>) {
        let Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, bulges, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        (outline, holes, bulges)
    }

    #[test]
    fn tab_from_rotate_picks_the_floor_edge_on_mouse_down() {
        use iced::{mouse, Point, Rectangle, Size};
        use iced::widget::canvas::Program;
        let (mut app, _) = floor();
        let _ = app.update(Message::Transform(crate::app::transform::TransformKind::Rotate));
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.transform.is_none());
        let overlay = app.sketch_overlay(Color::BLACK, Color::WHITE).unwrap();
        let (x, y) = app.on_screen(DVec3::new(2.0, 0.0, 0.0), 1000.0, 800.0).unwrap();
        let action = overlay.update(&mut Default::default(), &iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)),
            Rectangle::new(Point::ORIGIN, Size::new(1000.0, 800.0)), mouse::Cursor::Available(Point::new(x, y))).unwrap();
        let _ = app.update(action.into_inner().0.unwrap());
        assert_eq!(app.modeller.sketch.as_ref().unwrap().picked, Some(0));
        assert_eq!(app.sketch_overlay(Color::BLACK, Color::WHITE).unwrap().selected_sketch_line.unwrap().len(), 2);
    }

    #[test]
    fn co_copies_the_selected_floor_edge_from_the_clicked_base_point() {
        use crate::product::Product;
        use crate::shell::{Msg, Studio};
        use iced::keyboard::{Key, Modifiers};
        use openaec_ui::keyboard::KeyPress;
        let (app, floor) = floor();
        let mut studio = Studio::new(app);
        studio.set_product(Product::Modeller);
        studio.app.select_modelled(floor);
        let co = |studio: &mut Studio| {
            for key in ["c", "o"] {
                let _ = studio.update(Msg::Key(KeyPress { key: Key::Character(key.into()), modifiers: Modifiers::empty() }));
            }
        };
        studio.app.modeller.cursor3d.fresh = true;
        co(&mut studio);
        let copy = studio.app.modeller.transform.as_ref().unwrap();
        assert_eq!(copy.kind, crate::app::transform::TransformKind::Copy);
        assert!(copy.points.is_empty());
        let _ = studio.update(Msg::App(Message::Escape));
        let _ = studio.update(Msg::App(Message::EditMode));
        click(&mut studio.app, 2.0, 0.0);
        let original = studio.app.modeller.sketch.as_ref().unwrap().segments[0];
        studio.app.camera.ortho = false;
        co(&mut studio);
        assert!(!studio.app.camera.ortho);
        click(&mut studio.app, 10.0, 10.0);
        click(&mut studio.app, 12.0, 13.0);
        let sketch = studio.app.modeller.sketch.as_ref().unwrap();
        assert_eq!(sketch.segments.len(), 5);
        assert_eq!(sketch.segments[0], original);
        let copy = sketch.segments[4];
        assert!(copy.a.distance(original.a + DVec2::new(2.0, 3.0)) < MEET);
        assert!(copy.b.distance(original.b + DVec2::new(2.0, 3.0)) < MEET);
    }

    #[test]
    fn a_rectangle_drawn_on_a_floor_and_trimmed_becomes_its_outline() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().map(|s| s.segments.len()), Some(4), "Tab opens the sketch");
        assert!(app.modeller.edit.is_none());
        // a rectangle over the east side: from (3, 1) to (6, 2)
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 3.0, 1.0);
        click(&mut app, 6.0, 2.0);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 8);
        // as it is, the two loops cross: the kernel makes no floor of them, the sketch stays
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_some() && app.modeller.refusal.is_some());
        // trim what lies inside: the rectangle's west side with its two stubs, and the floor's
        // east side between them
        let _ = app.update(Message::EditKey('t'));
        click(&mut app, 3.0, 1.5);
        click(&mut app, 3.5, 1.0);
        click(&mut app, 3.5, 2.0);
        click(&mut app, 4.0, 1.5);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.sketch.as_ref().map(|s| (s.error, s.segments.clone())));
        let (outline, holes, bulges) = slab_of(&app, id);
        assert_eq!((outline.len(), holes.len(), bulges.len()), (8, 0, 0), "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 2.0)).abs() < 1e-9, "the floor and the part of the rectangle outside it");
        assert_eq!(app.selected_modelled(), vec![id]);
        // one step to take back
        let _ = app.update(Message::Undo);
        assert_eq!(slab_of(&app, id).0.len(), 4);
    }

    #[test]
    fn a_loop_inside_the_outline_is_an_opening_and_opens_again_as_lines() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 2.0, 2.0);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.refusal);
        let (outline, holes, _) = slab_of(&app, id);
        assert_eq!((outline.len(), holes.len()), (4, 1));
        assert!((area2(&holes[0]) - 2.0).abs() < 1e-9);
        // opened again the opening's sides are lines too; removed, the floor is whole again
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 8);
        for _ in 0..4 {
            if let Some(sketch) = app.modeller.sketch.as_mut() {
                sketch.picked = Some(4);
            }
            let _ = app.update(Message::DeletePicked);
        }
        let _ = app.update(Message::EditMode);
        assert_eq!(slab_of(&app, id).1.len(), 0);
        // an opening outside the outline is refused
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('r'));
        click(&mut app, 6.0, 1.0);
        click(&mut app, 7.0, 2.0);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_some() && app.modeller.refusal.is_some());
    }

    #[test]
    fn a_side_drawn_as_an_arc_is_kept_as_an_arc() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        // the east side goes, an arc from corner to corner through (5, 1.5) takes its place
        click(&mut app, 4.0, 1.5);
        let _ = app.update(Message::DeletePicked);
        let _ = app.update(Message::EditKey('a'));
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 3.0);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.second.is_some()));
        click(&mut app, 5.0, 1.5);
        let _ = app.update(Message::EditMode);
        assert!(app.modeller.sketch.is_none(), "{:?}", app.modeller.refusal);
        let (outline, _, bulges) = slab_of(&app, id);
        assert_eq!(outline.len(), 4);
        assert_eq!(bulges.iter().filter(|b| **b != 0.0).count(), 1, "{bulges:?}");
        // it bows outward: the floor is larger than its four corners
        let ring = bowed(&outline, &bulges);
        assert!(ring.iter().any(|p| p.distance(DVec2::new(5.0, 1.5)) < 1e-6), "{ring:?}");
        assert!(area2(&ring) > 2.0 * 13.5);
        // opened again the arc is still one line
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let sketch = app.modeller.sketch.as_ref().unwrap();
        assert_eq!((sketch.segments.len(), sketch.segments.iter().filter(|s| s.bulge != 0.0).count()), (4, 1));
    }

    #[test]
    fn esc_leaves_the_sketch_with_the_floor_unchanged_and_a_gap_is_told() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        // a line picked and removed: the outline has a gap, Tab says so and the sketch stays
        click(&mut app, 2.0, 0.0);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().picked, Some(0));
        let _ = app.update(Message::DeletePicked);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments.len(), 3);
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().error, Some(SketchError::Gap));
        assert!(app.sketch_hint(Language::English).unwrap().contains("not closed"));
        // a line drawn back in, from corner to corner (the clicks land on the points)
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 0.05, 0.05);
        click(&mut app, 3.95, 0.05);
        assert_eq!(app.modeller.sketch.as_ref().unwrap().segments[3], seg((0.0, 0.0), (4.0, 0.0)));
        assert_eq!(app.modeller.sketch.as_ref().unwrap().tool, SketchTool::Pick);
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 8.0, 8.0);
        // Esc: the line under way stops, then the tool goes, then the sketch leaves
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.first.is_none() && s.tool == SketchTool::Line));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.tool == SketchTool::Pick));
        let _ = app.update(Message::Escape);
        assert!(app.modeller.sketch.is_none());
        assert_eq!(slab_of(&app, id).0.len(), 4, "the floor as it was");
    }

    #[test]
    fn line_and_arc_share_the_endpoint_and_enter_commits_only_a_closed_floor() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        click(&mut app, 2.0, 0.0);
        let _ = app.update(Message::DeletePicked);
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 2.0, -1.0);
        let _ = app.update(Message::MeasureFinish);
        assert!(app.modeller.sketch.as_ref().unwrap().error.is_some());
        let _ = app.update(Message::EditKey('a'));
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, -3.0);
        let sketch = app.modeller.sketch.as_ref().unwrap();
        assert_eq!(sketch.tool, SketchTool::Pick);
        let _ = app.update(Message::MeasureFinish);
        assert!(app.modeller.sketch.is_none());
        let (outline, _, bows) = slab_of(&app, id);
        assert_eq!(outline.len(), 5);
        assert_eq!(bows.iter().filter(|b| **b != 0.0).count(), 1);
    }

    #[test]
    fn the_sketchs_buttons_take_a_tool_finish_and_cancel() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let p = app.theme.palette();
        let said = |app: &App, label: &str| -> Vec<Message> {
            let mut ui = iced_test::Simulator::with_size(iced::Settings::default(), iced::Size::new(900.0, 200.0), app.sketch_bar(p, Language::English).expect("the bar"));
            ui.click(label).unwrap_or_else(|_| panic!("no {label}"));
            ui.into_messages().collect()
        };
        for m in said(&app, "Rectangle (R)") {
            let _ = app.update(m);
        }
        assert_eq!(app.modeller.sketch.as_ref().map(|s| s.tool), Some(SketchTool::Rectangle));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 2.0, 2.0);
        for m in said(&app, "Finish (Tab)") {
            let _ = app.update(m);
        }
        assert!(app.modeller.sketch.is_none());
        assert_eq!(slab_of(&app, id).1.len(), 1, "the rectangle became an opening");
        // Cancel leaves at once, with a tool on and a point clicked
        app.select_modelled(id);
        let _ = app.update(Message::EditMode);
        let _ = app.update(Message::EditKey('l'));
        click(&mut app, 3.0, 1.0);
        for m in said(&app, "Cancel") {
            let _ = app.update(m);
        }
        assert!(app.modeller.sketch.is_none());
        assert_eq!(slab_of(&app, id).1.len(), 1, "unchanged");
    }

    #[test]
    fn a_point_dragged_takes_the_ends_that_met_there() {
        let (mut app, id) = floor();
        let _ = app.update(Message::EditMode);
        let overlay = app.edit_overlay(Color::BLACK, Color::WHITE).expect("the sketch shows");
        assert_eq!((overlay.points.len(), overlay.lines.as_ref().map(Vec::len)), (4, Some(4)));
        // the corner at (4, 3), point 2, out to (5, 4)
        let (sx, sy) = app.on_screen(DVec3::new(5.0, 4.0, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Edit(EditEvent::Press { index: 2, add: false }));
        let _ = app.update(Message::Edit(EditEvent::Drag { x: sx, y: sy, width: 1000.0, height: 800.0 }));
        let _ = app.update(Message::Edit(EditEvent::Release));
        let _ = app.update(Message::EditMode);
        let (outline, ..) = slab_of(&app, id);
        assert!(outline.iter().any(|p| p.distance(DVec2::new(5.0, 4.0)) < 1e-9), "{outline:?}");
        assert!((area2(&outline) - 2.0 * (12.0 + 3.5)).abs() < 1e-9, "{outline:?}");
    }

    /// A new project with the Slab tool chosen as from the ribbon: the sketch of a new floor.
    fn new_floor() -> App {
        let mut app = modelling();
        let _ = app.update(Message::ModelTool(Some((crate::app::modeller::ModelTool::Slab, "Slab"))));
        app
    }

    fn slabs(app: &App) -> Vec<ElementId> {
        app.modeller.model.iter().filter(|e| e.class() == "IfcSlab").map(|e| e.id).collect()
    }

    #[test]
    fn the_slab_tool_opens_a_sketch_and_an_l_drawn_with_lines_becomes_a_floor() {
        let mut app = new_floor();
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.new.is_some() && s.tool == SketchTool::Line));
        for (x, y) in [(0.0, 0.0), (6.0, 0.0), (6.0, 2.0), (2.0, 2.0), (2.0, 5.0), (0.0, 5.0), (0.0, 0.0)] {
            click(&mut app, x, y);
        }
        assert!(slabs(&app).is_empty(), "not before it is finished");
        let _ = app.update(Message::EditMode);
        let made = slabs(&app);
        assert_eq!(made.len(), 1);
        let (outline, holes, _) = slab_of(&app, made[0]);
        assert_eq!((outline.len(), holes.len()), (6, 0));
        assert!((area2(&outline).abs() / 2.0 - 18.0).abs() < 1e-9);
        // the sketch and the tool are gone, the floor is selected; one step takes it back
        assert!(app.modeller.sketch.is_none() && app.modeller.tool.is_none());
        assert_eq!(app.selected_modelled(), made);
        let _ = app.update(Message::Undo);
        assert!(slabs(&app).is_empty());
    }

    #[test]
    fn a_circle_drawn_in_the_sketch_is_a_round_floor() {
        let mut app = new_floor();
        app.sketch_key('c');
        click(&mut app, 3.0, 3.0);
        click(&mut app, 5.0, 3.0);
        let sketch = app.modeller.sketch.as_ref().unwrap();
        // four quarter arcs, each bowing out to the circle
        assert_eq!(sketch.segments.len(), 4);
        assert!(sketch.segments.iter().all(|s| (s.at(0.5).distance(DVec2::new(3.0, 3.0)) - 2.0).abs() < 1e-9), "{:?}", sketch.segments);
        let _ = app.update(Message::EditMode);
        let made = slabs(&app);
        assert_eq!(made.len(), 1);
        let volume: f64 = app.modeller.model.evaluate(made[0]).unwrap().iter().map(|p| p.solid.volume()).sum();
        let thickness = app.modeller.placement.of(crate::app::modeller::ModelTool::Slab).width;
        // (the kernel makes the round of chords: a little less than the circle)
        assert!((volume / thickness - PI * 4.0).abs() < 0.15, "{volume}");
    }

    #[test]
    fn the_sides_of_walls_taken_over_end_in_their_corners_and_cancel_makes_nothing() {
        use bim_core::element::PlanarForm;
        let mut app = modelling();
        // four walls round a room of 4 by 3 (their axes), 0.2 thick
        let corners = [(0.0, 0.0), (4.0, 0.0), (4.0, 3.0), (0.0, 3.0)];
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let form = PlanarForm::Wall { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height: 3.0 };
            app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        }
        let _ = app.update(Message::ModelTool(Some((crate::app::modeller::ModelTool::Slab, "Slab"))));
        app.sketch_key('o');
        // a click on the inner side of each wall
        for (x, y) in [(2.0, 0.1), (3.9, 1.5), (2.0, 2.9), (0.1, 1.5)] {
            click(&mut app, x, y);
        }
        let _ = app.update(Message::EditMode);
        let made = slabs(&app);
        assert_eq!(made.len(), 1, "{:?} {:?}", app.modeller.sketch.as_ref().map(|s| (s.error, s.segments.clone())), app.modeller.refusal);
        let (outline, ..) = slab_of(&app, made[0]);
        // the room between the inner sides: 3.8 by 2.8
        assert!((area2(&outline).abs() / 2.0 - 3.8 * 2.8).abs() < 1e-9, "{outline:?}");
        // Cancel in a new sketch makes nothing and lets the tool go
        let _ = app.update(Message::ModelTool(Some((crate::app::modeller::ModelTool::Slab, "Slab"))));
        click(&mut app, 10.0, 10.0);
        click(&mut app, 12.0, 10.0);
        let _ = app.update(Message::SketchCancel);
        assert!(app.modeller.sketch.is_none() && app.modeller.tool.is_none());
        assert_eq!(slabs(&app).len(), 1);
    }

    fn stairs(app: &App) -> Vec<ElementId> {
        app.modeller.model.iter().filter(|e| e.class() == "IfcStair").map(|e| e.id).collect()
    }

    fn stair_of(app: &App, id: ElementId) -> (f64, f64, usize, bim_core::StairShape) {
        let Shape::Planar(PlanarDef { form: PlanarForm::Stair { base, height, risers, shape, .. }, .. }) = app.modeller.model.get(id).unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        (base, height, risers, shape)
    }

    #[test]
    fn two_clicks_of_the_stair_tool_draw_a_straight_stair_as_long_as_its_treads() {
        use crate::app::placement::{PlaceField, PlaceMessage};
        let mut app = modelling();
        let _ = app.update(Message::ModelTool(Some((crate::app::modeller::ModelTool::Stair, "Stair"))));
        assert!(app.modeller.sketch.is_none(), "the tool draws at once; its sketch is asked for");
        app.place_message(PlaceMessage::Field(PlaceField::Height, "3".into()));
        // (without its railing: the treads are counted)
        app.place_message(PlaceMessage::Railing(false));
        // the second click says the way: 3 m in 17 steps of 176 mm, 16 treads of 250 mm, 4 m long
        click(&mut app, 1.0, 1.0);
        click(&mut app, 2.0, 1.0);
        let made = stairs(&app);
        assert_eq!(made.len(), 1);
        let (base, height, risers, shape) = stair_of(&app, made[0]);
        assert_eq!((base, height, risers), (0.0, 3.0, 17));
        assert_eq!(shape, bim_core::StairShape::Run { path: vec![DVec2::new(1.0, 1.0), DVec2::new(5.0, 1.0)], width: 1.0 });
        assert_eq!(app.modeller.model.evaluate(made[0]).unwrap().len(), 16);
        // with the strip's Railing on (as it starts) the next has balusters and handrails too
        app.place_message(PlaceMessage::Railing(true));
        click(&mut app, 1.0, 12.0);
        click(&mut app, 2.0, 12.0);
        let railed = *stairs(&app).last().unwrap();
        assert_eq!(app.modeller.model.evaluate(railed).unwrap().len(), 16 + 2 * 16 + 2 * 15);
        app.modeller.model.remove(railed);
        app.place_message(PlaceMessage::Railing(false));
        // steps typed, and no going: from click to click
        app.place_message(PlaceMessage::Field(PlaceField::Sill, "13".into()));
        app.place_message(PlaceMessage::Field(PlaceField::Depth, "0".into()));
        click(&mut app, 1.0, 5.0);
        click(&mut app, 1.0, 8.0);
        let (_, _, risers, shape) = stair_of(&app, stairs(&app)[1]);
        assert_eq!((risers, shape), (13, bim_core::StairShape::Run { path: vec![DVec2::new(1.0, 5.0), DVec2::new(1.0, 8.0)], width: 1.0 }));
        // its steps are typed in its properties, and it shows what they come to
        let second = stairs(&app)[1];
        app.set_model_tool(None);
        app.select_modelled(second);
        let _ = app.update(Message::ElementField(crate::app::element_props::ElementField::Risers, "16".into()));
        assert_eq!(stair_of(&app, second).2, 16);
        let scene = app.model.as_ref().unwrap();
        let going: Vec<String> = scene.elements_with("Risers", "16").iter().flat_map(|e| scene.element_props(*e)).filter(|(k, _)| k == "Going" || k == "Riser height").map(|(_, v)| v).collect();
        assert_eq!(going, ["200 mm", "187.5 mm"]);
    }

    #[test]
    fn a_stair_is_sketched_in_an_outline_or_along_a_line_with_a_landing() {
        use crate::app::placement::{PlaceField, PlaceMessage};
        let stair_tool = |app: &mut App| {
            let _ = app.update(Message::ModelTool(Some((crate::app::modeller::ModelTool::Stair, "Stair"))));
            app.place_message(PlaceMessage::Field(PlaceField::Height, "3".into()));
            app.place_message(PlaceMessage::Railing(false));
            let _ = app.update(Message::StairSketch);
        };
        let mut app = modelling();
        stair_tool(&mut app);
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.stair && s.new.is_some()));
        // an outline alone is told to want its line
        app.sketch_key('r');
        click(&mut app, 0.0, 0.0);
        click(&mut app, 8.0, 6.0);
        let _ = app.update(Message::EditMode);
        assert_eq!(app.modeller.sketch.as_ref().and_then(|s| s.error), Some(SketchError::NoPath));
        // with the line from its foot to its head: 17 steps, 16 treads across the outline
        app.sketch_key('p');
        click(&mut app, 0.0, 3.0);
        click(&mut app, 8.0, 3.0);
        let _ = app.update(Message::EditMode);
        let made = stairs(&app);
        assert_eq!(made.len(), 1, "{:?}", app.modeller.refusal);
        let (_, height, risers, shape) = stair_of(&app, made[0]);
        assert!(matches!(shape, bim_core::StairShape::Outline { foot, head, .. } if foot == DVec2::new(0.0, 3.0) && head == DVec2::new(8.0, 3.0)));
        assert_eq!((height, risers, app.modeller.model.evaluate(made[0]).unwrap().len()), (3.0, 17, 16));
        assert!(app.modeller.sketch.is_none() && app.modeller.tool.is_none());
        // a line alone with a corner: two flights and a landing, as wide as the strip says
        stair_tool(&mut app);
        app.sketch_key('p');
        for (x, y) in [(20.0, 0.0), (23.0, 0.0), (23.0, 3.0)] {
            click(&mut app, x, y);
        }
        let _ = app.update(Message::EditMode);
        let made = stairs(&app);
        assert_eq!(made.len(), 2, "{:?}", app.modeller.refusal);
        let (.., shape) = stair_of(&app, made[1]);
        assert_eq!(shape, bim_core::StairShape::Run { path: vec![DVec2::new(20.0, 0.0), DVec2::new(23.0, 0.0), DVec2::new(23.0, 3.0)], width: 1.0 });
        assert_eq!(app.modeller.model.evaluate(made[1]).unwrap().len(), 16);
        // one step takes it back
        let _ = app.update(Message::Undo);
        assert_eq!(stairs(&app).len(), 1);
    }

    fn roofs(app: &App) -> Vec<(f64, Option<bim_core::Slope>)> {
        let grid = app.modeller.model.grid;
        let area = |ring: &[DVec2]| (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f64>().abs() / 2.0;
        app.modeller
            .model
            .iter()
            .filter(|e| e.class() == "IfcRoof")
            .filter_map(|e| match e.shape_in_project(&grid) {
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, slope, .. }, .. }) => Some(((area(&outline) * 1e6).round() / 1e6, slope)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_roof_tool_makes_a_plane_from_every_eave_and_a_gable_where_a_line_has_no_slope() {
        use crate::app::placement::{PlaceField, PlaceMessage};
        let mut app = modelling();
        let _ = app.update(Message::RoofTool("Roof"));
        assert!(app.modeller.sketch.as_ref().is_some_and(|s| s.new.is_some()) && app.sketching_roof());
        assert_eq!(app.modeller.placement.of(crate::app::modeller::ModelTool::Slab).sill, ROOF_PITCH);
        // a rectangle of 10 by 6, every line an eave at 30 degrees: a hip roof of four planes
        app.sketch_key('r');
        click(&mut app, 0.0, 0.0);
        click(&mut app, 10.0, 6.0);
        let _ = app.update(Message::EditMode);
        let mut made = roofs(&app);
        made.sort_by(|a, b| a.0.total_cmp(&b.0));
        assert_eq!(made.iter().map(|r| r.0).collect::<Vec<_>>(), [9.0, 9.0, 21.0, 21.0], "{:?}", app.modeller.refusal);
        assert!(made.iter().all(|r| r.1.is_some_and(|s| (s.angle.to_degrees() - 30.0).abs() < 1e-9)));
        assert!(app.modeller.sketch.is_none() && app.modeller.tool.is_none());
        // one step takes the whole roof back
        let _ = app.update(Message::Undo);
        assert!(roofs(&app).is_empty());
        // the short sides without a slope (H on the picked line), the long ones at 45: a gable roof
        let _ = app.update(Message::RoofTool("Roof"));
        app.place_message(PlaceMessage::Field(PlaceField::Sill, "45".into()));
        app.sketch_key('r');
        click(&mut app, 0.0, 0.0);
        click(&mut app, 10.0, 6.0);
        app.sketch_key('r');
        for (x, y) in [(0.0, 3.0), (10.0, 3.0)] {
            click(&mut app, x, y);
            assert!(app.modeller.sketch.as_ref().unwrap().picked.is_some());
            app.sketch_key('h');
        }
        let _ = app.update(Message::EditMode);
        let made = roofs(&app);
        assert_eq!(made.iter().map(|r| r.0).collect::<Vec<_>>(), [30.0, 30.0], "{:?}", app.modeller.refusal);
        // every line without a slope, or the strip's angle at 0: one flat roof
        let _ = app.update(Message::RoofTool("Roof"));
        app.place_message(PlaceMessage::Field(PlaceField::Sill, "0".into()));
        app.sketch_key('r');
        click(&mut app, 20.0, 0.0);
        click(&mut app, 24.0, 4.0);
        let _ = app.update(Message::EditMode);
        let made = roofs(&app);
        assert_eq!((made.len(), made[2]), (3, (16.0, None)));
        // a picked line gets an angle of its own: a single pitch of 10 degrees from that eave
        let _ = app.update(Message::RoofTool("Roof"));
        app.place_message(PlaceMessage::Field(PlaceField::Sill, "0".into()));
        app.sketch_key('r');
        click(&mut app, 30.0, 0.0);
        click(&mut app, 34.0, 4.0);
        app.sketch_key('r');
        click(&mut app, 32.0, 0.0);
        let _ = app.update(Message::SketchSlope("10".into()));
        let _ = app.update(Message::EditMode);
        let made = roofs(&app);
        assert_eq!(made.len(), 4, "{:?}", app.modeller.refusal);
        assert!(made[3].0 == 16.0 && made[3].1.is_some_and(|s| (s.angle.to_degrees() - 10.0).abs() < 1e-9), "{:?}", made[3]);
    }

    #[test]
    fn the_roof_tool_roofs_an_l_wing_by_wing() {
        let mut app = modelling();
        let _ = app.update(Message::RoofTool("Roof"));
        for (x, y) in [(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (4.0, 4.0), (4.0, 10.0), (0.0, 10.0), (0.0, 0.0)] {
            click(&mut app, x, y);
        }
        let _ = app.update(Message::EditMode);
        let mut made: Vec<f64> = roofs(&app).iter().map(|r| r.0).collect();
        made.sort_by(f64::total_cmp);
        // two hip roofs, 10 by 4 and 4 by 10: two triangles of 4 and two trapezia of 16 each
        assert_eq!(made, [4.0, 4.0, 4.0, 4.0, 16.0, 16.0, 16.0, 16.0], "{:?}", app.modeller.refusal);
        let _ = app.update(Message::Undo);
        assert!(roofs(&app).is_empty(), "one step");
    }
}
