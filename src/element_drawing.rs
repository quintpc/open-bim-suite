//! Element drawings: the drawings a prefab unit of a framed system (a timber frame, a steel frame)
//! is made from in the workshop.
//!
//! For every unit, made straight from the parts the kernel generates, without a view, a cut or
//! the graphics card: first the frame seen from its interior, every member as its outline; the
//! unit's length, its height, its studs centre to centre and its openings dimensioned; and under
//! it the list of its members, alike ones counted together and numbered, each member with its
//! number beside it; over it the unit cut level through all its layers, as thick as it is built.
//! The drawing carries the mark of the unit's assembly ([`bim_core::system::mark`]), as an
//! export names it. Then one drawing for every layer
//! across it that has boards or battens (the sheathing, the service zone, the lining, the cavity
//! battens, the cladding), seen the same way: every board and batten as its outline, the seams
//! between the boards dimensioned, and the list of what to cut. Membranes are not drawn. A drawing
//! is a [`Drawing2d`], so it is written in every format a view's drawing is.

use crate::drawing2d::{Align, Dim2d, Drawing2d, Line2d, Pen, Space, Text2d};
use bim_core::{Element, Model, Shape, LINEAR_TOLERANCE};
use glam::DVec2;
use std::iter::once;

/// The scale an element drawing is drawn at: 1 : 20.
pub const SCALE: f64 = 20.0;
/// Text on paper: 2.5 mm capitals (metres), and how far a dimension line stands off (metres).
const TEXT: f64 = 0.0025;
const OFFSET: f64 = 0.006;
/// Studs nearer to one another than this (metres) are dimensioned as one.
const APART: f64 = 0.1;

/// A layer thinner than this (metres) is a membrane: not drawn.
const MEMBRANE: f64 = 0.001;
/// More seams than this in a row are not dimensioned one by one (courses of weatherboards).
const SEAMS: usize = 16;

/// A member of a frame in the frame's face: from where to where along the frame and up it.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Member {
    u: (f64, f64),
    v: (f64, f64),
    /// What it looks like in the face where that is not the rectangle of `u` and `v`: a member
    /// that slopes, or one cut on a slope.
    shape: Option<Shape2>,
}

/// The outline of a part in the face that is not a level rectangle: its corners in order (the
/// first `n` of `points`), and its size as it is cut: its width and its length along itself.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape2 {
    points: [[f64; 2]; 8],
    n: usize,
    size: (f64, f64),
    /// How far off square its most sloping end is cut (degrees): the saw's setting.
    angle: f64,
}

impl Shape2 {
    /// The outline of `points` in the face (their convex hull), when it is not the level
    /// rectangle that holds them: none for a plain stud, plate or board.
    fn of(points: &[[f64; 2]]) -> Option<Self> {
        let mut sorted: Vec<[f64; 2]> = points.to_vec();
        sorted.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
        sorted.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-7 && (a[1] - b[1]).abs() < 1e-7);
        if sorted.len() < 3 {
            return None;
        }
        let turn = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
        let mut hull: Vec<[f64; 2]> = Vec::with_capacity(sorted.len() + 1);
        for pass in 0..2 {
            let start = hull.len();
            let walk: Box<dyn Iterator<Item = &[f64; 2]>> = if pass == 0 { Box::new(sorted.iter()) } else { Box::new(sorted.iter().rev()) };
            for q in walk {
                while hull.len() >= start + 2 && turn(hull[hull.len() - 2], hull[hull.len() - 1], *q) <= 1e-12 {
                    hull.pop();
                }
                hull.push(*q);
            }
            hull.pop();
        }
        let n = hull.len();
        if !(3..=8).contains(&n) {
            return None;
        }
        // a side that is neither level nor plumb, long enough to be cut so
        let slopes = (0..n).any(|i| {
            let (a, b) = (hull[i], hull[(i + 1) % n]);
            (a[0] - b[0]).abs() > 1e-4 && (a[1] - b[1]).abs() > 1e-4 && (a[0] - b[0]).hypot(a[1] - b[1]) > 0.005
        });
        if !slopes {
            return None;
        }
        // as it is cut: the narrowest it is across any of its sides, and how long it is along that side
        let mut size = (f64::INFINITY, 0.0);
        let mut along_it = (1.0, 0.0);
        for i in 0..n {
            let (a, b) = (hull[i], hull[(i + 1) % n]);
            let long = (b[0] - a[0]).hypot(b[1] - a[1]);
            if long < 1e-9 {
                continue;
            }
            let (dx, dy) = ((b[0] - a[0]) / long, (b[1] - a[1]) / long);
            let (mut wide, mut from, mut to) = (0.0_f64, f64::INFINITY, f64::NEG_INFINITY);
            for q in &hull {
                let (along, off) = ((q[0] - a[0]) * dx + (q[1] - a[1]) * dy, ((q[0] - a[0]) * dy - (q[1] - a[1]) * dx).abs());
                (wide, from, to) = (wide.max(off), from.min(along), to.max(along));
            }
            if wide < size.0 {
                size = (wide, to - from);
                along_it = (dx, dy);
            }
        }
        // its ends: the sides that do not run along it; a square cut lies across it
        let mut angle = 0.0_f64;
        for i in 0..n {
            let (a, b) = (hull[i], hull[(i + 1) % n]);
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let (run, rise) = ((ex * along_it.0 + ey * along_it.1).abs(), (ex * along_it.1 - ey * along_it.0).abs());
            if rise > 0.005 && run > 1e-4 {
                angle = angle.max(run.atan2(rise).to_degrees());
            }
        }
        let mut points = [[0.0; 2]; 8];
        points[..n].copy_from_slice(&hull);
        Some(Shape2 { points, n, size, angle })
    }
}

/// A part of the system in its face, with where it is across it (to its exterior positive).
struct Item<'a> {
    at: Member,
    across: (f64, f64),
    member: bool,
    material: Option<&'a str>,
}

impl Member {
    /// Its size in the face and its length (metres): a stud's width and height, a plate's height
    /// and length.
    fn section(&self) -> (f64, f64) {
        if let Some(shape) = self.shape {
            return shape.size;
        }
        let (du, dv) = (self.u.1 - self.u.0, self.v.1 - self.v.0);
        (du.min(dv), du.max(dv))
    }

    fn upright(&self) -> bool {
        self.v.1 - self.v.0 > self.u.1 - self.u.0
    }
}

/// Millimetres, whole: a half is rounded up whichever side of it the sum of its coordinates
/// fell (a brick 52.5 high is 53 in every row of the list).
fn mm(metres: f64) -> i64 {
    ((metres * 10_000.0).round() / 10.0).round() as i64
}

