//! What a change of the model touches: the elements whose solids or connections may differ
//! between two states of a model, so that what is shown of the others can be kept.
//!
//! An element's solids follow from its own definition and from a few others:
//!
//! - **A wall, a building system, a door or a window** also from the walls, systems, doors and
//!   windows near it: a wall is cut to the corner it makes with the walls at its ends, a door or a
//!   window cuts every wall it stands in, a system ends at the system it meets.
//! - **Any element** from the connections it is a member of, and from the other member of each: a
//!   butt gives a beam its end plane, a cut takes the cutter's body out of its target.
//!
//! Nothing else: a floor, a roof, a stair, a column or an object away from any connection is made
//! from its own definition alone.

use crate::element::{Element, ElementId, PlanarForm, Shape};
use crate::model::Model;
use crate::ConnectionKind;
use glam::DVec3;
use std::collections::BTreeSet;

/// How far apart two of them may lie and still count as near (metres): more than a wall's
/// thickness and the reach of a corner's cut.
const NEAR: f64 = 0.75;

/// The box an element that meets its neighbours by itself takes up, in the project's own
/// coordinates; `None` for one that does not (see the module).
fn reach(model: &Model, e: &Element) -> Option<(DVec3, DVec3)> {
    let origin = model.grid.origin(e.tile);
    let (lo, hi) = match &e.shape {
        Shape::Planar(p) => {
            let PlanarForm::Wall { start, end, base, height } = &p.form else { return None };
            let half = DVec3::new(p.total_thickness() / 2.0, p.total_thickness() / 2.0, 0.0);
            (start.min(*end).extend(*base) - half, start.max(*end).extend(base + height) + half)
        }
        Shape::System(s) => {
            let points = || [s.start, s.end].into_iter().chain(s.outline.iter().copied());
            let lo = points().fold(s.start, |a, b| a.min(b));
            let hi = points().fold(s.start, |a, b| a.max(b));
            let thick = s.member.max_element().max(s.panel);
            // a lying one's height is how far it lies under or over its base
            (lo.extend(s.base - s.height.abs() - thick) - DVec3::new(thick, thick, 0.0), hi.extend(s.base + s.height.abs() + thick) + DVec3::new(thick, thick, 0.0))
        }
        Shape::Point(p) if p.opening.is_some() => {
            let half = p.size.x.max(p.size.y) / 2.0;
            (p.at - DVec3::new(half, half, 0.0), p.at + DVec3::new(half, half, p.size.z))
        }
        _ => return None,
    };
    Some((lo + origin, hi + origin))
}

fn near(a: &(DVec3, DVec3), b: &(DVec3, DVec3)) -> bool {
    (a.0 - NEAR).cmple(b.1).all() && (b.0 - NEAR).cmple(a.1).all()
}