/// The drawings of `element` of `model`: for every prefab unit it is made of, in order from its
/// start, the drawing of its frame and then one of every layer with boards or battens; and last,
/// for a frame of several units, their [`overview`]. `named`
/// gives a material's name for its key. A standing system that is not framed gets its
/// [`elevation`], a lying one its [`laying_plan`]. None for an element that is not a system with
/// a type, or that the kernel refuses.
pub fn element_drawings(model: &Model, element: &Element, named: &dyn Fn(&str) -> String) -> Vec<Drawing2d> {
    let Shape::System(s) = &element.shape else { return Vec::new() };
    if s.lies() {
        return laying_plan(model, element, s, named).into_iter().collect();
    }
    let Some(t) = s.system_type.as_ref().filter(|t| !t.lying) else { return Vec::new() };
    if t.framing.is_none() {
        return elevation(model, element, s, t, named).into_iter().collect();
    }
    let Some(dir) = (s.end - s.start).try_normalize() else { return Vec::new() };
    let (Ok(parts), Ok(units)) = (model.evaluate(element.id), bim_core::system::joined_units(model, element)) else { return Vec::new() };
    // its own openings and those of the doors and windows standing in it
    let openings = model.openings_of(element.id);
    let left = dir.perp();
    let (half, tol) = (s.member.y / 2.0, 1e-6);
    let items: Vec<Item> = parts
        .iter()
        .filter(|p| p.tile == element.tile)
        .map(|p| {
            let (mut u, mut v, mut across) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
            for q in &p.solid.positions {
                let flat = DVec2::new(q.x, q.y) - s.start;
                let (a, b, c) = (flat.dot(dir), q.z - s.base, -flat.dot(left));
                u = (u.0.min(a), u.1.max(a));
                v = (v.0.min(b), v.1.max(b));
                across = (across.0.min(c), across.1.max(c));
            }
            let face: Vec<[f64; 2]> = p.solid.positions.iter().map(|q| [(DVec2::new(q.x, q.y) - s.start).dot(dir), q.z - s.base]).collect();
            Item { at: Member { u, v, shape: Shape2::of(&face) }, across, member: p.class == "IfcMember", material: p.material.as_deref() }
        })
        .collect();
    // the members of the frame itself: of the frame's material, inside the frame and as deep as
    // it (a cold-formed stud stands between the flanges of its tracks)
    let members: Vec<Member> = items
        .iter()
        .filter(|i| i.member && i.material == Some(t.member_material.as_str()))
        .filter(|i| i.across.0 >= -half - tol && i.across.1 <= half + tol && i.across.1 - i.across.0 > 0.9 * s.member.y)
        .map(|i| i.at)
        .collect();
    if members.is_empty() {
        return Vec::new();
    }
    // the frame starts where its first member does: at a corner that is not at its line's start
    let start = members.iter().map(|m| m.u.0).fold(f64::INFINITY, f64::min);
    let (text, off) = (TEXT * SCALE, OFFSET * SCALE);
    let dim = |d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], rows: f64| {
        let value = (to[0] - from[0]).hypot(to[1] - from[1]);
        if value > LINEAR_TOLERANCE {
            d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [from,to], from, to, value, offset: off * rows, text_height: text, style: crate::document::DimensionStyle::standard() });
        }
    };
    let outline = |d: &mut Drawing2d, pen: Pen, m: &Member, lo: f64| {
        // a member that slopes or is cut on a slope as it is; any other as its rectangle
        if let Some(shape) = m.shape {
            for i in 0..shape.n {
                let (a, b) = (shape.points[i], shape.points[(i + 1) % shape.n]);
                d.lines.push(Line2d::new(pen, [a[0] - lo, a[1]], [b[0] - lo, b[1]]));
            }
            return;
        }
        let (x0, x1, y0, y1) = (m.u.0 - lo, m.u.1 - lo, m.v.0, m.v.1);
        for (a, b) in [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
            d.lines.push(Line2d::new(pen, a, b));
        }
    };
    let mut drawings = Vec::with_capacity(units.len());
    let mut lo = start;
    for (k, length) in units.iter().enumerate() {
        let hi = lo + length;
        let mark = bim_core::system::mark(element, k + 1);
        let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: mark.clone(), space: Space::Model { scale: SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
        let mut own: Vec<Member> = members.iter().copied().filter(|m| (m.u.0 + m.u.1) / 2.0 > lo - tol && (m.u.0 + m.u.1) / 2.0 < hi + tol).collect();
        own.sort_by(|a, b| a.u.0.total_cmp(&b.u.0).then(a.v.0.total_cmp(&b.v.0)));
        for m in &own {
            outline(&mut d, Pen::Cut, m, lo);
        }
        let height = s.height;
        // over it: its studs from plate to plate, centre to centre from either end
        // (a stud under a sloping cut stands on the foot and is cut short: it counts too)
        let stud = |m: &&Member| m.upright() && (m.v.1 - m.v.0 > height * 0.9 || (m.shape.is_some() && m.v.0 < 0.1 && m.v.1 - m.v.0 > height * 0.25));
        let mut studs: Vec<f64> = own.iter().filter(stud).map(|m| (m.u.0 + m.u.1) / 2.0 - lo).collect();
        studs.sort_by(f64::total_cmp);
        studs.dedup_by(|a, b| (*a - *b).abs() < tol);
        // studs that stand against one another, or against the unit's end, are one place to measure to
        let mut chain: Vec<f64> = vec![0.0];
        for x in studs.iter().copied().filter(|x| *x > APART && *x < length - APART) {
            if chain.last().is_some_and(|last| x - last >= APART) {
                chain.push(x);
            }
        }
        chain.push(*length);
        for pair in chain.windows(2) {
            dim(&mut d, [pair[0], height], [pair[1], height], 1.0);
        }
        // under it: its openings along it, and its whole length (drawn from right to left, so
        // the dimension lines stand below)
        let inside: Vec<_> = openings.iter().filter(|o| o.along > lo - tol && o.along + o.width < hi + tol).collect();
        let crosses = |d: &mut Drawing2d| {
            for o in &inside {
                let (a, b, foot, head) = (o.along - lo, o.along + o.width - lo, o.sill, o.sill + o.height);
                // the opening: its outline and a cross through it
                for (p, q) in [([a, foot], [b, foot]), ([b, foot], [b, head]), ([b, head], [a, head]), ([a, head], [a, foot]), ([a, foot], [b, head]), ([a, head], [b, foot])] {
                    d.lines.push(Line2d::new(Pen::Detail, p, q));
                }
            }
        };
        crosses(&mut d);
        let mut along: Vec<f64> = vec![0.0];
        along.extend(inside.iter().flat_map(|o| [o.along - lo, o.along + o.width - lo]));
        along.push(*length);
        along.sort_by(f64::total_cmp);
        let rows = if inside.is_empty() { 1.0 } else { 2.0 };
        if !inside.is_empty() {
            for pair in along.windows(2) {
                dim(&mut d, [pair[1], 0.0], [pair[0], 0.0], 1.0);
            }
        }
        dim(&mut d, [*length, 0.0], [0.0, 0.0], rows);
        // to its left its height; to its right the sill and the head of its first opening
        dim(&mut d, [0.0, 0.0], [0.0, height], 1.0);
        if let Some(o) = inside.first() {
            let mut up: Vec<f64> = vec![0.0, o.sill, o.sill + o.height, height];
            up.dedup_by(|a, b| (*a - *b).abs() < tol);
            for pair in up.windows(2) {
                dim(&mut d, [*length, pair[1]], [*length, pair[0]], 1.0);
            }
        }
        // its name, and the list of its members: alike ones counted together, the longest first
        let depth = mm(s.member.y);
        let title = |what: &str| format!("{mark} · {}/{} · {what}{} × {} mm · 1:{}", k + 1, units.len(), mm(*length), mm(height), SCALE as i64);
        let counted: Vec<(i64, i64)> = own.iter().map(|m| (mm(m.section().0), mm(m.section().1))).collect();
        let numbers = write_list(&mut d, -(off * (rows + 1.5) + text), text, title(""), &counted, &|width, long| format!("{width}×{depth} L={long}"));
        number(&mut d, &own, &numbers, lo, text);
        // over it the unit cut level, a metre up (lower in a low one), through all its layers
        let (first, last) = (k == 0, k + 1 == units.len());
        let cut = height.min(2.5) * 0.4;
        let through: Vec<&Item> = items
            .iter()
            .filter(|i| i.at.v.0 < cut && i.at.v.1 > cut && i.across.1 - i.across.0 >= MEMBRANE)
            .filter(|i| (i.at.u.1 > lo + 1e-4 || first) && (i.at.u.0 < hi - 1e-4 || last))
            .collect();
        if let Some((inner, outer)) = through.iter().map(|i| i.across).reduce(|a, b| (a.0.min(b.0), a.1.max(b.1))) {
            // its interior face towards the frame's elevation
            let foot = height + off * 3.5 - inner;
            let mut left = f64::INFINITY;
            for i in &through {
                let (x0, x1) = (if first { i.at.u.0 } else { i.at.u.0.max(lo) } - lo, if last { i.at.u.1 } else { i.at.u.1.min(hi) } - lo);
                let (y0, y1) = (foot + i.across.0, foot + i.across.1);
                let pen = if i.member { Pen::Cut } else { Pen::Beyond };
                for (a, b) in [([x0, y0], [x1, y0]), ([x1, y0], [x1, y1]), ([x1, y1], [x0, y1]), ([x0, y1], [x0, y0])] {
                    d.lines.push(Line2d::new(pen, a, b));
                }
                left = left.min(x0);
            }
            dim(&mut d, [left, foot + inner], [left, foot + outer], 1.0);
        }
        drawings.push(d);
        // then every layer across it with boards or battens, from the interior out
        let mut layers: Vec<(usize, &bim_library::SystemLayer)> = t.layers.iter().enumerate().collect();
        layers.sort_by(|a, b| a.1.offset.total_cmp(&b.1.offset));
        for (n, l) in layers {
            let thick = l.members.as_ref().map_or(0.0, |m| m.depth).max(l.panels.thickness);
            if thick < MEMBRANE {
                continue;
            }
            let of_layer = |i: &&Item| {
                let made_of = i.material == Some(l.panels.material.as_str()) || l.members.as_ref().is_some_and(|m| i.member && i.material == Some(m.material.as_str()));
                made_of && i.across.0 >= l.offset - thick / 2.0 - tol && i.across.1 <= l.offset + thick / 2.0 + tol && (i.at.u.1 > lo + 1e-4 || first) && (i.at.u.0 < hi - 1e-4 || last)
            };
            // what runs on past the unit (the plates of a layer that is not made in units) is
            // drawn as far as the unit goes
            let within = |i: &Item| Member { u: (if first { i.at.u.0 } else { i.at.u.0.max(lo) }, if last { i.at.u.1 } else { i.at.u.1.min(hi) }), v: i.at.v, shape: i.at.shape };
            let mut own: Vec<Item> = items.iter().filter(of_layer).map(|i| Item { at: within(i), across: i.across, member: i.member, material: i.material }).collect();
            // a batten layer shows its battens; the wool between them is not cut to a list
            if own.iter().any(|i| i.member) {
                own.retain(|i| i.member);
            }
            if own.is_empty() {
                continue;
            }
            own.sort_by(|a, b| a.at.u.0.total_cmp(&b.at.u.0).then(a.at.v.0.total_cmp(&b.at.v.0)));
            let key = l.members.as_ref().filter(|_| own[0].member).map_or(l.panels.material.as_str(), |m| m.material.as_str());
            let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: format!("{mark} L{} {key}", n + 1), space: Space::Model { scale: SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
            for i in &own {
                outline(&mut d, Pen::Cut, &i.at, lo);
            }
            crosses(&mut d);
            // what it covers, as it reaches past the frame at a corner
            let (u0, u1) = own.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |acc, i| (acc.0.min(i.at.u.0 - lo), acc.1.max(i.at.u.1 - lo)));
            let (v0, v1) = own.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |acc, i| (acc.0.min(i.at.v.0), acc.1.max(i.at.v.1)));
            // over it the seams between its boards along it (the centres of its battens), to its
            // right those up it, unless there are too many to read
            let merged = |mut at: Vec<f64>| {
                at.sort_by(f64::total_cmp);
                at.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
                at
            };
            let battens = own[0].member;
            let seams_along = merged(if battens { once(u0).chain(own.iter().filter(|i| i.at.upright()).map(|i| (i.at.u.0 + i.at.u.1) / 2.0 - lo)).chain(once(u1)).collect() } else { own.iter().flat_map(|i| [i.at.u.0 - lo, i.at.u.1 - lo]).collect() });
            let seams_up = merged(if battens { vec![v0, v1] } else { own.iter().flat_map(|i| [i.at.v.0, i.at.v.1]).collect() });
            if seams_along.len() > 2 && seams_along.len() <= SEAMS {
                for pair in seams_along.windows(2) {
                    dim(&mut d, [pair[0], v1], [pair[1], v1], 1.0);
                }
            }
            if seams_up.len() > 2 && seams_up.len() <= SEAMS {
                for pair in seams_up.windows(2) {
                    dim(&mut d, [u1, pair[1]], [u1, pair[0]], 1.0);
                }
            }
            dim(&mut d, [u1, v0], [u0, v0], 1.0);
            dim(&mut d, [u0, v0], [u0, v1], 1.0);
            let counted: Vec<(i64, i64)> = own.iter().map(|i| (mm(i.at.section().0), mm(i.at.section().1))).collect();
            let across = mm(thick);
            let numbers = write_list(&mut d, v0 - (off * 2.5 + text), text, title(&format!("{} {across} · ", named(key))), &counted, &|width, long| if battens { format!("{width}×{across} L={long}") } else { format!("{across} × {width} × {long}") });
            number(&mut d, &own.iter().map(|i| i.at).collect::<Vec<_>>(), &numbers, lo, text);
            drawings.push(d);
        }
        lo = hi;
    }
    // a frame of several units: last, where each of them stands in it
    if units.len() > 1 {
        drawings.push(overview(element, &units, &members, &openings, start, s.height));
    }
    drawings
}

/// The overview of a frame made of several prefab units, at 1 : 50, seen as its units are: each
/// unit as its outline with its number under it, the frame's members thin inside it, its openings
/// crossed; the units' lengths, the frame's length and its height dimensioned, and the list of
/// the units with their sizes. The site puts the units up from it.
fn overview(element: &Element, units: &[f64], members: &[Member], openings: &[bim_core::Opening], start: f64, height: f64) -> Drawing2d {
    let (text, off) = (TEXT * PLAN_SCALE, OFFSET * PLAN_SCALE);
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: format!("{} {} overview", element.name, element.id.0), space: Space::Model { scale: PLAN_SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    let rectangle = |d: &mut Drawing2d, pen: Pen, u: (f64, f64), v: (f64, f64)| {
        for (a, b) in [([u.0, v.0], [u.1, v.0]), ([u.1, v.0], [u.1, v.1]), ([u.1, v.1], [u.0, v.1]), ([u.0, v.1], [u.0, v.0])] {
            d.lines.push(Line2d::new(pen, a, b));
        }
    };
    for m in members {
        rectangle(&mut d, Pen::Beyond, (m.u.0 - start, m.u.1 - start), m.v);
    }
    for o in openings {
        let (a, b, foot, head) = (o.along - start, o.along + o.width - start, o.sill, o.sill + o.height);
        d.lines.push(Line2d::new(Pen::Detail, [a, foot], [b, head]));
        d.lines.push(Line2d::new(Pen::Detail, [a, head], [b, foot]));
        rectangle(&mut d, Pen::Detail, (a, b), (foot, head));
    }
    let total: f64 = units.iter().sum();
    let mut lo = 0.0;
    let mut y = -(off * 5.0 + text);
    let title = format!("{} · {} × {} mm · 1:{}", element.name, mm(total), mm(height), PLAN_SCALE as i64);
    d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text * 1.4, text: title, align: Align::Left, angle: 0.0 });
    y -= text * 1.4 * 1.8;
    for (k, length) in units.iter().enumerate() {
        let mark = bim_core::system::mark(element, k + 1);
        rectangle(&mut d, Pen::Cut, (lo, lo + length), (0.0, height));
        d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [[lo, height], [lo + length, height]], from: [lo, height], to: [lo + length, height], value: *length, offset: off, text_height: text, style: crate::document::DimensionStyle::standard() });
        // its number large under it, clear of its members and openings; its whole mark in the list
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [lo + length / 2.0, -text * 2.4], height: text * 2.0, text: (k + 1).to_string(), align: Align::Center, angle: 0.0 });
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text, text: format!("{}  {mark} · {} × {} mm", k + 1, mm(*length), mm(height)), align: Align::Left, angle: 0.0 });
        y -= text * 1.8;
        lo += length;
    }
    // the whole length under the numbers
    d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [[total, 0.0], [0.0, 0.0]], from: [total, 0.0], to: [0.0, 0.0], value: total, offset: off * 2.5, text_height: text, style: crate::document::DimensionStyle::standard() });
    d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [[0.0, 0.0], [0.0, height]], from: [0.0, 0.0], to: [0.0, height], value: height, offset: off, text_height: text, style: crate::document::DimensionStyle::standard() });
    d
}

/// The scale a laying plan is drawn at: 1 : 50.
pub const PLAN_SCALE: f64 = 50.0;