impl Model {
    /// The elements of this model whose solids, or whose connections, may differ from those they
    /// had in `before`: what was added or changed, what is near it or joined to it, and what a
    /// cutter among those cuts. `None` when everything may (another tile grid).
    pub fn touched_since(&self, before: &Model) -> Option<BTreeSet<ElementId>> {
        if self.grid != before.grid {
            return None;
        }
        // what was added, changed or removed, as it is now and as it was
        let changed: Vec<(Option<&Element>, Option<&Element>)> = self
            .iter()
            .filter_map(|e| match before.get(e.id) {
                Some(old) if old == e => None,
                old => Some((Some(e), old)),
            })
            .chain(before.iter().filter(|old| self.get(old.id).is_none()).map(|old| (None, Some(old))))
            .collect();
        let mut touched: BTreeSet<ElementId> = BTreeSet::new();
        if changed.is_empty() {
            return Some(touched);
        }
        let mut boxes: Vec<(DVec3, DVec3)> = Vec::new();
        for (now, was) in &changed {
            for (e, model) in [(now, self), (was, before)] {
                let Some(e) = e else { continue };
                touched.insert(e.id);
                // a connection changed: its members are made again
                if let Shape::Connection(c) = &e.shape {
                    touched.extend([c.primary, c.secondary]);
                }
                boxes.extend(reach(model, e));
            }
        }
        // what stands near something changed
        if !boxes.is_empty() {
            for e in self.iter().filter(|e| !touched.contains(&e.id)).collect::<Vec<_>>() {
                if reach(self, e).is_some_and(|b| boxes.iter().any(|c| near(&b, c))) {
                    touched.insert(e.id);
                }
            }
        }
        // a connection with a member changed: the connection and its other member
        let changed_ids: BTreeSet<ElementId> = changed.iter().filter_map(|(now, was)| now.or(*was).map(|e| e.id)).collect();
        for e in self.iter() {
            if let Shape::Connection(c) = &e.shape {
                if changed_ids.contains(&c.primary) || changed_ids.contains(&c.secondary) {
                    touched.extend([e.id, c.primary, c.secondary]);
                }
            }
        }
        // a cutter made again cuts its target again (its body follows its neighbours too)
        for e in self.iter() {
            if let Shape::Connection(c) = &e.shape {
                if c.kind == ConnectionKind::Cut && touched.contains(&c.primary) {
                    touched.extend([e.id, c.secondary]);
                }
            }
        }
        touched.retain(|id| self.get(*id).is_some());
        Some(touched)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::element::{PlanarDef, PointDef};
    use crate::{ConnectionDef, LinearDef, OpeningKind, ProfileDef};
    use glam::DVec2;

    fn wall(a: (f64, f64), b: (f64, f64)) -> Shape {
        Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None })
    }

    fn add(m: &mut Model, shape: Shape) -> ElementId {
        let grid = m.grid;
        m.add(Element::in_project(&grid, "e", None, shape)).unwrap()
    }

    #[test]
    fn a_change_touches_what_is_near_it_and_what_it_is_joined_to_and_nothing_else() {
        let mut m = Model::default();
        let a = add(&mut m, wall((0.0, 0.0), (5.0, 0.0)));
        let far = add(&mut m, wall((20.0, 0.0), (25.0, 0.0)));
        let above = add(&mut m, wall((0.0, 0.0), (5.0, 0.0)).tap(|s| if let Shape::Planar(p) = s { p.form = PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 6.0, height: 3.0 } }));
        let beam = |to: (f64, f64)| Shape::Linear(LinearDef { start: DVec3::new(4.0, 10.0, 3.0), end: DVec3::new(to.0, to.1, 3.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.3 }, roll: 0.0, role: Default::default() });
        let (b1, b2) = (add(&mut m, beam((0.0, 10.0))), add(&mut m, beam((4.0, 14.0))));
        let before = m.clone();
        assert_eq!(m.touched_since(&before), Some(BTreeSet::new()), "nothing changed");
        // a wall at the corner of the first: both, and not the far one, the one above or a beam
        let b = add(&mut m, wall((5.0, 0.0), (5.0, 4.0)));
        assert_eq!(m.touched_since(&before), Some([a, b].into()));
        // a door in the first wall: the wall it cuts, and the wall at its corner within reach
        let now = m.clone();
        let door = add(&mut m, Shape::Point(PointDef { at: DVec3::new(2.0, 0.0, 0.0), rotation: 0.0, size: DVec3::new(0.9, 0.04, 2.1), opening: Some(OpeningKind::Door), mirrored: false, role: Default::default() }));
        let touched = m.touched_since(&now).unwrap();
        assert!(touched.contains(&a) && touched.contains(&door) && !touched.contains(&far) && !touched.contains(&above), "{touched:?}");
        // a butt of the beams: both beams and the connection
        let now = m.clone();
        let butt = add(&mut m, Shape::Connection(ConnectionDef { kind: ConnectionKind::Butt, primary: b1, secondary: b2 }));
        assert_eq!(m.touched_since(&now), Some([b1, b2, butt].into()));
        // one beam moved: the other follows through their connection
        let now = m.clone();
        let mut moved = m.get(b2).unwrap().clone();
        moved.shape = beam((4.0, 15.0));
        m.replace(moved).unwrap();
        assert_eq!(m.touched_since(&now), Some([b1, b2, butt].into()));
        // the corner wall removed: the first wall closes its end again
        let now = m.clone();
        m.remove(b);
        assert_eq!(m.touched_since(&now), Some([a].into()), "the door stands away from the corner");
        // another tile grid: everything
        let mut other = m.clone();
        other.grid = crate::TileGrid::new(500).unwrap();
        assert_eq!(other.touched_since(&m), None);
    }

    trait Tap: Sized {
        fn tap(self, f: impl FnOnce(&mut Self)) -> Self;
    }
    impl Tap for Shape {
        fn tap(mut self, f: impl FnOnce(&mut Self)) -> Self {
            f(&mut self);
            self
        }
    }
}