/// The laying plan of a floor made as a system (hollow-core slabs, wide-slab planks): seen from
/// above, every slab as the outline of its top (cut to the floor's outline, around its openings,
/// with the cores drilled through it), the trimmers that carry cut slabs, the floor's length and
/// width and the seams between its slabs dimensioned, and the list of its slabs and trimmers.
/// What lies on the slabs (a topping) is named in the title, not drawn.
///
/// A system with nothing between its members (a board ceiling on channels, a ceiling of slats) is
/// drawn as what hangs there: its members as their outlines, and over them the boards of its
/// lowest layer, the one that is seen; the seams between those boards dimensioned, and the lists
/// of the boards (as many times as it has layers of them) and of the members.
fn laying_plan(model: &Model, element: &Element, s: &bim_core::SystemDef, named: &dyn Fn(&str) -> String) -> Option<Drawing2d> {
    let t = s.system_type.as_ref()?;
    if t.panel_material == "air" {
        return open_plan(model, element, s, t, named);
    }
    let dir = (s.end - s.start).try_normalize()?;
    let left = dir.perp();
    let parts = model.evaluate(element.id).ok()?;
    let (text, off) = (TEXT * PLAN_SCALE, OFFSET * PLAN_SCALE);
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: format!("{} {}", element.name, element.id.0), space: Space::Model { scale: PLAN_SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    let flat = |q: &glam::DVec3| {
        let p = DVec2::new(q.x, q.y) - s.start;
        [p.dot(dir), p.dot(left)]
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    let mut slabs: Vec<(i64, i64)> = Vec::new();
    let mut trimmers: Vec<(i64, i64)> = Vec::new();
    let mut seams: Vec<f64> = Vec::new();
    for p in parts.iter().filter(|p| p.tile == element.tile) {
        let own = p.class == "IfcPlate" && p.material.as_deref() == Some(t.panel_material.as_str());
        let steel = p.class == "IfcMember" && p.material.as_deref() == Some("steel");
        if !own && !steel {
            continue;
        }
        let top = p.solid.positions.iter().map(|q| q.z).fold(f64::NEG_INFINITY, f64::max);
        let (mut a, mut b) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for q in &p.solid.positions {
            let at = flat(q);
            (a, b) = ([a[0].min(at[0]), a[1].min(at[1])], [b[0].max(at[0]), b[1].max(at[1])]);
        }
        if steel {
            // a trimmer: its angle as a rectangle (its lugs are part of it in the list)
            if b[0] - a[0] > 2.0 * (b[1] - a[1]) {
                for (p, q) in [([a[0], a[1]], [b[0], a[1]]), ([b[0], a[1]], [b[0], b[1]]), ([b[0], b[1]], [a[0], b[1]]), ([a[0], b[1]], [a[0], a[1]])] {
                    d.lines.push(Line2d::new(Pen::Detail, p, q));
                }
                trimmers.push((mm(b[1] - a[1]), mm(b[0] - a[0])));
            }
            continue;
        }
        // a slab: the edges of its top
        for face in &p.solid.faces {
            for ring in &face.rings {
                for k in 0..ring.len() {
                    let (q, r) = (p.solid.positions[ring[k] as usize], p.solid.positions[ring[(k + 1) % ring.len()] as usize]);
                    if (q.z - top).abs() < 1e-6 && (r.z - top).abs() < 1e-6 {
                        let (from, to) = (flat(&q), flat(&r));
                        // every edge lies in two faces: drawn once
                        if (from[0], from[1]) < (to[0], to[1]) {
                            d.lines.push(Line2d::new(Pen::Cut, from, to));
                        }
                    }
                }
            }
        }
        (lo, hi) = ([lo[0].min(a[0]), lo[1].min(a[1])], [hi[0].max(b[0]), hi[1].max(b[1])]);
        slabs.push((mm(b[0] - a[0]), mm(b[1] - a[1])));
        seams.extend([a[0], b[0]]);
    }
    if slabs.is_empty() {
        return None;
    }
    let dim = |d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], rows: f64| {
        let value = (to[0] - from[0]).hypot(to[1] - from[1]);
        if value > LINEAR_TOLERANCE {
            d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [from,to], from, to, value, offset: off * rows, text_height: text, style: crate::document::DimensionStyle::standard() });
        }
    };
    // over it the seams between its slabs, under it its length, to its left its width
    seams.sort_by(f64::total_cmp);
    seams.dedup_by(|a, b| (*a - *b).abs() < 0.05);
    if seams.len() > 2 && seams.len() <= SEAMS + 8 {
        for pair in seams.windows(2) {
            dim(&mut d, [pair[0], hi[1]], [pair[1], hi[1]], 1.0);
        }
    }
    dim(&mut d, [hi[0], lo[1]], [lo[0], lo[1]], 1.0);
    dim(&mut d, [lo[0], lo[1]], [lo[0], hi[1]], 1.0);
    let thick = mm(s.member.y.max(s.panel));
    let on_it: Vec<String> = t.layers.iter().filter(|l| l.offset > 0.0).map(|l| format!(" + {} {}", named(&l.panels.material), mm(l.panels.thickness))).collect();
    let title = format!("{} · {} {thick}{} · {} × {} mm · 1:{}", element.name, named(&t.panel_material), on_it.concat(), mm(hi[0] - lo[0]), mm(hi[1] - lo[1]), PLAN_SCALE as i64);
    let mut y = lo[1] - (off * 2.5 + text);
    write_list(&mut d, y, text, title, &slabs, &|width, long| format!("{thick} × {width} × {long}"));
    y -= text * 1.8 * (1.4 + d.texts.len() as f64 - 1.0);
    if !trimmers.is_empty() {
        let mut list: Vec<(i64, usize)> = Vec::new();
        for (_, long) in &trimmers {
            match list.iter_mut().find(|(l, _)| l == long) {
                Some((_, n)) => *n += 1,
                None => list.push((*long, 1)),
            }
        }
        for (long, n) in list {
            d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text, text: format!("{n} × {} L={long}", named("steel")), align: Align::Left, angle: 0.0 });
            y -= text * 1.8;
        }
    }
    Some(d)
}

/// A member narrower than this (metres) is a joint between panels, not a profile.
const JOINT: f64 = 0.015;
/// More parts than this are not drawn one by one (a wall of bricks).
const MOST: usize = 4000;

/// The elevation of a standing system on a grid (a curtain wall, a frame, a wall of elements,
/// cladding), seen from its interior at 1 : 50: every panel and every member as its outline, the
/// seams between the panels dimensioned along it and up it, its length and its height, each
/// panel with its number, its openings crossed, and the lists of the panels and of the members.
/// A system of more than [`MOST`] parts (brickwork) is drawn as its outline and its openings,
/// with its courses counted in the title and its bricks in the list.
fn elevation(model: &Model, element: &Element, s: &bim_core::SystemDef, t: &bim_core::SystemTypeDef, named: &dyn Fn(&str) -> String) -> Option<Drawing2d> {
    let dir = (s.end - s.start).try_normalize()?;
    let parts = model.evaluate(element.id).ok()?;
    // a wall of bricks: its outline, its courses counted and its bricks listed, not drawn
    let many = parts.len() > MOST;
    let (text, off) = (TEXT * PLAN_SCALE, OFFSET * PLAN_SCALE);
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: format!("{} {}", element.name, element.id.0), space: Space::Model { scale: PLAN_SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    let faced = |p: &bim_core::Part| {
        let (mut u, mut v) = ((f64::INFINITY, f64::NEG_INFINITY), (f64::INFINITY, f64::NEG_INFINITY));
        for q in &p.solid.positions {
            let (a, b) = ((DVec2::new(q.x, q.y) - s.start).dot(dir), q.z - s.base);
            (u, v) = ((u.0.min(a), u.1.max(a)), (v.0.min(b), v.1.max(b)));
        }
        Member { u, v, shape: None }
    };
    let own = || parts.iter().filter(|p| p.tile == element.tile);
    let panels: Vec<Member> = own().filter(|p| p.class == "IfcPlate" && p.material.as_deref() == Some(t.panel_material.as_str())).map(faced).collect();
    let members: Vec<Member> = own().filter(|p| p.class == "IfcMember" && p.material.as_deref() == Some(t.member_material.as_str())).map(faced).collect();
    if panels.is_empty() && members.is_empty() {
        return None;
    }
    let mut outline = |pen: Pen, m: &Member| {
        for (a, b) in [([m.u.0, m.v.0], [m.u.1, m.v.0]), ([m.u.1, m.v.0], [m.u.1, m.v.1]), ([m.u.1, m.v.1], [m.u.0, m.v.1]), ([m.u.0, m.v.1], [m.u.0, m.v.0])] {
            d.lines.push(Line2d::new(pen, a, b));
        }
    };
    // members thinner than [`JOINT`] (a joint of mortar or glue, a seam) are the lines between
    // the panels: not drawn twice
    let drawn = |m: &&Member| m.section().0 >= JOINT - 1e-9;
    let all = || panels.iter().chain(&members);
    let (u0, u1) = all().fold((f64::INFINITY, f64::NEG_INFINITY), |acc, m| (acc.0.min(m.u.0), acc.1.max(m.u.1)));
    let (v0, v1) = all().fold((f64::INFINITY, f64::NEG_INFINITY), |acc, m| (acc.0.min(m.v.0), acc.1.max(m.v.1)));
    if many {
        outline(Pen::Cut, &Member { u: (u0, u1), v: (v0, v1), shape: None });
    } else {
        for m in members.iter().filter(drawn) {
            outline(Pen::Cut, m);
        }
        for p in &panels {
            outline(if members.iter().any(|m| drawn(&m)) { Pen::Beyond } else { Pen::Cut }, p);
        }
    }
    // the lintels that carry it over its openings
    let lintels: Vec<(Member, Option<&str>)> = own().filter(|p| p.class == "IfcBeam").map(|p| (faced(p), p.material.as_deref())).collect();
    for (m, _) in &lintels {
        outline(Pen::Cut, m);
    }
    // its openings and those of the doors and windows standing in it: an outline and a cross
    let openings = model.openings_of(element.id);
    for o in &openings {
        let (a, b, foot, head) = (o.along, o.along + o.width, o.sill, o.sill + o.height);
        for (p, q) in [([a, foot], [b, foot]), ([b, foot], [b, head]), ([b, head], [a, head]), ([a, head], [a, foot]), ([a, foot], [b, head]), ([a, head], [b, foot])] {
            d.lines.push(Line2d::new(Pen::Detail, p, q));
        }
    }
    let dim = |d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], rows: f64| {
        let value = (to[0] - from[0]).hypot(to[1] - from[1]);
        if value > LINEAR_TOLERANCE {
            d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [from, to], from, to, value, offset: off * rows, text_height: text, style: crate::document::DimensionStyle::standard() });
        }
    };
    // the grid: the middles of the joints between the panels, from edge to edge
    let lines = |at: &dyn Fn(&Member) -> (f64, f64), lo: f64, hi: f64| {
        let mut edges: Vec<f64> = panels.iter().flat_map(|p| [at(p).0, at(p).1]).collect();
        edges.sort_by(f64::total_cmp);
        let mut grid = vec![lo];
        let mut k = 0;
        while k < edges.len() {
            // edges within a joint of one another are one line, at their middle
            let mut j = k;
            while j + 1 < edges.len() && edges[j + 1] - edges[k] < s.member.x.max(0.02) + 1e-6 {
                j += 1;
            }
            let middle = (edges[k] + edges[j]) / 2.0;
            if middle - lo > 0.05 && hi - middle > 0.05 {
                grid.push(middle);
            }
            k = j + 1;
        }
        grid.push(hi);
        grid
    };
    let (along, up) = (lines(&|m| m.u, u0, u1), lines(&|m| m.v, v0, v1));
    if along.len() > 2 && along.len() <= SEAMS + 16 {
        for pair in along.windows(2) {
            dim(&mut d, [pair[0], v1], [pair[1], v1], 1.0);
        }
    } else if !openings.is_empty() {
        // too many seams to read: where its openings are along it
        let mut at: Vec<f64> = once(u0).chain(openings.iter().flat_map(|o| [o.along, o.along + o.width])).chain(once(u1)).collect();
        at.sort_by(f64::total_cmp);
        for pair in at.windows(2) {
            dim(&mut d, [pair[0], v1], [pair[1], v1], 1.0);
        }
    }
    if up.len() > 2 && up.len() <= SEAMS {
        for pair in up.windows(2) {
            dim(&mut d, [u1, pair[1]], [u1, pair[0]], 1.0);
        }
    }
    dim(&mut d, [u1, v0], [u0, v0], 1.0);
    dim(&mut d, [u0, v0], [u0, v1], 1.0);
    // its name, the panels numbered, and the members
    let thick = bim_library::millimetres(s.panel, bim_library::Lang::En);
    // courses too many to dimension one by one are counted: how many, and how high each is
    // (counted by the grid: an opening cuts bricks, which is no course more)
    let rows = if s.spacing.y > LINEAR_TOLERANCE { ((v1 - v0) / s.spacing.y).round().max(1.0) as usize } else { up.len() - 1 };
    let courses = if up.len() - 1 > SEAMS { format!(" · {rows} × {}", bim_library::millimetres(s.spacing.y, bim_library::Lang::En)) } else { String::new() };
    let title = format!("{} · {} {thick} · {} × {} mm{courses} · 1:{}", element.name, named(&t.panel_material), mm(u1 - u0), mm(v1 - v0), PLAN_SCALE as i64);
    let mut y = v0 - (off * 2.5 + text);
    let before = d.texts.len();
    let counted: Vec<(i64, i64)> = panels.iter().map(|p| (mm(p.section().0), mm(p.section().1))).collect();
    let numbers = write_list(&mut d, y, text, title, &counted, &|width, long| format!("{thick} × {width} × {long}"));
    let listed = d.texts[before..].iter().filter(|t| t.at[0] == 0.0).count();
    // a number in every panel, where there is room for them
    if panels.len() <= 400 && !many {
        number(&mut d, &panels, &numbers, 0.0, text);
    }
    y -= text * 1.8 * (1.4 + listed as f64 - 1.0) + text * 0.9;
    let (width, depth) = (mm(s.member.x), mm(s.member.y));
    let mut list: Vec<(i64, usize)> = Vec::new();
    for m in members.iter().filter(drawn) {
        let long = mm(m.section().1);
        match list.iter_mut().find(|(l, _)| *l == long) {
            Some((_, n)) => *n += 1,
            None => list.push((long, 1)),
        }
    }
    list.sort_by(|a, b| b.0.cmp(&a.0));
    for (long, n) in list {
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text, text: format!("{n} × {} {width}×{depth} L={long}", named(&t.member_material)), align: Align::Left, angle: 0.0 });
        y -= text * 1.8;
    }
    // the lintels: as thick as the panels, alike ones counted together, the longest first
    let mut over: Vec<((i64, i64, &str), usize)> = Vec::new();
    for (m, material) in &lintels {
        let key = (mm(m.section().0), mm(m.section().1), material.unwrap_or(""));
        match over.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => over.push((key, 1)),
        }
    }
    over.sort_by(|a, b| b.0 .1.cmp(&a.0 .1));
    for ((high, long, material), n) in over {
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text, text: format!("{n} × {} {thick}×{high} L={long}", named(material)), align: Align::Left, angle: 0.0 });
        y -= text * 1.8;
    }
    Some(d)
}

/// The laying plan of a lying system with nothing between its members (see [`laying_plan`]).
fn open_plan(model: &Model, element: &Element, s: &bim_core::SystemDef, t: &bim_core::SystemTypeDef, named: &dyn Fn(&str) -> String) -> Option<Drawing2d> {
    let dir = (s.end - s.start).try_normalize()?;
    let left = dir.perp();
    let parts = model.evaluate(element.id).ok()?;
    let (text, off) = (TEXT * PLAN_SCALE, OFFSET * PLAN_SCALE);
    let mut d = Drawing2d { polygons: Vec::new(), images: Vec::new(), name: format!("{} {}", element.name, element.id.0), space: Space::Model { scale: PLAN_SCALE }, paper: None, lines: Vec::new(), texts: Vec::new(), dimensions: Vec::new() };
    // every part as the rectangle it covers, with where its underside is
    let boxed = |p: &bim_core::Part| {
        let (mut a, mut b, mut under) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2], f64::INFINITY);
        for q in &p.solid.positions {
            let flat = DVec2::new(q.x, q.y) - s.start;
            let at = [flat.dot(dir), flat.dot(left)];
            (a, b, under) = ([a[0].min(at[0]), a[1].min(at[1])], [b[0].max(at[0]), b[1].max(at[1])], under.min(q.z));
        }
        (a, b, under)
    };
    let own = || parts.iter().filter(|p| p.tile == element.tile);
    let members: Vec<([f64; 2], [f64; 2], f64)> = own().filter(|p| p.class == "IfcMember" && p.material.as_deref() == Some(t.member_material.as_str())).map(boxed).collect();
    // the boards that are seen: of the layer that hangs lowest
    let lining = t.layers.iter().filter(|l| l.panels.thickness >= MEMBRANE).min_by(|a, b| a.offset.total_cmp(&b.offset));
    let all: Vec<([f64; 2], [f64; 2], f64)> = lining.map(|l| own().filter(|p| p.class == "IfcPlate" && p.material.as_deref() == Some(l.panels.material.as_str())).map(boxed).collect()).unwrap_or_default();
    let lowest = all.iter().map(|b| b.2).fold(f64::INFINITY, f64::min);
    let boards: Vec<&([f64; 2], [f64; 2], f64)> = all.iter().filter(|b| b.2 < lowest + 1e-6).collect();
    if members.is_empty() && boards.is_empty() {
        return None;
    }
    let mut rectangle = |pen: Pen, a: [f64; 2], b: [f64; 2]| {
        for (p, q) in [([a[0], a[1]], [b[0], a[1]]), ([b[0], a[1]], [b[0], b[1]]), ([b[0], b[1]], [a[0], b[1]]), ([a[0], b[1]], [a[0], a[1]])] {
            d.lines.push(Line2d::new(pen, p, q));
        }
    };
    for (a, b, _) in &members {
        rectangle(if boards.is_empty() { Pen::Cut } else { Pen::Detail }, *a, *b);
    }
    for (a, b, _) in &boards {
        rectangle(Pen::Cut, *a, *b);
    }
    // as large as its boards are; without boards as its members reach
    let measured: Vec<&([f64; 2], [f64; 2], f64)> = if boards.is_empty() { members.iter().collect() } else { boards.clone() };
    let (lo, hi) = measured.into_iter().fold(([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]), |(lo, hi), (a, b, _)| ([lo[0].min(a[0]), lo[1].min(a[1])], [hi[0].max(b[0]), hi[1].max(b[1])]));
    let dim = |d: &mut Drawing2d, from: [f64; 2], to: [f64; 2], rows: f64| {
        let value = (to[0] - from[0]).hypot(to[1] - from[1]);
        if value > LINEAR_TOLERANCE {
            d.dimensions.push(Dim2d { first_witness: true, units_per_paper_metre: off / OFFSET, witnesses: [from, to], from, to, value, offset: off * rows, text_height: text, style: crate::document::DimensionStyle::standard() });
        }
    };
    // over it the seams between its boards along it, to its right those across it; without
    // boards the members that run across it, centre to centre
    let merged = |mut at: Vec<f64>| {
        at.sort_by(f64::total_cmp);
        at.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        at
    };
    let (along, across) = if boards.is_empty() {
        let upright = |m: &&([f64; 2], [f64; 2], f64)| m.1[1] - m.0[1] > m.1[0] - m.0[0];
        (merged(once(lo[0]).chain(members.iter().filter(upright).map(|m| (m.0[0] + m.1[0]) / 2.0)).chain(once(hi[0])).collect()), vec![lo[1], hi[1]])
    } else {
        (merged(boards.iter().flat_map(|b| [b.0[0], b.1[0]]).collect()), merged(boards.iter().flat_map(|b| [b.0[1], b.1[1]]).collect()))
    };
    if along.len() > 2 && along.len() <= SEAMS + 8 {
        for pair in along.windows(2) {
            dim(&mut d, [pair[0], hi[1]], [pair[1], hi[1]], 1.0);
        }
    }
    if across.len() > 2 && across.len() <= SEAMS + 8 {
        for pair in across.windows(2) {
            dim(&mut d, [hi[0], pair[1]], [hi[0], pair[0]], 1.0);
        }
    }
    dim(&mut d, [hi[0], lo[1]], [lo[0], lo[1]], 1.0);
    dim(&mut d, [lo[0], lo[1]], [lo[0], hi[1]], 1.0);
    // its name, then the boards (every layer of them is cut alike) and the members
    let (width, depth) = (mm(s.member.x), mm(s.member.y));
    let layers = lining.map_or(0, |l| t.layers.iter().filter(|o| o.panels.material == l.panels.material && o.panels.thickness >= MEMBRANE).count());
    let what = match lining {
        Some(l) if layers > 1 => format!("{} {layers}×{}", named(&l.panels.material), bim_library::millimetres(l.panels.thickness, bim_library::Lang::En).trim_end_matches(" mm").to_string()),
        Some(l) => format!("{} {}", named(&l.panels.material), bim_library::millimetres(l.panels.thickness, bim_library::Lang::En).trim_end_matches(" mm")),
        None => format!("{} {width}×{depth}", named(&t.member_material)),
    };
    let title = format!("{} · {what} · {} × {} mm · 1:{}", element.name, mm(hi[0] - lo[0]), mm(hi[1] - lo[1]), PLAN_SCALE as i64);
    let mut y = lo[1] - (off * 2.5 + text);
    let size = |b: &([f64; 2], [f64; 2], f64)| (mm((b.1[0] - b.0[0]).min(b.1[1] - b.0[1])), mm((b.1[0] - b.0[0]).max(b.1[1] - b.0[1])));
    let before = d.texts.len();
    if let Some(l) = lining.filter(|_| !boards.is_empty()) {
        let thick = bim_library::millimetres(l.panels.thickness, bim_library::Lang::En);
        let thick = thick.trim_end_matches(" mm");
        let counted: Vec<(i64, i64)> = boards.iter().flat_map(|b| std::iter::repeat_n(size(b), layers.max(1))).collect();
        write_list(&mut d, y, text, title, &counted, &|width, long| format!("{thick} × {width} × {long}"));
    } else {
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text * 1.4, text: title, align: Align::Left, angle: 0.0 });
    }
    // the members under one another after the boards, alike ones counted together, the longest first
    let lines = d.texts[before..].iter().filter(|t| t.at[0] == 0.0).count();
    y -= text * 1.8 * (1.4 + lines as f64 - 1.0) + text * 0.9;
    let mut list: Vec<(i64, usize)> = Vec::new();
    for m in &members {
        let long = size(m).1;
        match list.iter_mut().find(|(l, _)| *l == long) {
            Some((_, n)) => *n += 1,
            None => list.push((long, 1)),
        }
    }
    list.sort_by(|a, b| b.0.cmp(&a.0));
    for (long, n) in list {
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [0.0, y], height: text, text: format!("{n} × {} {width}×{depth} L={long}", named(&t.member_material)), align: Align::Left, angle: 0.0 });
        y -= text * 1.8;
    }
    Some(d)
}

/// Whether a drawing of [`element_drawings`] is a unit's frame, not a layer's or the overview.
pub fn is_frame(d: &Drawing2d) -> bool {
    !d.name.ends_with(" overview") && !d.name.rsplit(' ').nth(1).is_some_and(|w| w.starts_with('L') && w[1..].parse::<usize>().is_ok())
}

/// Writes under a drawing, from `y` down, its `title` and the list of its parts (each its size in
/// the face and its length, millimetres): alike ones counted together, the longest first, each
/// worded by `line` after its number in the list. Gives the sizes back in the order of the list:
/// a part's number is its size's place there, from 1.
fn write_list(d: &mut Drawing2d, mut y: f64, text: f64, title: String, parts: &[(i64, i64)], line: &dyn Fn(i64, i64) -> String) -> Vec<(i64, i64)> {
    let mut list: Vec<((i64, i64), usize)> = Vec::new();
    for key in parts {
        match list.iter_mut().find(|(k, _)| k == key) {
            Some((_, n)) => *n += 1,
            None => list.push((*key, 1)),
        }
    }
    list.sort_by(|a, b| b.0 .1.cmp(&a.0 .1).then(b.0 .0.cmp(&a.0 .0)));
    let mut write = |x: f64, size: f64, line: String, down: bool| {
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at: [x, y], height: size, text: line, align: Align::Left, angle: 0.0 });
        if down {
            y -= size * 1.8;
        }
    };
    write(0.0, text * 1.4, title, true);
    for (k, ((width, long), n)) in list.iter().enumerate() {
        write(0.0, text, (k + 1).to_string(), false);
        write(text * 3.0, text, format!("{n} × {}", line(*width, *long)), true);
    }
    list.into_iter().map(|(key, _)| key).collect()
}

/// Writes at every one of `parts` (drawn from `lo` along) its number in the list `numbers` of
/// [`write_list`]: in the middle of a board, beside a member that stands (those that follow one
/// another a step higher each, so their numbers stay apart), inside the frame along a member
/// that lies.
fn number(d: &mut Drawing2d, parts: &[Member], numbers: &[(i64, i64)], lo: f64, text: f64) {
    let top = parts.iter().map(|m| m.v.1).fold(f64::NEG_INFINITY, f64::max);
    let foot = parts.iter().map(|m| m.v.0).fold(f64::INFINITY, f64::min);
    let mut standing = 0usize;
    for m in parts {
        let key = (mm(m.section().0), mm(m.section().1));
        let Some(n) = numbers.iter().position(|k| *k == key) else { continue };
        let (du, dv) = (m.u.1 - m.u.0, m.v.1 - m.v.0);
        let (mid_u, mid_v) = ((m.u.0 + m.u.1) / 2.0 - lo, (m.v.0 + m.v.1) / 2.0);
        let at = if du.min(dv) > 4.0 * text {
            [mid_u - text / 2.0, mid_v - text / 2.0]
        } else if m.upright() {
            standing += 1;
            [m.u.1 - lo + 0.4 * text, m.v.0 + dv * (0.3 + 0.08 * (standing % 4) as f64)]
        } else if mid_v > (top + foot) / 2.0 {
            [mid_u, m.v.0 - 1.4 * text]
        } else {
            [mid_u, m.v.1 + 0.4 * text]
        };
        // one cut off square says by how much, for the saw
        let label = match m.shape.filter(|s| s.angle >= 0.5) {
            Some(s) => format!("{} · {:.0}°", n + 1, s.angle),
            None => (n + 1).to_string(),
        };
        d.texts.push(Text2d { family: None, bold: false, color: None, pen: Pen::Text, at, height: text, text: label, align: Align::Left, angle: 0.0 });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bim_core::{Opening, SystemDef, TileGrid};
    use bim_library::Library;

    /// The lines of a frame's elevation: its members, without the cut over it.
    fn elevation(d: &Drawing2d) -> impl Iterator<Item = &Line2d> {
        d.lines.iter().filter(|l| l.pen == Pen::Cut && l.a[1] <= 2.7 + 1e-6 && l.b[1] <= 2.7 + 1e-6)
    }

    fn frame(length: f64, openings: Vec<Opening>) -> (Model, bim_core::ElementId) {
        typed(length, openings, "system.timber-frame-140")
    }

    fn typed(length: f64, openings: Vec<Opening>, id: &str) -> (Model, bim_core::ElementId) {
        let generic = SystemDef { start: DVec2::new(2.0, 1.0), end: DVec2::new(2.0, 1.0 + length), base: 0.5, height: 2.7, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings, breaks: Vec::new(), outline: Vec::new(), grid: None, division: None };
        let s = generic.typed_from(&Library::starter(), id).unwrap();
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Frame", None, Shape::System(s))).unwrap();
        (m, id)
    }

    #[test]
    fn a_frame_gets_a_drawing_for_every_unit_with_its_members_dimensions_and_list() {
        // 7.2 m along +y from (2, 1), its base at 0.5: three units of 2400
        let (m, id) = frame(7.2, Vec::new());
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        let drawings: Vec<&Drawing2d> = all.iter().filter(|d| is_frame(d)).collect();
        assert_eq!(drawings.len(), 3);
        // last the overview: the three units side by side, each with its number, its length and
        // its line in the list, and the frame's whole length
        let over = all.last().unwrap();
        assert!(over.name == format!("Frame {} overview", id.0) && !is_frame(over));
        assert_eq!(over.lines.iter().filter(|l| l.pen == Pen::Cut).count(), 3 * 4);
        let values: Vec<i64> = over.dimensions.iter().map(|x| mm(x.value)).collect();
        assert_eq!(values, [2400, 2400, 2400, 7200, 2700]);
        for k in 1..=3 {
            assert!(over.texts.iter().any(|t| t.text == k.to_string()) && over.texts.iter().any(|t| t.text == format!("{k}  Frame {}-{k} · 2400 × 2700 mm", id.0)), "{k}");
        }
        for (k, d) in drawings.iter().enumerate() {
            // two plates, two end studs and the studs at 600 between them: 7 members, 4 lines each
            assert_eq!(elevation(d).count(), 7 * 4, "unit {k}");
            // over it the unit cut level: its studs and what stands either side of them, as
            // thick as the system is built, and that thickness dimensioned
            let cut: Vec<&Line2d> = d.lines.iter().filter(|l| l.a[1] > 2.7 + 1e-6).collect();
            assert!(cut.iter().filter(|l| l.pen == Pen::Cut).count() >= 5 * 4 && cut.iter().any(|l| l.pen == Pen::Beyond), "unit {k}");
            let across = cut.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |acc, l| (acc.0.min(l.a[1]).min(l.b[1]), acc.1.max(l.a[1]).max(l.b[1])));
            assert!(across.1 - across.0 > 0.14 && d.dimensions.iter().any(|x| (x.value - (across.1 - across.0)).abs() < 1e-6), "unit {k}: {across:?}");
            // every member has its number in the list beside it: 1 the studs, 2 the plates
            let numbers = |n: &str| d.texts.iter().filter(|t| t.text == n).count();
            assert_eq!((numbers("1"), numbers("2")), (1 + 5, 1 + 2), "unit {k}");
            assert!(d.name == format!("Frame {}-{}", id.0, k + 1) && d.texts.iter().any(|t| t.text.starts_with(&d.name)), "unit {k}");
            // drawn from its own corner, whatever its place and direction in the model
            let (lo, hi) = d.extent().unwrap();
            assert!(lo[0] < 0.0 && hi[0] > 2.4 && hi[0] < 3.0 && hi[1] > 2.7, "unit {k}: {lo:?} {hi:?}");
            // its length, its height, and its studs centre to centre
            let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
            assert!(values.contains(&2400) && values.contains(&2700) && values.iter().filter(|v| **v == 600).count() >= 2, "unit {k}: {values:?}");
            // the list: the plates, the studs
            assert!(d.texts.iter().any(|t| t.text == "2 × 38×140 L=2400"), "unit {k}: {:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
            assert!(d.texts.iter().any(|t| t.text == "5 × 38×140 L=2624"), "unit {k}");
            assert!(d.texts[0].text.contains(&format!("{}/3", k + 1)) && d.texts[0].text.contains("2400 × 2700"));
        }
    }

    #[test]
    fn an_opening_is_drawn_and_dimensioned_in_the_unit_it_is_in() {
        let window = Opening { along: 0.9, sill: 0.9, width: 1.2, height: 1.2, framing: None };
        let (m, id) = frame(3.3, vec![window]);
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        let drawings: Vec<&Drawing2d> = all.iter().filter(|d| is_frame(d)).collect();
        assert_eq!(drawings.len(), 1);
        let d = drawings[0];
        // its outline and its cross
        assert_eq!(d.lines.iter().filter(|l| l.pen == Pen::Detail).count(), 6);
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        for want in [900, 1200, 3300, 2700, 600] {
            assert!(values.contains(&want), "{want} in {values:?}");
        }
        // the header is in the list, as wide as the opening and its trimmers
        assert!(d.texts.iter().any(|t| t.text.ends_with(&format!("L={}", 1200 + 2 * 38))), "{:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
    }

    #[test]
    fn what_is_not_a_frame_has_no_element_drawing() {
        let curtain = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), grid: None, division: None };
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Curtain wall", None, Shape::System(curtain))).unwrap();
        assert!(element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string()).is_empty());
    }

    #[test]
    fn every_layer_with_boards_or_battens_has_a_drawing_of_its_own() {
        // a clad frame of one unit with a window: its frame, then from the interior out the
        // lining, the service battens, the sheathing, the cavity battens and the weatherboards
        let window = Opening { along: 0.9, sill: 0.9, width: 1.2, height: 1.2, framing: None };
        let (m, id) = typed(3.3, vec![window], "system.timber-frame-140-clad");
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_uppercase());
        let kinds: Vec<&str> = all.iter().map(|d| d.name.rsplit(' ').next().unwrap()).collect();
        assert_eq!(kinds, ["1-1", "gypsum-board", "timber", "plywood", "timber", "timber-cladding"], "{:?}", all.iter().map(|d| &d.name).collect::<Vec<_>>());
        assert!(is_frame(&all[0]) && all[1..].iter().all(|d| !is_frame(d)));
        // the lining: boards 1200 wide cut around the window, their seams dimensioned, a list
        let lining = &all[1];
        assert!(lining.texts[0].text.contains("GYPSUM-BOARD 13") || lining.texts[0].text.contains("GYPSUM-BOARD 12"), "{}", lining.texts[0].text);
        assert!(lining.texts.iter().skip(1).filter(|t| t.text.contains('×')).all(|t| t.text.contains(" × 13 × ") || t.text.contains(" × 12 × ")), "{:?}", lining.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
        let values: Vec<i64> = lining.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.contains(&1200) && values.contains(&2700), "{values:?}");
        assert_eq!(lining.lines.iter().filter(|l| l.pen == Pen::Detail).count(), 6, "the window");
        // the cavity battens: the battens only, as a list of lengths
        let cavity = &all[4];
        assert!(cavity.texts.iter().any(|t| t.text.ends_with("× 45×22 L=2700")), "{:?}", cavity.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
        // the weatherboards: too many courses to dimension one by one, all in the list
        let boards = &all[5];
        assert!(boards.dimensions.len() < 12 && boards.texts.len() > 2, "{} {}", boards.dimensions.len(), boards.texts.len());
    }

    #[test]
    fn a_steel_frame_is_drawn_with_its_studs_and_tracks() {
        let (m, id) = typed(3.0, Vec::new(), "system.steel-frame-150");
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        let d = all.iter().find(|d| is_frame(d)).unwrap();
        // two tracks and six studs, seated in the tracks
        assert_eq!(elevation(d).count(), 8 * 4);
        assert!(d.texts.iter().any(|t| t.text == "2 × 50×150 L=3000") && d.texts.iter().any(|t| t.text == "6 × 50×150 L=2697"), "{:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
    }
    #[test]
    fn a_window_standing_in_a_frame_is_drawn_and_no_board_leaves_its_unit() {
        // 9 m along +y from (2, 1), its base at 0.5, a window object standing in it: the units
        // are those the model makes with it
        let (mut m, id) = typed(9.0, Vec::new(), "system.timber-frame-140-clad");
        let grid = m.grid;
        let window = bim_core::PointDef { at: glam::DVec3::new(2.0, 1.0 + 3.6, 0.5 + 0.9), rotation: std::f64::consts::FRAC_PI_2, size: glam::DVec3::new(1.2, 0.04, 1.2), opening: Some(bim_core::OpeningKind::Window), mirrored: false, role: Default::default() };
        m.add(Element::in_project(&grid, "Window", None, Shape::Point(window))).unwrap();
        let e = m.get(id).unwrap();
        assert_eq!(m.openings_of(id).len(), 1);
        let units = bim_core::system::joined_units(&m, e).unwrap();
        let all = element_drawings(&m, e, &|key| key.to_string());
        assert_eq!(all.iter().filter(|d| is_frame(d)).count(), units.len());
        assert_eq!(all.iter().filter(|d| is_frame(d)).map(|d| d.lines.iter().filter(|l| l.pen == Pen::Detail).count()).sum::<usize>(), 6, "the window, once");
        let mut unit = 0;
        for d in all.iter().filter(|d| !d.name.ends_with(" overview")) {
            if is_frame(d) {
                unit = d.name.rsplit('-').next().unwrap().parse::<usize>().unwrap() - 1;
            }
            let (lo, hi) = d.lines.iter().filter(|l| l.pen == Pen::Cut).fold((f64::INFINITY, f64::NEG_INFINITY), |acc, l| (acc.0.min(l.a[0]).min(l.b[0]), acc.1.max(l.a[0]).max(l.b[0])));
            assert!(lo > -0.3 && hi < units[unit] + 0.3, "{}: {lo} to {hi} in a unit of {}", d.name, units[unit]);
        }
    }
    #[test]
    fn a_frame_with_a_corner_off_is_drawn_as_it_is_cut() {
        // 3 m of frame, 1.2 by 0.9 off its top at its end
        let (mut m, id) = frame(3.0, Vec::new());
        let mut cut = m.get(id).unwrap().clone();
        let Shape::System(s) = &mut cut.shape else { panic!("a system") };
        let p = DVec2::new;
        s.outline = vec![p(0.0, 0.0), p(3.0, 0.0), p(3.0, 1.8), p(1.8, 2.7), p(0.0, 2.7)];
        m.replace(cut).unwrap();
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        let d = all.iter().find(|d| is_frame(d)).unwrap();
        // the plate along the cut slopes, and so do the heads of the studs under it
        let sloping = d.lines.iter().filter(|l| l.pen == Pen::Cut && l.a[1] <= 2.7 + 1e-6 && (l.a[0] - l.b[0]).abs() > 1e-6 && (l.a[1] - l.b[1]).abs() > 1e-6).count();
        assert!(sloping >= 4, "{sloping}");
        // in the list as it is cut: 38 wide and 1500 long, not the 1200 by 900 it reaches over
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts.iter().any(|t| t.starts_with("1 × 38×140 L=15")), "{texts:?}");
        // the studs under the cut are measured to, and their heads are cut 37° off square
        // (the cut rises 900 over 1200)
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.iter().filter(|v| **v == 600).count() >= 4, "{values:?}");
        assert!(texts.iter().filter(|t| t.ends_with(" · 37°")).count() >= 2, "{texts:?}");
    }

    #[test]
    fn a_system_on_a_grid_gets_an_elevation_with_its_panels_and_members() {
        // a curtain wall of 6 m, bays of 1500: four panes over one another in rows of 1800
        let (m, id) = typed(6.0, Vec::new(), "system.curtain-wall-aluminium-1500");
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        assert_eq!(all.len(), 1);
        let d = &all[0];
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts[0].starts_with("Frame · glass ") && texts[0].ends_with("1:50"), "{texts:?}");
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.iter().filter(|v| **v == 1500).count() >= 2 && values.iter().any(|v| *v >= 6000), "{values:?}");
        // panes thin, mullions and transoms bold; every pane numbered and listed, the members listed
        let panes = d.lines.iter().filter(|l| l.pen == Pen::Beyond).count() / 4;
        assert_eq!(panes, 8);
        // five mullions, and three transoms in each of four bays
        assert!(d.lines.iter().filter(|l| l.pen == Pen::Cut).count() >= (5 + 12) * 4);
        let listed: usize = texts.iter().filter(|t| t.contains(" × ") && !t.contains("L=") && !t.contains("1:50")).map(|t| t.split(' ').next().unwrap().parse::<usize>().unwrap()).sum();
        assert_eq!(listed, panes, "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("aluminium 50×150 L=")), "{texts:?}");
        // brickwork is not drawn brick by brick: its outline, its courses and its bricks counted;
        // a window in it is crossed and the lintel over it drawn and listed
        let window = Opening { along: 4.0, sill: 0.9, width: 1.2, height: 1.2, framing: None };
        let (m, id) = typed(16.0, vec![window], "system.masonry-waal-stretcher");
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        assert_eq!(all.len(), 1);
        let d = &all[0];
        assert_eq!((d.lines.iter().filter(|l| l.pen == Pen::Cut).count(), d.lines.iter().filter(|l| l.pen == Pen::Detail).count()), (4 + 4, 6));
        assert!(d.texts.iter().any(|t| t.text.starts_with("1 × concrete 100×12") && t.text.ends_with(" L=1400")), "{:?}", d.texts.iter().map(|t| &t.text).collect::<Vec<_>>());
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts[0].contains(" · 43 × 62.5 · 1:50"), "{texts:?}");
        let bricks: usize = texts.iter().filter(|t| t.contains(" × 100 × ")).map(|t| t.split(' ').next().unwrap().parse::<usize>().unwrap()).sum();
        assert!(bricks > 2500, "{bricks}");
        // alike bricks on one line: whole ones and halves at the ends of every other course
        assert!(texts.iter().filter(|t| t.contains(" × 100 × ")).count() >= 2, "{texts:?}");
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.iter().any(|v| *v >= 16000) && values.iter().any(|v| (2690..=2710).contains(v)), "{values:?}");
    }

    #[test]
    fn a_ceiling_on_channels_gets_a_laying_plan_of_its_boards_and_channels() {
        let ceiling = |id: &str| {
            let generic = SystemDef { grid: None, start: DVec2::new(1.0, 1.0), end: DVec2::new(5.8, 1.0), base: 2.6, height: 3.6, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
            let s = generic.laid(Library::starter().system_type(id).unwrap()).unwrap();
            let mut m = Model::new(TileGrid::default());
            let grid = m.grid;
            let id = m.add(Element::in_project(&grid, "Ceiling", None, Shape::System(s))).unwrap();
            element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string())
        };
        // two layers of board on channels, 4.8 by 3.6 m
        let all = ceiling("system.ceiling-gypsum-2");
        assert_eq!(all.len(), 1);
        let d = &all[0];
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        // the boards seen from below, once, the channels behind them
        let boards = d.lines.iter().filter(|l| l.pen == Pen::Cut).count() / 4;
        let channels = d.lines.iter().filter(|l| l.pen == Pen::Detail).count() / 4;
        assert!(boards >= 6 && channels >= 10, "{boards} {channels}");
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.contains(&4800) && values.contains(&3600) && values.contains(&1200) && values.contains(&2400), "{values:?}");
        // the list: both layers of board, and the channels
        assert!(texts[0].starts_with("Ceiling · gypsum-board 2×12.5 · 4800 × 3600 mm"), "{texts:?}");
        let listed: usize = texts.iter().filter(|t| t.contains(" × 12.5 × ")).map(|t| t.split(' ').next().unwrap().parse::<usize>().unwrap()).sum();
        assert_eq!(listed, 2 * boards, "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("steel 60×27 L=")), "{texts:?}");
        // slats with nothing under them: the slats themselves
        let all = ceiling("system.ceiling-timber-slats-90");
        assert_eq!(all.len(), 1);
        assert!(all[0].lines.iter().filter(|l| l.pen == Pen::Cut).count() > 30 * 4);
        assert!(all[0].texts.iter().any(|t| t.text.contains("timber 30×45 L=")), "{:?}", all[0].texts.iter().map(|t| &t.text).collect::<Vec<_>>());
    }

    #[test]
    fn a_floor_system_gets_a_laying_plan() {
        use bim_core::OpeningFraming;
        let generic = SystemDef { start: DVec2::new(1.0, 1.0), end: DVec2::new(5.8, 1.0), base: 3.0, height: 6.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), grid: None, division: None };
        let mut s = generic.laid(Library::starter().system_type("system.hollow-core-200").unwrap()).unwrap();
        // an opening that takes the second slab out over a metre, and a core through the fourth
        s.openings = vec![Opening { along: 1.5, sill: 2.0, width: 0.8, height: 1.0, framing: None }, Opening { along: 4.0, sill: 1.0, width: 0.2, height: 0.2, framing: Some(OpeningFraming { round: true, ..Default::default() }) }];
        let mut m = Model::new(TileGrid::default());
        let grid = m.grid;
        let id = m.add(Element::in_project(&grid, "Floor", None, Shape::System(s))).unwrap();
        let all = element_drawings(&m, m.get(id).unwrap(), &|key| key.to_string());
        assert_eq!(all.len(), 1);
        let d = &all[0];
        // five slabs (one in two), each at least its four sides; the core's circle; two trimmers
        assert!(d.lines.iter().filter(|l| l.pen == Pen::Cut).count() > 5 * 4 + 8);
        assert_eq!(d.lines.iter().filter(|l| l.pen == Pen::Detail).count(), 2 * 4);
        let values: Vec<i64> = d.dimensions.iter().map(|x| mm(x.value)).collect();
        assert!(values.contains(&4800) && values.contains(&6000) && values.iter().filter(|v| **v == 1200).count() >= 2, "{values:?}");
        let texts: Vec<&str> = d.texts.iter().map(|t| t.text.as_str()).collect();
        assert!(texts[0].contains("hollow-core-concrete 200") && texts[0].contains("1:50"), "{texts:?}");
        assert!(texts.contains(&"2 × 200 × 1190 × 6000") && texts.contains(&"1 × 200 × 1180 × 6000") && texts.contains(&"1 × 200 × 1180 × 3000") && texts.contains(&"2 × steel L=1180"), "{texts:?}");
    }
}
