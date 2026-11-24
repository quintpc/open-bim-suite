//! The five base elements from which BIM objects are built, as in Open BIM Studio's
//! `elements/` and `pattern/` modules:
//!
//! - **point** ([`PointDef`]): a type placed at one point;
//! - **linear** ([`LinearDef`]): a profile along a line (beam, column, member);
//! - **planar** ([`PlanarDef`]): a wall by its axis or a slab by its outline, with openings;
//! - **system** ([`SystemDef`]): a recipe on a domain that generates members and panels;
//! - **connection** ([`ConnectionDef`]): where two linear elements meet.
//!
//! An element holds its definition only. Its coordinates are f64 metres in the frame of its tile
//! ([`Element::tile`]), the tile that holds its placement origin. Solids are evaluated from the
//! definition ([`crate::Model::evaluate`]).

use crate::profile::{Profile, ProfileDef};
use crate::solid::{Axes, Plane, Solid};
use crate::tile::{TileGrid, TileKey};
use crate::{positive, Refusal, LINEAR_TOLERANCE};
use glam::{DVec2, DVec3};
use serde::{Deserialize, Serialize};

/// A stable element id: never reused within a model, the key of every reference (ADR-0011).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElementId(pub u64);

/// The five base elements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BaseKind {
    Point,
    Linear,
    Planar,
    System,
    Connection,
}

impl BaseKind {
    pub const ALL: [BaseKind; 5] = [BaseKind::Point, BaseKind::Linear, BaseKind::Planar, BaseKind::System, BaseKind::Connection];

    /// Its English name.
    pub fn name(self) -> &'static str {
        match self {
            BaseKind::Point => "Point",
            BaseKind::Linear => "Linear",
            BaseKind::Planar => "Planar",
            BaseKind::System => "System",
            BaseKind::Connection => "Connection",
        }
    }
}

/// A point element: a box of `size` (width along its x, depth, height) standing on `at`, turned
/// `rotation` radians about the vertical. With `opening` it is a door or a window: `at` is the
/// middle of its width at its foot, and it cuts an opening of its width and height in every wall
/// or framed system it stands in (see `Model::standing_openings`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointDef {
    pub at: DVec3,
    #[serde(default)]
    pub rotation: f64,
    pub size: DVec3,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub opening: Option<OpeningKind>,
    /// What it is: an object, or a terminal or an appliance of the installations.
    #[serde(default, skip_serializing_if = "PointRole::is_object")]
    pub role: PointRole,
}

/// What a point element is. The element is the same in every role (a box standing on a point);
/// the role says which IFC class it has: an object of the building, or a terminal or an
/// appliance of its installations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PointRole {
    /// Anything else: a proxy.
    #[default]
    Object,
    Radiator,
    Boiler,
    Pump,
    Fan,
    AirHandlingUnit,
    AirTerminal,
    Sanitary,
    Valve,
    Tank,
    Outlet,
    Light,
}

impl PointRole {
    /// The roles of the installations: terminals and appliances.
    pub const APPLIANCES: [PointRole; 11] = [
        PointRole::Radiator,
        PointRole::Boiler,
        PointRole::Pump,
        PointRole::Fan,
        PointRole::AirHandlingUnit,
        PointRole::AirTerminal,
        PointRole::Sanitary,
        PointRole::Valve,
        PointRole::Tank,
        PointRole::Outlet,
        PointRole::Light,
    ];

    pub fn is_object(&self) -> bool {
        *self == PointRole::Object
    }

    /// Its IFC class (IFC4.3 names).
    pub fn class(self) -> &'static str {
        match self {
            PointRole::Object => "IfcBuildingElementProxy",
            PointRole::Radiator => "IfcSpaceHeater",
            PointRole::Boiler => "IfcBoiler",
            PointRole::Pump => "IfcPump",
            PointRole::Fan => "IfcFan",
            PointRole::AirHandlingUnit => "IfcUnitaryEquipment",
            PointRole::AirTerminal => "IfcAirTerminal",
            PointRole::Sanitary => "IfcSanitaryTerminal",
            PointRole::Valve => "IfcValve",
            PointRole::Tank => "IfcTank",
            PointRole::Outlet => "IfcOutlet",
            PointRole::Light => "IfcLightFixture",
        }
    }

    /// The size it usually has (width, depth, height) and how far above its level it stands.
    pub fn usual(self) -> (DVec3, f64) {
        match self {
            PointRole::Object => (DVec3::new(0.6, 0.6, 0.9), 0.0),
            PointRole::Radiator => (DVec3::new(1.0, 0.1, 0.6), 0.15),
            PointRole::Boiler => (DVec3::new(0.45, 0.35, 0.7), 1.2),
            PointRole::Pump => (DVec3::new(0.18, 0.15, 0.18), 1.0),
            PointRole::Fan => (DVec3::new(0.6, 0.6, 0.3), 0.0),
            PointRole::AirHandlingUnit => (DVec3::new(2.0, 1.0, 1.2), 0.0),
            PointRole::AirTerminal => (DVec3::new(0.3, 0.3, 0.1), 2.5),
            PointRole::Sanitary => (DVec3::new(0.4, 0.7, 0.4), 0.0),
            PointRole::Valve => (DVec3::new(0.1, 0.1, 0.15), 1.0),
            PointRole::Tank => (DVec3::new(0.6, 0.6, 1.5), 0.0),
            PointRole::Outlet => (DVec3::new(0.08, 0.04, 0.08), 0.3),
            PointRole::Light => (DVec3::new(0.3, 0.3, 0.1), 2.5),
        }
    }
}

/// What a point element that cuts an opening is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpeningKind {
    Door,
    Window,
}

/// A linear element: `profile` from `start` to `end`, turned `roll` radians about its axis. A
/// horizontal member has its profile's y upwards; a vertical one its profile's x along the
/// project's x.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearDef {
    pub start: DVec3,
    pub end: DVec3,
    pub profile: ProfileDef,
    #[serde(default)]
    pub roll: f64,
    /// What it is: a member of the structure, or a run of a building service.
    #[serde(default, skip_serializing_if = "LinearRole::is_member")]
    pub role: LinearRole,
}

/// What a linear element is. The element is the same in every role (a profile along a line); the
/// role says which IFC class it has and which fittings join it to another of its kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinearRole {
    /// A column, a beam or another member: its class follows from the way it runs.
    #[default]
    Member,
    /// A run of air duct.
    Duct,
    /// A run of pipe.
    Pipe,
    /// A run of cable tray.
    CableTray,
}

impl LinearRole {
    pub fn is_member(&self) -> bool {
        *self == LinearRole::Member
    }

    /// Whether it is a run of a building service.
    pub fn is_service(self) -> bool {
        self != LinearRole::Member
    }

    /// The IFC class of a run in this role; none for a member, whose class follows from its axis.
    pub fn segment_class(self) -> Option<&'static str> {
        match self {
            LinearRole::Member => None,
            LinearRole::Duct => Some("IfcDuctSegment"),
            LinearRole::Pipe => Some("IfcPipeSegment"),
            LinearRole::CableTray => Some("IfcCableCarrierSegment"),
        }
    }

    /// The IFC class of a fitting between two runs in this role.
    pub fn fitting_class(self) -> &'static str {
        match self {
            LinearRole::Member => "IfcElementAssembly",
            LinearRole::Duct => "IfcDuctFitting",
            LinearRole::Pipe => "IfcPipeFitting",
            LinearRole::CableTray => "IfcCableCarrierFitting",
        }
    }
}

/// The service an element of a building's installations belongs to: what flows through a run, a
/// fitting, a terminal or an appliance. Elements joined to each other in one service are one
/// system (IfcDistributionSystem); two runs of different services are never joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    SupplyAir,
    ReturnAir,
    ExhaustAir,
    HeatingSupply,
    HeatingReturn,
    ColdWater,
    HotWater,
    Waste,
    RainWater,
    Gas,
    Power,
    Data,
}

impl Service {
    pub const ALL: [Service; 12] = [
        Service::SupplyAir,
        Service::ReturnAir,
        Service::ExhaustAir,
        Service::HeatingSupply,
        Service::HeatingReturn,
        Service::ColdWater,
        Service::HotWater,
        Service::Waste,
        Service::RainWater,
        Service::Gas,
        Service::Power,
        Service::Data,
    ];

    /// The role of the runs that carry it.
    pub fn role(self) -> LinearRole {
        match self {
            Service::SupplyAir | Service::ReturnAir | Service::ExhaustAir => LinearRole::Duct,
            Service::Power | Service::Data => LinearRole::CableTray,
            _ => LinearRole::Pipe,
        }
    }

    /// Its name (English).
    pub fn name(self) -> &'static str {
        match self {
            Service::SupplyAir => "Supply air",
            Service::ReturnAir => "Return air",
            Service::ExhaustAir => "Exhaust air",
            Service::HeatingSupply => "Heating supply",
            Service::HeatingReturn => "Heating return",
            Service::ColdWater => "Cold water",
            Service::HotWater => "Hot water",
            Service::Waste => "Waste water",
            Service::RainWater => "Rain water",
            Service::Gas => "Gas",
            Service::Power => "Power",
            Service::Data => "Data",
        }
    }

    /// The predefined type of its IfcDistributionSystem (IFC4).
    pub fn ifc_system(self) -> &'static str {
        match self {
            Service::SupplyAir | Service::ReturnAir => "VENTILATION",
            Service::ExhaustAir => "EXHAUST",
            Service::HeatingSupply | Service::HeatingReturn => "HEATING",
            Service::ColdWater => "DOMESTICCOLDWATER",
            Service::HotWater => "DOMESTICHOTWATER",
            Service::Waste => "WASTEWATER",
            Service::RainWater => "RAINWATER",
            Service::Gas => "GAS",
            Service::Power => "ELECTRICAL",
            Service::Data => "DATA",
        }
    }
}

/// A rectangular opening in a wall: `along` the axis from its start to the opening's near side,
/// `sill` above the wall's base, `width` × `height`. An opening with no sill (a door) cuts the
/// wall's bottom edge; any other keeps wall below, above and on both sides.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Opening {
    pub along: f64,
    pub sill: f64,
    pub width: f64,
    pub height: f64,
    /// In a framed system: how this opening is framed where it is not as its system's type frames
    /// openings. A wall's opening has none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub framing: Option<OpeningFraming>,
}

/// The framing of one opening where it departs from its system's type: a header of its own
/// height, and more king studs or trimmers either side than one. What it leaves out is the type's.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct OpeningFraming {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kings: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trimmers: Option<u8>,
    /// In a lying system (a floor): the opening is round, drilled, its `width` the diameter.
    #[serde(default, skip_serializing_if = "is_false")]
    pub round: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Opening {
    /// How many king studs and how many trimmers stand either side of it in a frame: one of each,
    /// unless its own framing says two.
    pub fn studs_beside(&self) -> (u8, u8) {
        let own = self.framing.unwrap_or_default();
        (own.kings.unwrap_or(1).clamp(1, 2), own.trimmers.unwrap_or(1).clamp(1, 2))
    }
}

/// How a slab rises: from edge `edge` of its outline (the eave, where its top is at the slab's
/// `top`) inwards, at `angle` radians to the level. Its thickness is measured square to it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Slope {
    pub edge: usize,
    pub angle: f64,
}

/// The planes of a roof over `outline`, as a roof by footprint has them: every edge with an angle
/// above zero in `angles` (radians, an entry per edge) is an eave a plane rises from at that
/// angle, and each plane covers the part of the outline where it is the lowest. An edge without
/// an angle is a gable: the planes beside it run on to it. Gives each plane's outline in plan
/// (anticlockwise) with its slope, the eave being the edge the slope names. With no angle at all
/// the roof is flat: the outline itself, without a slope.
///
/// An outline that turns inwards (an L, a T, a U) is roofed wing by wing: at every inner corner
/// it is cut along each of the two edges that meet there, run on to the far side, and each wing
/// so cut off gets the roof of its own outline, the cut taking the slope of the edge it runs on
/// from. The wings overlap, and so do their roofs: where two meet, the higher one is the roof
/// and the lower one ends under it, so the valleys lie where the planes cross.
pub fn roof_planes(outline: &[DVec2], angles: &[f64]) -> Result<Vec<(Vec<DVec2>, Option<Slope>)>, Refusal> {
    let n = outline.len();
    if n < 3 || outline.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::NotPositive);
    }
    let area: f64 = (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum();
    let ring: Vec<DVec2> = if area >= 0.0 { outline.to_vec() } else { outline.iter().rev().copied().collect() };
    let angle_of = |i: usize| angles.get(if area >= 0.0 { i } else { (2 * n - 2 - i) % n }).copied().unwrap_or(0.0);
    let angles: Vec<f64> = (0..n).map(angle_of).collect();
    if angles.iter().all(|a| *a <= 1e-9) {
        return roof_of_wing(&ring, &angles);
    }
    let mut planes = Vec::new();
    for (wing, slopes) in roof_wings(ring, angles, 0)? {
        for plane in roof_of_wing(&wing, &slopes)? {
            // a plane two wings share is the roof's once
            let same = |other: &(Vec<DVec2>, Option<Slope>)| other.1 == plane.1 && other.0.len() == plane.0.len() && other.0.iter().zip(&plane.0).all(|(a, b)| a.distance(*b) <= LINEAR_TOLERANCE);
            if !planes.iter().any(same) {
                planes.push(plane);
            }
        }
    }
    Ok(planes)
}

/// The most wings an outline is roofed in.
const MOST_WINGS: usize = 64;

/// The wings of an outline (anticlockwise, an angle per edge): itself when it does not turn
/// inwards; else, at its first inner corner, the two parts cut off along the edges that meet
/// there, each cut again in the same way.
fn roof_wings(ring: Vec<DVec2>, angles: Vec<f64>, depth: usize) -> Result<Vec<(Vec<DVec2>, Vec<f64>)>, Refusal> {
    let tol = LINEAR_TOLERANCE;
    let n = ring.len();
    let turn = |i: usize| (ring[i] - ring[(i + n - 1) % n]).perp_dot(ring[(i + 1) % n] - ring[i]);
    let Some(r) = (0..n).find(|i| turn(*i) < -tol) else { return Ok(vec![(ring, angles)]) };
    if depth > 16 {
        return Err(Refusal::NotConvex);
    }
    // where the ray from corner r along `way` first meets the outline: the point, and the edge
    let hit = |way: DVec2| -> Option<(DVec2, usize)> {
        let mut best: Option<(f64, DVec2, usize)> = None;
        for m in (0..n).filter(|m| *m != r && (*m + 1) % n != r) {
            let (a, b) = (ring[m], ring[(m + 1) % n]);
            let det = way.perp_dot(b - a);
            if det.abs() < 1e-12 {
                continue;
            }
            let (t, u) = ((a - ring[r]).perp_dot(b - a) / det, (a - ring[r]).perp_dot(way) / det);
            if t > tol && (-1e-9..=1.0 + 1e-9).contains(&u) && best.is_none_or(|(least, ..)| t < least) {
                best = Some((t, ring[r] + way * t, m));
            }
        }
        best.map(|(_, q, m)| (q, m))
    };
    // corners of a wing that fell together are one, their edge of no length gone
    let tidy = |points: Vec<DVec2>, slopes: Vec<f64>| -> (Vec<DVec2>, Vec<f64>) {
        let mut out: (Vec<DVec2>, Vec<f64>) = (Vec::new(), Vec::new());
        for k in 0..points.len() {
            if points[k].distance(points[(k + 1) % points.len()]) > tol {
                out.0.push(points[k]);
                out.1.push(slopes[k]);
            }
        }
        out
    };
    let (before, after) = ((r + n - 1) % n, (r + 1) % n);
    let mut wings = Vec::new();
    // along the edge that comes in at the corner, run on: the wing that edge is of
    let way = (ring[r] - ring[before]).try_normalize().ok_or(Refusal::ZeroLength)?;
    let (q, m) = hit(way).ok_or(Refusal::NotConvex)?;
    let (mut points, mut slopes) = (vec![q], vec![angles[m]]);
    let mut k = (m + 1) % n;
    while k != r {
        points.push(ring[k]);
        slopes.push(angles[k]);
        k = (k + 1) % n;
    }
    points.push(ring[r]);
    slopes.push(angles[before]);
    wings.push(tidy(points, slopes));
    // along the edge that goes out from the corner, run back: the wing that edge is of
    let way = (ring[r] - ring[after]).try_normalize().ok_or(Refusal::ZeroLength)?;
    let (q, m) = hit(way).ok_or(Refusal::NotConvex)?;
    let (mut points, mut slopes) = (Vec::new(), Vec::new());
    let mut k = r;
    loop {
        points.push(ring[k]);
        slopes.push(angles[k]);
        if k == m {
            break;
        }
        k = (k + 1) % n;
    }
    points.push(q);
    slopes.push(angles[r]);
    wings.push(tidy(points, slopes));
    let mut out = Vec::new();
    for (points, slopes) in wings {
        if points.len() < 3 {
            continue;
        }
        out.extend(roof_wings(points, slopes, depth + 1)?);
        if out.len() > MOST_WINGS {
            return Err(Refusal::NotConvex);
        }
    }
    Ok(out)
}

/// The planes of the roof over a wing: an outline that does not turn inwards (see
/// [`roof_planes`]).
fn roof_of_wing(outline: &[DVec2], angles: &[f64]) -> Result<Vec<(Vec<DVec2>, Option<Slope>)>, Refusal> {
    let n = outline.len();
    if n < 3 || outline.iter().any(|p| !p.is_finite()) {
        return Err(Refusal::NotPositive);
    }
    let area: f64 = (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum();
    let ring: Vec<DVec2> = if area >= 0.0 { outline.to_vec() } else { outline.iter().rev().copied().collect() };
    // edge i of the ring, as it was numbered in the outline
    let angle_of = |i: usize| angles.get(if area >= 0.0 { i } else { (2 * n - 2 - i) % n }).copied().unwrap_or(0.0);
    if (0..n).any(|i| !(0.0..=STEEPEST).contains(&angle_of(i))) {
        return Err(Refusal::NotPositive);
    }
    let eaves: Vec<usize> = (0..n).filter(|i| angle_of(*i) > 1e-9).collect();
    if eaves.is_empty() {
        return Ok(vec![(ring, None)]);
    }
    let tol = LINEAR_TOLERANCE;
    if (0..n).any(|i| (ring[(i + 1) % n] - ring[i]).perp_dot(ring[(i + 2) % n] - ring[(i + 1) % n]) < -tol) {
        return Err(Refusal::NotConvex);
    }
    // how high the plane of eave i is over point p, the eave at 0
    let rise = |i: usize, p: DVec2| {
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        (p - a).dot((b - a).normalize_or_zero().perp()) * angle_of(i).tan()
    };
    let mut planes = Vec::new();
    for &i in &eaves {
        // the outline, cut to where this plane lies under every other
        let mut part = ring.clone();
        for &j in eaves.iter().filter(|j| **j != i) {
            let over = |p: DVec2| rise(j, p) - rise(i, p);
            let mut cut: Vec<DVec2> = Vec::with_capacity(part.len() + 2);
            for k in 0..part.len() {
                let (a, b) = (part[k], part[(k + 1) % part.len()]);
                let (da, db) = (over(a), over(b));
                if da >= -tol {
                    cut.push(a);
                }
                if (da > tol && db < -tol) || (da < -tol && db > tol) {
                    cut.push(a + (b - a) * (da / (da - db)));
                }
            }
            cut.dedup_by(|a, b| a.distance(*b) <= tol);
            if cut.len() > 1 && cut[0].distance(cut[cut.len() - 1]) <= tol {
                cut.pop();
            }
            part = cut;
            if part.len() < 3 {
                break;
            }
        }
        let size: f64 = (0..part.len()).map(|k| part[k].perp_dot(part[(k + 1) % part.len()])).sum::<f64>() / 2.0;
        if part.len() < 3 || size <= tol {
            continue;
        }
        // its eave: the edge of the part that lies on the outline's edge i
        let (a, b) = (ring[i], ring[(i + 1) % n]);
        let across = (b - a).normalize_or_zero().perp();
        let on = |p: DVec2| (p - a).dot(across).abs() <= 10.0 * tol;
        let Some(edge) = (0..part.len()).find(|k| on(part[*k]) && on(part[(*k + 1) % part.len()]) && part[*k].distance(part[(*k + 1) % part.len()]) > tol) else { continue };
        planes.push((part, Some(Slope { edge, angle: angle_of(i) })));
    }
    if planes.is_empty() {
        return Err(Refusal::Degenerate);
    }
    Ok(planes)
}

/// The steepest a slab rises (radians): 85°.
pub const STEEPEST: f64 = 1.4835298641951802;

/// The form of a planar element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanarForm {
    /// A wall standing on its axis from `start` to `end` at height `base`, `height` high, centred
    /// on the axis.
    Wall { start: DVec2, end: DVec2, base: f64, height: f64 },
    /// A slab under `top`, of `outline` with `holes`.
    Slab {
        outline: Vec<DVec2>,
        #[serde(default)]
        holes: Vec<Vec<DVec2>>,
        top: f64,
        /// How far each edge of the outline bows: entry `i` is the bulge of the edge from corner
        /// `i` to the next (the tangent of a quarter of the arc's angle, above zero where the
        /// edge bows to the right of its way). Empty, or 0 for an edge: straight.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        bulges: Vec<f64>,
        /// What the slab is: a floor, a roof or a ceiling. A floor is not written.
        #[serde(default, skip_serializing_if = "SlabRole::is_floor")]
        role: SlabRole,
        /// How it rises from one of its edges (a roof's plane). None: it lies level.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        slope: Option<Slope>,
    },
    /// A stair from `base` up `height` in `risers` equal steps: a tread on every step but the
    /// last, which is the floor it reaches. The element's thickness is how thick it is under its
    /// treads. See [`StairShape`] for where its treads lie.
    Stair {
        base: f64,
        height: f64,
        risers: usize,
        shape: StairShape,
        /// A railing on both sides of its flights: a baluster on every tread and a handrail over
        /// them (a stair along a line; one in an outline has none).
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        railing: bool,
    },
}

/// Where the treads of a stair lie in plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StairShape {
    /// Along a line from its foot, `width` wide: a flight on every stretch of `path`, and a
    /// landing (a tread as wide as the stair is) on every corner of it.
    Run { path: Vec<DVec2>, width: f64 },
    /// Within an outline: the outline is cut into treads across the way it climbs, from `foot`
    /// to `head`. What of the outline lies before the foot or past the head is left out.
    Outline { outline: Vec<DVec2>, foot: DVec2, head: DVec2 },
}

impl StairShape {
    /// Its points in plan, to move or turn it by.
    pub fn points_mut(&mut self) -> Box<dyn Iterator<Item = &mut DVec2> + '_> {
        match self {
            StairShape::Run { path, .. } => Box::new(path.iter_mut()),
            StairShape::Outline { outline, foot, head } => Box::new(outline.iter_mut().chain([foot, head])),
        }
    }
}

/// How high a stair's handrail is above the treads, and how thick its balusters and its handrail
/// are (metres).
pub const RAILING_HEIGHT: f64 = 0.9;
pub const RAILING_SIZE: f64 = 0.04;

/// The railing of a stair along a line: on both sides of every flight a baluster on the middle of
/// every tread, and a handrail from each to the next. A landing and a stair in an outline have
/// none.
pub fn stair_railing(base: f64, height: f64, risers: usize, shape: &StairShape) -> Result<Vec<Solid>, Refusal> {
    let StairShape::Run { .. } = shape else { return Ok(Vec::new()) };
    let rise = height / risers.max(1) as f64;
    let inset = RAILING_SIZE;
    let bar = Profile::rectangle(RAILING_SIZE, RAILING_SIZE)?;
    let mut solids = Vec::new();
    // the tops of the balusters of the tread before, on its right and its left side
    let mut before: Option<([DVec3; 2], DVec2)> = None;
    for (ring, step, landing) in stair_steps(risers, shape)? {
        // a flight's tread is a rectangle from its near edge to its far one: its right side from
        // corner 0 to 1, its left from 3 to 2 (see `stair_steps`); a landing has none
        if landing || ring.len() != 4 {
            before = None;
            continue;
        }
        let way = (ring[1] - ring[0]).normalize_or_zero();
        let across = (ring[3] - ring[0]).normalize_or_zero();
        let top = base + step as f64 * rise;
        let feet = [(ring[0] + ring[1]) / 2.0 + across * inset, (ring[3] + ring[2]) / 2.0 - across * inset];
        let tops = feet.map(|f| f.extend(top + RAILING_HEIGHT));
        for foot in feet {
            solids.push(Solid::extrude(&bar, &Axes::new(foot.extend(top), DVec3::X, DVec3::Z)?, RAILING_HEIGHT)?);
        }
        // the handrail from the tread before, when it is of the same flight
        if let Some((from, _)) = before.filter(|(_, w)| w.distance(way) <= 1e-9) {
            for (a, b) in from.into_iter().zip(tops) {
                solids.push(Solid::extrude(&bar, &Axes::new(a, DVec3::Z, b - a)?, a.distance(b))?);
            }
        }
        before = Some((tops, way));
    }
    Ok(solids)
}

/// The fewest steps a stair has.
pub const FEWEST_RISERS: usize = 2;

/// The treads of a stair in plan, from its foot up, each a polygon (anticlockwise) with how many
/// steps above the base its top is. A landing is a tread among them.
pub fn stair_treads(risers: usize, shape: &StairShape) -> Result<Vec<(Vec<DVec2>, usize)>, Refusal> {
    Ok(stair_steps(risers, shape)?.into_iter().map(|(ring, step, _)| (ring, step)).collect())
}

/// The treads of [`stair_treads`], each with whether it is a landing.
fn stair_steps(risers: usize, shape: &StairShape) -> Result<Vec<(Vec<DVec2>, usize, bool)>, Refusal> {
    if risers < FEWEST_RISERS || risers > 10_000 {
        return Err(Refusal::NotPositive);
    }
    let treads = risers - 1;
    let ccw = |mut ring: Vec<DVec2>| {
        let area: f64 = (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum();
        if area < 0.0 {
            ring.reverse();
        }
        ring
    };
    match shape {
        StairShape::Run { path, width } => {
            if path.len() < 2 || !positive(*width) || path.iter().any(|p| !p.is_finite()) {
                return Err(Refusal::NotPositive);
            }
            let half = width / 2.0;
            let flights = path.len() - 1;
            // each flight between its landings: where it starts, the way it runs and how long it is
            let mut runs = Vec::with_capacity(flights);
            for i in 0..flights {
                let dir = (path[i + 1] - path[i]).try_normalize().ok_or(Refusal::ZeroLength)?;
                let from = path[i] + if i > 0 { dir * half } else { DVec2::ZERO };
                let to = path[i + 1] - if i + 1 < flights { dir * half } else { DVec2::ZERO };
                let length = (to - from).dot(dir);
                if length <= LINEAR_TOLERANCE {
                    return Err(Refusal::NotPositive);
                }
                runs.push((from, dir, length));
            }
            // the treads that are no landing, shared out by the flights' lengths, one each at least
            let landings = flights - 1;
            if treads < landings + flights {
                return Err(Refusal::NotPositive);
            }
            let free = treads - landings;
            let total: f64 = runs.iter().map(|r| r.2).sum();
            let mut counts: Vec<usize> = runs.iter().map(|r| ((free as f64 * r.2 / total).floor() as usize).max(1)).collect();
            // what rounding left over goes to the flights with the deepest treads; too many come
            // off those with the shallowest
            while counts.iter().sum::<usize>() < free {
                let k = (0..flights).max_by(|a, b| (runs[*a].2 / counts[*a] as f64).total_cmp(&(runs[*b].2 / counts[*b] as f64))).unwrap_or(0);
                counts[k] += 1;
            }
            while counts.iter().sum::<usize>() > free {
                let Some(k) = (0..flights).filter(|k| counts[*k] > 1).min_by(|a, b| (runs[*a].2 / counts[*a] as f64).total_cmp(&(runs[*b].2 / counts[*b] as f64))) else { return Err(Refusal::NotPositive) };
                counts[k] -= 1;
            }
            let mut out = Vec::with_capacity(treads);
            let mut step = 0;
            for (i, ((from, dir, length), count)) in runs.iter().zip(&counts).enumerate() {
                let (going, left) = (length / *count as f64, dir.perp() * half);
                for j in 0..*count {
                    let (a, b) = (*from + *dir * (j as f64 * going), *from + *dir * ((j + 1) as f64 * going));
                    step += 1;
                    out.push((vec![a - left, b - left, b + left, a + left], step, false));
                }
                if i + 1 < flights {
                    // the landing: between the end of this flight and the start of the next, out
                    // to its outer corner
                    let (next, way, _) = runs[i + 1];
                    let (end, corner) = (*from + *dir * *length, path[i + 1]);
                    let across = way.perp() * half;
                    let points = [end - left, end + left, next - across, next + across, corner + (*dir - way) * half, corner - (*dir - way) * half];
                    step += 1;
                    out.push((hull(&points), step, true));
                }
            }
            Ok(out)
        }
        StairShape::Outline { outline, foot, head } => {
            if outline.len() < 3 || outline.iter().chain([foot, head]).any(|p| !p.is_finite()) {
                return Err(Refusal::NotPositive);
            }
            let dir = (*head - *foot).try_normalize().ok_or(Refusal::ZeroLength)?;
            let going = (*head - *foot).length() / treads as f64;
            if going <= LINEAR_TOLERANCE {
                return Err(Refusal::NotPositive);
            }
            let ring = ccw(outline.clone());
            let mut out = Vec::with_capacity(treads);
            for j in 0..treads {
                // what of the outline lies between this tread's two riser lines
                let (lo, hi) = (j as f64 * going, (j + 1) as f64 * going);
                let strip = clip(&clip(&ring, *foot, dir, lo, true), *foot, dir, hi, false);
                let area: f64 = (0..strip.len()).map(|i| strip[i].perp_dot(strip[(i + 1) % strip.len()])).sum::<f64>() / 2.0;
                if strip.len() >= 3 && area > LINEAR_TOLERANCE * going {
                    out.push((strip, j + 1, false));
                }
            }
            if out.is_empty() {
                return Err(Refusal::NotPositive);
            }
            Ok(out)
        }
    }
}

/// `ring` cut by the line across `dir` at `at` along it from `origin`: what lies past it
/// (`keep_past`) or before it.
fn clip(ring: &[DVec2], origin: DVec2, dir: DVec2, at: f64, keep_past: bool) -> Vec<DVec2> {
    let side = |p: DVec2| if keep_past { (p - origin).dot(dir) - at } else { at - (p - origin).dot(dir) };
    let mut out: Vec<DVec2> = Vec::with_capacity(ring.len() + 2);
    for i in 0..ring.len() {
        let (a, b) = (ring[i], ring[(i + 1) % ring.len()]);
        let (da, db) = (side(a), side(b));
        if da >= 0.0 {
            out.push(a);
        }
        if (da > 0.0 && db < 0.0) || (da < 0.0 && db > 0.0) {
            out.push(a + (b - a) * (da / (da - db)));
        }
    }
    // points that fell together are one
    out.dedup_by(|a, b| a.distance(*b) <= LINEAR_TOLERANCE);
    if out.len() > 1 && out[0].distance(out[out.len() - 1]) <= LINEAR_TOLERANCE {
        out.pop();
    }
    out
}

/// The convex outline round `points`, anticlockwise.
fn hull(points: &[DVec2]) -> Vec<DVec2> {
    let mut sorted = points.to_vec();
    sorted.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    sorted.dedup_by(|a, b| a.distance(*b) <= LINEAR_TOLERANCE);
    let mut chain = |points: &mut dyn Iterator<Item = DVec2>| {
        let mut out: Vec<DVec2> = Vec::new();
        for p in points {
            while out.len() >= 2 && (out[out.len() - 1] - out[out.len() - 2]).perp_dot(p - out[out.len() - 1]) <= LINEAR_TOLERANCE * LINEAR_TOLERANCE {
                out.pop();
            }
            out.push(p);
        }
        out.pop();
        out
    };
    let mut ring = chain(&mut sorted.iter().copied());
    ring.extend(chain(&mut sorted.iter().rev().copied()));
    ring
}

/// The solids of a stair: a block under every tread, `thickness` thicker than a step is high (so
/// each stands on the one below it), none reaching under the stair's base.
pub fn stair_solids(thickness: f64, base: f64, height: f64, risers: usize, shape: &StairShape) -> Result<Vec<Solid>, Refusal> {
    if !positive(thickness) || !positive(height) || !base.is_finite() {
        return Err(Refusal::NotPositive);
    }
    let rise = height / risers.max(1) as f64;
    let mut solids = Vec::new();
    for (ring, step) in stair_treads(risers, shape)? {
        let top = step as f64 * rise;
        let foot = (top - rise - thickness).max(0.0);
        let axes = Axes::new(DVec3::new(0.0, 0.0, base + foot), DVec3::X, DVec3::Z)?;
        solids.push(Solid::extrude(&Profile::new(ring, Vec::new())?, &axes, top - foot)?);
    }
    Ok(solids)
}

/// What a slab is in the building: it is made the same, and classed and listed by this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SlabRole {
    #[default]
    Floor,
    Roof,
    Ceiling,
}

impl SlabRole {
    pub fn is_floor(&self) -> bool {
        *self == SlabRole::Floor
    }
}

/// A bulge this small is a straight edge.
const STRAIGHT: f64 = 1e-9;
/// An arc is made of chords that each turn at most this far (radians).
const CHORD_TURN: f64 = std::f64::consts::PI / 16.0;

/// The middle and the radius of the arc from `a` to `b` with `bulge`, the angle it starts at and
/// the angle it turns through (above zero: counter-clockwise). `None` where it is straight.
pub fn arc_of(a: DVec2, b: DVec2, bulge: f64) -> Option<(DVec2, f64, f64, f64)> {
    let chord = b - a;
    let length = chord.length();
    if bulge.abs() < STRAIGHT || !positive(length) {
        return None;
    }
    let left = DVec2::new(-chord.y, chord.x) / length;
    let centre = (a + b) / 2.0 + left * (length / 2.0 * (1.0 - bulge * bulge) / (2.0 * bulge));
    let from = a - centre;
    Some((centre, from.length(), from.y.atan2(from.x), 4.0 * bulge.atan()))
}

/// The points of the edge from `a` to `b` with `bulge`, `a` first and without `b`: `a` alone where
/// it is straight, else the arc in chords.
pub fn arc_points(a: DVec2, b: DVec2, bulge: f64) -> Vec<DVec2> {
    let Some((centre, radius, start, turn)) = arc_of(a, b, bulge) else { return vec![a] };
    let n = ((turn.abs() / CHORD_TURN).ceil() as usize).max(2);
    std::iter::once(a).chain((1..n).map(|k| centre + radius * DVec2::from_angle(start + turn * k as f64 / n as f64))).collect()
}

/// `outline` with its bowed edges in chords: the ring a solid is made of.
pub fn bowed(outline: &[DVec2], bulges: &[f64]) -> Vec<DVec2> {
    let n = outline.len();
    (0..n).flat_map(|i| arc_points(outline[i], outline[(i + 1) % n], bulges.get(i).copied().unwrap_or(0.0))).collect()
}

/// A planar element: a wall or a slab, `thickness` thick, with openings (walls only). With a
/// build-up, it is made of the build-up's layers and is as thick as they are together.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlanarDef {
    pub form: PlanarForm,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Opening>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_up: Option<BuildUpDef>,
}

/// A layer of a planar element: a material (a key of the project's library) and a thickness. A
/// cavity (an air layer) takes up its room but has no solid.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerDef {
    pub material: String,
    pub thickness: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cavity: bool,
}

/// A planar element's build-up: its type (the id of a build-up in the project's library) and the
/// type's layers, copied, so the element is whole without the library. A wall's layers run from
/// its exterior face in, its exterior on the right walking from start to end (as in Open BIM
/// Studio); a slab's from its top down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildUpDef {
    pub id: String,
    pub layers: Vec<LayerDef>,
}

impl BuildUpDef {
    /// The build-up of the library's build-up `id`; a layer of a material hatched as air is a
    /// cavity. `None` when the library has no such build-up.
    pub fn from_library(library: &bim_library::Library, id: &str) -> Option<Self> {
        let b = library.buildup(id)?;
        let layers = b
            .layers
            .iter()
            .map(|l| LayerDef {
                material: l.material.clone(),
                thickness: l.thickness,
                cavity: library.material(&l.material).is_some_and(|m| m.hatch == bim_library::HatchKind::Air),
            })
            .collect();
        Some(Self { id: b.id.clone(), layers })
    }

    /// The layers together.
    pub fn thickness(&self) -> f64 {
        self.layers.iter().map(|l| l.thickness).sum()
    }

    /// Checks the layers: each thicker than the linear tolerance, and at least one not a cavity.
    pub fn check(&self) -> Result<(), Refusal> {
        if self.layers.iter().any(|l| !positive(l.thickness)) {
            return Err(Refusal::NotPositive);
        }
        if self.layers.iter().all(|l| l.cavity) {
            return Err(Refusal::NoLayers);
        }
        Ok(())
    }
}

impl PlanarDef {
    /// How thick it is: its build-up's layers together, else its own thickness.
    pub fn total_thickness(&self) -> f64 {
        self.build_up.as_ref().map_or(self.thickness, BuildUpDef::thickness)
    }

    /// It with build-up `build_up` (or none): its thickness becomes the build-up's.
    pub fn with_build_up(mut self, build_up: Option<BuildUpDef>) -> Self {
        if let Some(b) = &build_up {
            self.thickness = b.thickness();
        }
        self.build_up = build_up;
        self
    }
}

/// A system: a vertical domain on its line from `start` to `end`, from `base` up `height`,
/// divided into equal bays about `spacing` (along, up) apart. Members of `member` (width in the
/// face, depth across it) run on the division lines; panels `panel` thick fill the bays. A type
/// (`system_type`, copied from a library) adds a running bond, the materials and layers across the
/// face ([`crate::system`]). A framed type (a timber frame) frames `openings` in the face, measured
/// as a wall's from the system's start and base; a system on a grid (brickwork, a curtain wall) is
/// cut where they lie, its panels and its members ending at them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SystemDef {
    pub start: DVec2,
    pub end: DVec2,
    pub base: f64,
    pub height: f64,
    pub spacing: DVec2,
    pub member: DVec2,
    pub panel: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_type: Option<crate::system::SystemTypeDef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub openings: Vec<Opening>,
    /// A framed system made in prefab units: where the user wants a joint between two units,
    /// along its line from its start. These come first; its type splits what lies between them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<f64>,
    /// A lying system (a floor, a ceiling) laid in an outline: the outline in plan. It is laid
    /// along its line over the rectangle that holds the outline and cut to it; its openings are
    /// measured from its line's start. A standing system that is not a rectangle (a corner off,
    /// a gable): the outline in its own face, each point along its line from its start and up
    /// from its base; it is made as the rectangle and cut to it. Empty: the rectangle its line
    /// and its `height` make.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outline: Vec<DVec2>,
    /// How its bays are divided along its line, where this system does not follow its type:
    /// equal ones, or whole ones with what is left at the end, at the start or at both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub division: Option<Division>,
    /// A curtain wall's own grid, once its lines were moved, added or taken away, or a bay got a
    /// panel of its own (see [`OwnGrid`]); none: equal bays about `spacing`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grid: Option<OwnGrid>,
}

/// The grid of a curtain wall that is its own, in place of equal bays: where its lines stand
/// between its edges, and the bays whose panel is not the glass of the rest. It counts for a
/// system with members on a stacked grid (a curtain wall); a framed or a bonded system keeps the
/// grid of its type.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OwnGrid {
    /// The vertical lines between its ends: how far along from its start, ascending.
    #[serde(default)]
    pub along: Vec<f64>,
    /// The horizontal lines between its foot and its head: how far up from its base, ascending.
    #[serde(default)]
    pub up: Vec<f64>,
    /// The bays with a panel of their own: the column (from its start) and the row (from its
    /// foot), and what fills it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub panels: Vec<(usize, usize, BayPanel)>,
}

/// What fills a bay of a curtain wall in place of its glass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BayPanel {
    /// Nothing: the bay is open.
    Open,
    /// A closed panel, of what the members are made of.
    Solid,
    /// A door leaf in the bay.
    Door,
}

impl OwnGrid {
    /// The equal grid of a face `length` by `height` in bays about `spacing`, as lines of its own.
    pub fn equal(length: f64, height: f64, spacing: DVec2) -> Self {
        let lines = |extent: f64, pitch: f64| {
            let n = ((extent / pitch).round().clamp(1.0, 1e6)) as usize;
            (1..n).map(|k| extent * k as f64 / n as f64).collect::<Vec<_>>()
        };
        Self { along: lines(length, spacing.x), up: lines(height, spacing.y), panels: Vec::new() }
    }

    /// What fills bay (`column`, `row`), when it is not glass.
    pub fn panel(&self, column: usize, row: usize) -> Option<BayPanel> {
        self.panels.iter().find(|(c, r, _)| (*c, *r) == (column, row)).map(|(_, _, p)| *p)
    }

    /// Its lines with the edges, along and up: from 0 to `length` and to `height`.
    pub fn stops(&self, length: f64, height: f64) -> (Vec<f64>, Vec<f64>) {
        let with = |lines: &[f64], extent: f64| std::iter::once(0.0).chain(lines.iter().copied()).chain(std::iter::once(extent)).collect::<Vec<_>>();
        (with(&self.along, length), with(&self.up, height))
    }
}

pub use bim_library::system::Division;

/// How a connection joins its members.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionKind {
    /// The secondary member stops at the face of the primary one, which runs through.
    Butt,
    /// Both members are cut by the plane that halves the angle between them.
    Mitre,
    /// The primary element cuts its body out of the secondary one (a beam through a wall, a duct
    /// through a slab). Any two elements with a body; the secondary keeps what is left.
    Cut,
    /// A steel end plate: the secondary stops a plate's thickness short of the primary's face,
    /// and a plate against that face is bolted to the primary (see [`crate::join::EndPlate`]).
    EndPlate(crate::join::EndPlate),
    /// A steel fin plate: the secondary stops a gap short of the primary's face, and a plate
    /// welded to that face is bolted through the secondary's web (see [`crate::join::FinPlate`]).
    FinPlate(crate::join::FinPlate),
    /// A steel splice: two members in one line each stop a plate's thickness short of where they
    /// meet, and the plates on their ends are bolted together (sizes as [`crate::join::EndPlate`]).
    Splice(crate::join::EndPlate),
    /// A steel base plate: the secondary (a column) stands on the primary on a plate, a bed of
    /// grout and four anchors (see [`crate::join::BasePlate`]).
    BasePlate(crate::join::BasePlate),
    /// A bend between two runs of a building service that meet end to end: both stop short of the
    /// corner and a fitting turns from the one into the other (see [`crate::join::Bend`]).
    Bend(crate::join::Bend),
    /// A tee: a run of a building service that ends on the side of another, which runs through; a
    /// fitting takes its end (see [`crate::join::Tee`]).
    Tee(crate::join::Tee),
    /// Two runs of a building service in one line with different profiles: a fitting goes from
    /// the one into the other (see [`crate::join::Transition`]).
    Transition(crate::join::Transition),
    /// The two are not joined where they meet: two building systems that would meet at a corner
    /// or a T junction are each made as they are alone, and two linear members get no join.
    Apart,
}

/// What a connection is to the analysis model: whether it passes a bending moment on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fixity {
    /// A hinge: it passes forces on, no moment.
    Pinned,
    /// It passes moments on.
    Rigid,
}

impl ConnectionKind {
    /// Whether it is a fitting between service runs (a bend, a tee, a transition): a body of its
    /// own in the run, where a butt or a mitre is only the way two members are cut.
    pub fn is_fitting(&self) -> bool {
        matches!(self, ConnectionKind::Bend(_) | ConnectionKind::Tee(_) | ConnectionKind::Transition(_))
    }

    /// What a steel connection is to the analysis model: a fin plate is a hinge, an end plate
    /// and a splice pass moments on, and a base plate with its four anchors is taken as a hinge. `None` for a join that is no connection of its own (a butt, a mitre,
    /// a cut).
    pub fn fixity(&self) -> Option<Fixity> {
        match self {
            ConnectionKind::FinPlate(_) | ConnectionKind::BasePlate(_) => Some(Fixity::Pinned),
            ConnectionKind::EndPlate(_) | ConnectionKind::Splice(_) => Some(Fixity::Rigid),
            _ => None,
        }
    }
}

/// A connection between two elements: two linear ones joined (butt, mitre), any two with a body
/// where one cuts the other, or two framed building systems where they meet at an edge (a corner
/// or a T junction): butt, the primary running through at a corner, or apart.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConnectionDef {
    pub kind: ConnectionKind,
    pub primary: ElementId,
    pub secondary: ElementId,
}

/// An element's definition, by base element.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Point(PointDef),
    Linear(LinearDef),
    Planar(PlanarDef),
    System(SystemDef),
    Connection(ConnectionDef),
}

/// Where the top of an element reaches: a level (its IfcX path) and how far above it (metres;
/// below it when negative). A wall's or a standing system's height, or a column's top end, is
/// worked out from it whenever the level moves; with none the element keeps its own height.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Top {
    pub level: String,
    #[serde(default)]
    pub offset: f64,
}

/// An element: its id, name, tile, level and definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Element {
    pub id: ElementId,
    pub name: String,
    /// The tile whose frame its coordinates are in: the one holding its placement origin.
    pub tile: TileKey,
    /// The level it is placed on (the level's IfcX path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    /// The level its top reaches, if it reaches one (see [`Top`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub top: Option<Top>,
    /// The strength class of what it is made of, when it is given: a steel grade (S235), a
    /// concrete class (C30/37), a timber class (C24).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grade: Option<String>,
    /// Issued pile mark, retained through changes of shape or position.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pile_number: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pile_mark: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub reinforcing_bar: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reinforcement_areas: Vec<crate::reinforcement::Area>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hairpin_leg: Option<f64>,
    /// The reinforcement of a concrete member with a rectangular section, when it is given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reinforcement: Option<crate::reinforcement::Reinforcement>,
    /// The nets in a slab or a wall, when they are given.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nets: Option<crate::reinforcement::Nets>,
    /// Reinforcement drawn besides the usual one: stirrups or bars over a stretch of a
    /// reinforced member, bars along a line in a slab with nets.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extras: Vec<crate::reinforcement::Extra>,
    /// The service it belongs to, when it is part of an installation (see [`Service`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<Service>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service_type: Option<String>,
    pub shape: Shape,
}

impl Element {
    /// An element whose `shape` is given in project coordinates: it is placed in the tile that
    /// holds its placement origin, its coordinates turned into that tile's frame. Its id is set
    /// when it is added to a model.
    pub fn in_project(grid: &TileGrid, name: impl Into<String>, level: Option<String>, mut shape: Shape) -> Self {
        let tile = shape.placement().map_or(TileKey::default(), |p| grid.tile_of(p));
        shape.translate(-grid.origin(tile));
        Self { id: ElementId(0), name: name.into(), tile, level, top: None, grade: None, service: None, service_type: None, reinforcement: None, nets: None, extras: Vec::new(), pile_number: None, pile_mark: None, reinforcing_bar: false, reinforcement_areas: Vec::new(), hairpin_leg: None, shape }
    }

    /// The same element (its id, name, level, top, grade and service) with `shape`, given in project coordinates.
    pub fn reshaped(&self, grid: &TileGrid, shape: Shape) -> Self {
        Self { id: self.id, pile_number: self.pile_number, pile_mark: self.pile_mark.clone(), hairpin_leg: self.hairpin_leg, reinforcement_areas: self.reinforcement_areas.clone(), reinforcing_bar: self.reinforcing_bar, top: self.top.clone(), grade: self.grade.clone(), service: self.service, service_type: self.service_type.clone(), reinforcement: self.reinforcement, nets: self.nets, extras: self.extras.clone(), ..Self::in_project(grid, self.name.clone(), self.level.clone(), shape) }
    }

    /// Its definition in project coordinates.
    pub fn shape_in_project(&self, grid: &TileGrid) -> Shape {
        let mut shape = self.shape.clone();
        shape.translate(grid.origin(self.tile));
        shape
    }

    /// Its base element.
    pub fn base(&self) -> BaseKind {
        self.shape.base()
    }

    /// Its IFC class (IFC4.3 names, ADR-0010).
    pub fn class(&self) -> &'static str {
        match &self.shape {
            Shape::Point(PointDef { opening: Some(OpeningKind::Door), .. }) => "IfcDoor",
            Shape::Point(PointDef { opening: Some(OpeningKind::Window), .. }) => "IfcWindow",
            Shape::Point(p) => p.role.class(),
            Shape::Linear(_) if self.pile_number.is_some() => "IfcPile",
            Shape::Linear(_) if self.reinforcing_bar => "IfcReinforcingBar",
            Shape::Linear(l) => l.role.segment_class().unwrap_or_else(|| linear_class(l.end - l.start)),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { .. }, .. }) => "IfcWall",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { role: SlabRole::Roof, .. }, .. }) => "IfcRoof",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { role: SlabRole::Ceiling, .. }, .. }) => "IfcCovering",
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { .. }, .. }) => "IfcSlab",
            Shape::Planar(PlanarDef { form: PlanarForm::Stair { .. }, .. }) => "IfcStair",
            Shape::System(_) => "IfcCurtainWall",
            // a join with parts of its own is an assembly of them; one without is a relation only
            Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(_) | ConnectionKind::FinPlate(_) | ConnectionKind::Splice(_) | ConnectionKind::BasePlate(_), .. }) => "IfcElementAssembly",
            // a fitting of a building service: the bend or the tee itself
            Shape::Connection(ConnectionDef { kind: ConnectionKind::Bend(b), .. }) => b.role.fitting_class(),
            Shape::Connection(ConnectionDef { kind: ConnectionKind::Tee(t), .. }) => t.role.fitting_class(),
            Shape::Connection(ConnectionDef { kind: ConnectionKind::Transition(t), .. }) => t.role.fitting_class(),
            Shape::Connection(_) => "IfcRelConnectsElements",
        }
    }

    /// Moves it by `delta` (metres). When its placement origin leaves its tile, it moves to the
    /// tile that holds it now, its coordinates carried over exactly.
    pub fn translate(&mut self, grid: &TileGrid, delta: DVec3) {
        self.shape.translate(delta);
        self.rehome(grid);
    }

    /// Puts it in the tile that holds its placement origin (ADR-0016), if it is not there.
    pub fn rehome(&mut self, grid: &TileGrid) {
        let Some(placement) = self.shape.placement() else { return };
        let home = grid.tile_of(grid.origin(self.tile) + placement);
        if home != self.tile {
            self.shape.translate(grid.offset(self.tile, home));
            self.tile = home;
        }
    }
}

impl Shape {
    /// Its base element.
    pub fn base(&self) -> BaseKind {
        match self {
            Shape::Point(_) => BaseKind::Point,
            Shape::Linear(_) => BaseKind::Linear,
            Shape::Planar(_) => BaseKind::Planar,
            Shape::System(_) => BaseKind::System,
            Shape::Connection(_) => BaseKind::Connection,
        }
    }

    /// Its placement origin; a connection has none (it lives in its primary member's tile).
    pub fn placement(&self) -> Option<DVec3> {
        match self {
            Shape::Point(p) => Some(p.at),
            Shape::Linear(l) => Some(l.start),
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, base, .. }, .. }) => Some(start.extend(*base)),
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => outline.first().map(|p| p.extend(*top)),
            Shape::Planar(PlanarDef { form: PlanarForm::Stair { base, shape, .. }, .. }) => match shape {
                StairShape::Run { path, .. } => path.first().map(|p| p.extend(*base)),
                StairShape::Outline { foot, .. } => Some(foot.extend(*base)),
            },
            Shape::System(s) => Some(s.start.extend(s.base)),
            Shape::Connection(_) => None,
        }
    }

    /// Moves every coordinate by `delta`.
    pub fn translate(&mut self, delta: DVec3) {
        let flat = delta.truncate();
        match self {
            Shape::Point(p) => p.at += delta,
            Shape::Linear(l) => {
                l.start += delta;
                l.end += delta;
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => {
                *start += flat;
                *end += flat;
                *base += delta.z;
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, holes, top, .. }, .. }) => {
                outline.iter_mut().chain(holes.iter_mut().flatten()).for_each(|p| *p += flat);
                *top += delta.z;
            }
            Shape::Planar(PlanarDef { form: PlanarForm::Stair { base, shape, .. }, .. }) => {
                shape.points_mut().for_each(|p| *p += flat);
                *base += delta.z;
            }
            Shape::System(s) => {
                s.start += flat;
                s.end += flat;
                // a lying system's outline is in plan; a standing one's is in its own face
                if s.lies() {
                    s.outline.iter_mut().for_each(|p| *p += flat);
                }
                s.base += delta.z;
            }
            Shape::Connection(_) => {}
        }
    }
}

/// The IFC class of a linear element running along `axis`: a column within 1° of the vertical, a
/// beam within 1° of the horizontal, otherwise a member.
pub fn linear_class(axis: DVec3) -> &'static str {
    let Some(a) = axis.try_normalize() else { return "IfcMember" };
    let sin_1 = 1f64.to_radians().sin();
    if a.z.abs() >= 1f64.to_radians().cos() {
        "IfcColumn"
    } else if a.z.abs() <= sin_1 {
        "IfcBeam"
    } else {
        "IfcMember"
    }
}

/// A solid of an element and the IFC class of what it is (a curtain wall's members and panels have
/// classes of their own), and what it is made of when that is known (a material key of the
/// project's library).
#[derive(Debug, Clone, PartialEq)]
pub struct PartSolid {
    pub class: &'static str,
    pub solid: Solid,
    pub material: Option<String>,
}

/// The frame of a linear element: at its start, `z` along it, the profile's `x` square to it
/// (horizontal, or the project's x for a vertical member), turned by `roll`; and its length.
pub fn linear_axes(l: &LinearDef) -> Result<(Axes, f64), Refusal> {
    let axis = l.end - l.start;
    let length = axis.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let z = axis / length;
    let x = if z.z.abs() > 1.0 - 1e-12 { DVec3::X } else { DVec3::Z.cross(z).normalize() };
    let (sin, cos) = l.roll.sin_cos();
    let x = x * cos + z.cross(x) * sin;
    Ok((Axes::new(l.start, x, z)?, length))
}

/// The planes across a linear element at its two ends.
pub fn square_ends(axes: &Axes, length: f64) -> (Plane, Plane) {
    (Plane { point: axes.origin, normal: -axes.z }, Plane { point: axes.origin + axes.z * length, normal: axes.z })
}

/// The solid of a linear element between end planes (its own square ends where `None`).
pub fn linear_solid(l: &LinearDef, start: Option<Plane>, end: Option<Plane>) -> Result<Solid, Refusal> {
    let (axes, length) = linear_axes(l)?;
    let (s, e) = square_ends(&axes, length);
    Solid::sweep_between(&l.profile.profile()?, &axes, start.unwrap_or(s), end.unwrap_or(e))
}

/// The solid of a point element.
pub fn point_solid(p: &PointDef) -> Result<Solid, Refusal> {
    if !positive(p.size.z) {
        return Err(Refusal::NotPositive);
    }
    let (sin, cos) = p.rotation.sin_cos();
    let axes = Axes::new(p.at, DVec3::new(cos, sin, 0.0), DVec3::Z)?;
    Solid::extrude(&Profile::rectangle(p.size.x, p.size.y)?, &axes, p.size.z)
}

/// Where a run of a building service connects to an appliance or a terminal.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Connector {
    pub at: DVec3,
    /// The way a run leaves it (a unit vector).
    pub out: DVec3,
    /// The kind of run it takes.
    pub role: LinearRole,
    /// The service it is of; with none it takes the service of its appliance (the two sides of a
    /// pump or a valve).
    pub service: Option<Service>,
    /// The diameter of its stub.
    pub size: f64,
}

/// How far a connector's stub stands out of its appliance.
pub const STUB: f64 = 0.04;

impl PointRole {
    /// Its connectors in the box of its element: where (across the width and the depth from
    /// −0.5 to 0.5, up the height from 0 to 1), the way out, and what connects.
    fn connections(self) -> &'static [([f64; 3], [f64; 3], LinearRole, Option<Service>)] {
        use LinearRole::{Duct, Pipe};
        const LEFT: [f64; 3] = [-1.0, 0.0, 0.0];
        const RIGHT: [f64; 3] = [1.0, 0.0, 0.0];
        const BACK: [f64; 3] = [0.0, 1.0, 0.0];
        const DOWN: [f64; 3] = [0.0, 0.0, -1.0];
        const UP: [f64; 3] = [0.0, 0.0, 1.0];
        match self {
            PointRole::Object | PointRole::Outlet | PointRole::Light => &[],
            PointRole::Radiator => &[([-0.5, 0.0, 0.1], LEFT, Pipe, Some(Service::HeatingSupply)), ([0.5, 0.0, 0.1], RIGHT, Pipe, Some(Service::HeatingReturn))],
            PointRole::Boiler => &[
                ([-0.36, 0.0, 0.0], DOWN, Pipe, Some(Service::HeatingSupply)),
                ([-0.18, 0.0, 0.0], DOWN, Pipe, Some(Service::HotWater)),
                ([0.0, 0.0, 0.0], DOWN, Pipe, Some(Service::Gas)),
                ([0.18, 0.0, 0.0], DOWN, Pipe, Some(Service::ColdWater)),
                ([0.36, 0.0, 0.0], DOWN, Pipe, Some(Service::HeatingReturn)),
                ([0.0, 0.0, 1.0], UP, Duct, Some(Service::ExhaustAir)),
            ],
            PointRole::Pump | PointRole::Valve => &[([-0.5, 0.0, 0.5], LEFT, Pipe, None), ([0.5, 0.0, 0.5], RIGHT, Pipe, None)],
            PointRole::Fan => &[([-0.5, 0.0, 0.5], LEFT, Duct, None), ([0.5, 0.0, 0.5], RIGHT, Duct, None)],
            PointRole::AirHandlingUnit => &[
                ([-0.5, -0.25, 0.5], LEFT, Duct, Some(Service::ReturnAir)),
                ([-0.5, 0.25, 0.5], LEFT, Duct, Some(Service::ExhaustAir)),
                ([0.5, 0.25, 0.5], RIGHT, Duct, Some(Service::SupplyAir)),
            ],
            PointRole::AirTerminal => &[([0.0, 0.0, 1.0], UP, Duct, None)],
            PointRole::Sanitary => &[
                ([-0.25, 0.5, 0.8], BACK, Pipe, Some(Service::ColdWater)),
                ([0.25, 0.5, 0.8], BACK, Pipe, Some(Service::HotWater)),
                ([0.0, 0.5, 0.2], BACK, Pipe, Some(Service::Waste)),
            ],
            PointRole::Tank => &[([0.5, 0.0, 0.1], RIGHT, Pipe, Some(Service::ColdWater)), ([0.0, 0.0, 1.0], UP, Pipe, Some(Service::HotWater))],
        }
    }
}

impl PointDef {
    /// Its connectors: where the runs of the building services connect to it, by its role. A
    /// door, a window or an object has none.
    pub fn connectors(&self) -> Vec<Connector> {
        if self.opening.is_some() {
            return Vec::new();
        }
        let (sin, cos) = self.rotation.sin_cos();
        let turned = |v: DVec3| DVec3::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos, v.z);
        self.role
            .connections()
            .iter()
            .map(|(at, out, role, service)| {
                let out = DVec3::from_array(*out);
                // the stub fits the face it stands on
                let face = [self.size.x, self.size.y, self.size.z].into_iter().zip(out.to_array()).filter(|(_, o)| o.abs() < 0.5).map(|(s, _)| s).fold(f64::INFINITY, f64::min);
                let widest: f64 = if *role == LinearRole::Duct { 0.25 } else { 0.04 };
                Connector { at: self.at + turned(DVec3::from_array(*at) * self.size), out: turned(out), role: *role, service: *service, size: widest.min(0.6 * face) }
            })
            .collect()
    }
}

/// The stubs of a point element's connectors: a short round piece standing out of it at each.
fn stub_solids(p: &PointDef) -> Vec<Solid> {
    p.connectors()
        .into_iter()
        .filter_map(|c| {
            let across = if c.out.z.abs() > 0.5 { DVec3::X } else { DVec3::Z };
            let axes = Axes::new(c.at, across, c.out).ok()?;
            Solid::extrude(&Profile::circle(c.size).ok()?, &axes, STUB).ok()
        })
        .collect()
}

/// How wide the members of a door or window frame are, how deep at least, and how thick a pane.
pub const FRAME_WIDTH: f64 = 0.06;
pub const FRAME_DEPTH: f64 = 0.09;
pub const PANE: f64 = 0.02;

/// The solids of a door or a window: its frame (a door has none under its leaf) and what fills
/// it, a leaf or a pane of glass (marked true). One too small for a frame is a single block.
pub fn opening_solids(p: &PointDef) -> Result<Vec<(Solid, bool)>, Refusal> {
    let (w, h, f) = (p.size.x, p.size.z, FRAME_WIDTH);
    let Some(kind) = p.opening.filter(|_| w > 3.0 * f && h > 3.0 * f) else {
        // a block, and the stubs of its connectors when it is an appliance
        return Ok(std::iter::once(point_solid(p)?).chain(stub_solids(p)).map(|solid| (solid, false)).collect());
    };
    let depth = p.size.y.max(FRAME_DEPTH);
    let (sin, cos) = p.rotation.sin_cos();
    let along = DVec3::new(cos, sin, 0.0);
    // a block: its middle along the width, its foot above the sill, and its sizes
    let block = |x: f64, z: f64, size: DVec3| point_solid(&PointDef { at: p.at + along * x + DVec3::Z * z, rotation: p.rotation, size, opening: None, role: Default::default() });
    let side = w / 2.0 - f / 2.0;
    let mut solids = Vec::new();
    match kind {
        OpeningKind::Window => {
            solids.push((block(0.0, 0.0, DVec3::new(w, depth, f))?, false));
            solids.push((block(0.0, h - f, DVec3::new(w, depth, f))?, false));
            for x in [-side, side] {
                solids.push((block(x, f, DVec3::new(f, depth, h - 2.0 * f))?, false));
            }
            solids.push((block(0.0, f, DVec3::new(w - 2.0 * f, PANE, h - 2.0 * f))?, true));
        }
        OpeningKind::Door => {
            solids.push((block(0.0, h - f, DVec3::new(w, depth, f))?, false));
            for x in [-side, side] {
                solids.push((block(x, 0.0, DVec3::new(f, depth, h - f))?, false));
            }
            solids.push((block(0.0, 0.0, DVec3::new(w - 2.0 * f, p.size.y.min(depth), h - f))?, false));
        }
    }
    Ok(solids)
}

/// The solids of a planar element, each through all its layers: one, or one per piece of a wall
/// that openings through its whole height (or width) split.
pub fn planar_solids(p: &PlanarDef) -> Result<Vec<Solid>, Refusal> {
    let thickness = p.total_thickness();
    if !positive(thickness) {
        return Err(Refusal::NotPositive);
    }
    if let PlanarForm::Stair { base, height, risers, shape, railing } = &p.form {
        let mut solids = stair_solids(thickness, *base, *height, *risers, shape)?;
        if *railing {
            solids.extend(stair_railing(*base, *height, *risers, shape)?);
        }
        return Ok(solids);
    }
    if let Some(b) = &p.build_up {
        b.check()?;
    }
    let (profiles, axes) = planar_base(p, thickness, [0.0; 2])?;
    profiles.iter().map(|profile| Solid::extrude(profile, &axes, thickness)).collect()
}

/// The solids of a planar element with their materials: one per layer of its build-up (none for a
/// cavity), or its one solid without a material.
pub fn planar_parts(p: &PlanarDef) -> Result<Vec<(Option<String>, Solid)>, Refusal> {
    planar_parts_extended(p, [0.0; 2])
}

/// The solids of a planar element as [`planar_parts`] makes them, a wall reaching `extend` metres
/// past its start and its end (to be cut back to its corners, see `joins`); a slab as it is.
pub fn planar_parts_extended(p: &PlanarDef, extend: [f64; 2]) -> Result<Vec<(Option<String>, Solid)>, Refusal> {
    // a stair is its treads, of one material
    if let PlanarForm::Stair { .. } = &p.form {
        return Ok(planar_solids(p)?.into_iter().map(|solid| (None, solid)).collect());
    }
    let Some(b) = &p.build_up else {
        let thickness = p.total_thickness();
        if !positive(thickness) {
            return Err(Refusal::NotPositive);
        }
        let (profiles, axes) = planar_base(p, thickness, extend)?;
        return profiles.iter().map(|profile| Ok((None, Solid::extrude(profile, &axes, thickness)?))).collect();
    };
    b.check()?;
    let thickness = b.thickness();
    let (profiles, axes) = planar_base(p, thickness, extend)?;
    let mut parts = Vec::new();
    let mut from = 0.0;
    for layer in &b.layers {
        let to = from + layer.thickness;
        if !layer.cavity {
            // a wall's layers from its exterior (its right face, at z = thickness) in; a slab's
            // from its top (at z = thickness) down
            let at = Axes::new(axes.origin + axes.z * (thickness - to), axes.x, axes.z)?;
            for profile in &profiles {
                parts.push((Some(layer.material.clone()), Solid::extrude(profile, &at, layer.thickness)?));
            }
        }
        from = to;
    }
    Ok(parts)
}

/// What a planar element `thickness` thick extrudes, and the frame it extrudes in along its z: a
/// wall's elevation from its left face (reaching `extend` past its start and end), a profile per
/// piece of wall; a slab's outline from its underside.
fn planar_base(p: &PlanarDef, thickness: f64, extend: [f64; 2]) -> Result<(Vec<Profile>, Axes), Refusal> {
    match &p.form {
        PlanarForm::Wall { start, end, base, height } => {
            let (axes, length) = wall_axes(*start, *end, *base, thickness)?;
            if !positive(*height) {
                return Err(Refusal::NotPositive);
            }
            // the openings must fit the wall itself, not only its reach past its ends
            let elevation = wall_elevation(length, *height, &p.openings)?;
            if extend == [0.0; 2] {
                return Ok((elevation, axes));
            }
            // an opening that reaches an end goes on through the reach past it
            let tol = LINEAR_TOLERANCE;
            let reach_length = length + extend[0] + extend[1];
            let shifted: Vec<Opening> = p
                .openings
                .iter()
                .map(|o| {
                    let from = if o.along <= tol { 0.0 } else { o.along + extend[0] };
                    let to = if o.along + o.width >= length - tol { reach_length } else { o.along + o.width + extend[0] };
                    Opening { along: from, width: to - from, ..*o }
                })
                .collect();
            let reach = Axes::new(axes.origin - axes.x * extend[0], axes.x, axes.z)?;
            Ok((wall_elevation(reach_length, *height, &shifted)?, reach))
        }
        PlanarForm::Slab { outline, holes, top, bulges, slope, .. } => {
            let ring = bowed(outline, bulges);
            let Some(s) = slope.filter(|s| s.angle.abs() > 1e-9 && outline.len() >= 3) else {
                let axes = Axes::new(DVec3::new(0.0, 0.0, top - thickness), DVec3::X, DVec3::Z)?;
                return Ok((vec![Profile::new(ring, holes.clone())?], axes));
            };
            if !(s.angle.abs() <= STEEPEST) {
                return Err(Refusal::NotPositive);
            }
            // the eave, and the way in from it: to the left of an outline that runs anticlockwise
            let n = outline.len();
            let (a, b) = (outline[s.edge % n], outline[(s.edge + 1) % n]);
            let along = (b - a).normalize_or_zero();
            if along == DVec2::ZERO {
                return Err(Refusal::ZeroLength);
            }
            let area: f64 = (0..n).map(|k| outline[k].perp_dot(outline[(k + 1) % n])).sum();
            let d = if area >= 0.0 { along.perp() } else { -along.perp() };
            // the plane it lies in: x up the slope, y level along the eave, z square to it
            let (sin, cos) = s.angle.sin_cos();
            let x = (d * cos).extend(sin);
            let normal = (-d * sin).extend(cos);
            let level = d.perp();
            let on = |q: &DVec2| DVec2::new((*q - a).dot(d) / cos, (*q - a).dot(level));
            let axes = Axes::new(a.extend(*top) - normal * thickness, x, normal)?;
            let profile = Profile::new(ring.iter().map(on).collect(), holes.iter().map(|h| h.iter().map(on).collect()).collect())?;
            Ok((vec![profile], axes))
        }
        // a stair is no one profile extruded (see `stair_solids`)
        PlanarForm::Stair { .. } => Err(Refusal::Degenerate),
    }
}

/// The frame of a wall's elevation: at the axis start, `thickness / 2` to its left, at `base`;
/// `x` along the axis, `y` up, `z` across to the right. And the axis length.
fn wall_axes(start: DVec2, end: DVec2, base: f64, thickness: f64) -> Result<(Axes, f64), Refusal> {
    let d = end - start;
    let length = d.length();
    if !positive(length) {
        return Err(Refusal::ZeroLength);
    }
    let dir = (d / length).extend(0.0);
    let right = dir.cross(DVec3::Z);
    Ok((Axes::new(start.extend(base) - right * (thickness / 2.0), dir, right)?, length))
}

/// A wall's elevation, `length` × `height`, less its openings: a profile per piece of wall that is
/// left (from the start, bottom up), each outline with the holes in it.
///
/// An opening may reach the base (a door), the top or an end, and an opening through the wall's
/// whole height (or length) splits it; what lies past the wall's edges takes nothing. An opening
/// must take something from the wall, openings must not touch, and some wall must be left.
///
/// The elevation is cut on the grid of the wall's and the openings' edges (an edge within the
/// linear tolerance of another taken onto it): each cell is wall or opening, the cells of wall that
/// share a side are one piece, and a piece's outline and holes are the sides between wall and the
/// rest, run with the wall on their left.
fn wall_elevation(length: f64, height: f64, openings: &[Opening]) -> Result<Vec<Profile>, Refusal> {
    let tol = LINEAR_TOLERANCE;
    for o in openings {
        if !positive(o.width) || !positive(o.height) || !o.along.is_finite() || !o.sill.is_finite() {
            return Err(Refusal::NotPositive);
        }
        if o.along >= length - tol || o.along + o.width <= tol || o.sill >= height - tol || o.sill + o.height <= tol {
            return Err(Refusal::OpeningOutside);
        }
    }
    for (i, a) in openings.iter().enumerate() {
        for b in &openings[i + 1..] {
            let apart_along = a.along + a.width < b.along - tol || b.along + b.width < a.along - tol;
            let apart_up = a.sill + a.height < b.sill - tol || b.sill + b.height < a.sill - tol;
            if !apart_along && !apart_up {
                return Err(Refusal::OpeningsOverlap);
            }
        }
    }
    // the grid lines: the wall's edges and the openings' within it, near ones taken as one
    let lines = |ends: Vec<f64>, max: f64| -> Vec<f64> {
        let mut values: Vec<f64> = ends.into_iter().map(|v| v.clamp(0.0, max)).chain([0.0, max]).collect();
        values.sort_by(f64::total_cmp);
        let mut kept: Vec<f64> = Vec::new();
        for v in values {
            if kept.last().is_none_or(|k| v - k > tol) {
                kept.push(v);
            }
        }
        // the far edge is the wall's own
        if let Some(last) = kept.last_mut() {
            *last = max;
        }
        kept
    };
    let xs = lines(openings.iter().flat_map(|o| [o.along, o.along + o.width]).collect(), length);
    let ys = lines(openings.iter().flat_map(|o| [o.sill, o.sill + o.height]).collect(), height);
    let (nx, ny) = (xs.len() - 1, ys.len() - 1);
    // a cell is wall unless its middle lies in an opening
    let wall = |i: usize, j: usize| {
        let (x, y) = ((xs[i] + xs[i + 1]) / 2.0, (ys[j] + ys[j + 1]) / 2.0);
        !openings.iter().any(|o| o.along < x && x < o.along + o.width && o.sill < y && y < o.sill + o.height)
    };
    let solid: Vec<Vec<bool>> = (0..nx).map(|i| (0..ny).map(|j| wall(i, j)).collect()).collect();
    let mut piece = vec![vec![usize::MAX; ny]; nx];
    let mut pieces = Vec::new();
    for i0 in 0..nx {
        for j0 in 0..ny {
            if !solid[i0][j0] || piece[i0][j0] != usize::MAX {
                continue;
            }
            // the cells of this piece, and the sides it shares with the rest, wall on their left
            let k = pieces.len();
            // ordered, so every ring starts at its lowest corner and the result is the same every time
            let mut edges: std::collections::BTreeMap<(usize, usize), (usize, usize)> = std::collections::BTreeMap::new();
            let mut stack = vec![(i0, j0)];
            piece[i0][j0] = k;
            while let Some((i, j)) = stack.pop() {
                let neighbours = [(i.checked_sub(1), Some(j)), ((i + 1 < nx).then_some(i + 1), Some(j)), (Some(i), j.checked_sub(1)), (Some(i), (j + 1 < ny).then_some(j + 1))];
                for (a, b) in neighbours {
                    if let (Some(a), Some(b)) = (a, b) {
                        if solid[a][b] && piece[a][b] == usize::MAX {
                            piece[a][b] = k;
                            stack.push((a, b));
                        }
                    }
                }
                let open = |a: Option<usize>, b: Option<usize>| !matches!((a, b), (Some(a), Some(b)) if solid[a][b]);
                let sides = [
                    (open(Some(i), j.checked_sub(1)), (i, j), (i + 1, j)),
                    (open((i + 1 < nx).then_some(i + 1), Some(j)), (i + 1, j), (i + 1, j + 1)),
                    (open(Some(i), (j + 1 < ny).then_some(j + 1)), (i + 1, j + 1), (i, j + 1)),
                    (open(i.checked_sub(1), Some(j)), (i, j + 1), (i, j)),
                ];
                for (bounds, from, to) in sides {
                    // a corner two sides leave is a pinch, which only touching openings make
                    if bounds && edges.insert(from, to).is_some() {
                        return Err(Refusal::OpeningsOverlap);
                    }
                }
            }
            // the sides run into rings: the outline counter-clockwise, holes clockwise
            let mut outer = None;
            let mut holes = Vec::new();
            while let Some((&start, _)) = edges.first_key_value() {
                let mut ring = Vec::new();
                let mut at = start;
                while let Some(next) = edges.remove(&at) {
                    ring.push(DVec2::new(xs[at.0], ys[at.1]));
                    at = next;
                }
                let area: f64 = (0..ring.len()).map(|n| ring[n].perp_dot(ring[(n + 1) % ring.len()])).sum();
                if area > 0.0 {
                    outer = Some(ring);
                } else {
                    holes.push(ring);
                }
            }
            pieces.push(Profile::new(outer.ok_or(Refusal::Degenerate)?, holes)?);
        }
    }
    if pieces.is_empty() {
        return Err(Refusal::Empty);
    }
    Ok(pieces)
}

/// The members and panels of a system, by its type when it has one ([`crate::system`]).
pub fn system_parts(s: &SystemDef) -> Result<Vec<PartSolid>, Refusal> {
    crate::system::parts(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(openings: Vec<Opening>) -> PlanarDef {
        PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings, build_up: None }
    }

    /// The solid of a planar element in one piece.
    fn planar_solid(p: &PlanarDef) -> Result<Solid, Refusal> {
        planar_solids(p).map(|mut pieces| {
            assert_eq!(pieces.len(), 1, "one piece");
            pieces.remove(0)
        })
    }

    fn at(along: f64, sill: f64, width: f64, height: f64) -> Opening {
        Opening { along, sill, width, height, framing: None }
    }

    /// The volume of a 5 × 3 m wall 0.2 thick with `area` square metres of its elevation taken out.
    fn less(area: f64) -> f64 {
        (5.0 * 3.0 - area) * 0.2
    }

    #[test]
    fn a_slab_edge_with_a_bulge_bows_and_is_read_back() {
        // a 2 × 2 square whose east edge is a half circle outward (it runs north, bowing right)
        let outline = vec![DVec2::ZERO, DVec2::new(2.0, 0.0), DVec2::new(2.0, 2.0), DVec2::new(0.0, 2.0)];
        let ring = bowed(&outline, &[0.0, 1.0]);
        assert_eq!(ring.len(), 3 + 16);
        assert!(ring.iter().any(|p| (*p - DVec2::new(3.0, 1.0)).length() < 1e-9), "the arc reaches a metre out: {ring:?}");
        let form = PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: vec![0.0, 1.0], role: Default::default(), slope: None };
        let slab = PlanarDef { form, thickness: 0.2, openings: Vec::new(), build_up: None };
        let solid = planar_solid(&slab).unwrap();
        let area = 4.0 + std::f64::consts::PI / 2.0;
        assert!((solid.volume() - 0.2 * area).abs() < 0.2 * 0.02, "{}", solid.volume());
        // written and read, the bulges are kept; a slab without them writes none
        let text = serde_json::to_string(&slab).unwrap();
        assert_eq!(serde_json::from_str::<PlanarDef>(&text).unwrap(), slab);
        let straight = PlanarDef { form: PlanarForm::Slab { outline: vec![DVec2::ZERO, DVec2::X, DVec2::Y], holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None }, ..slab.clone() };
        assert!(!serde_json::to_string(&straight).unwrap().contains("bulges"));
    }

    #[test]
    fn a_wall_is_its_elevation_extruded_through_its_thickness() {
        let s = planar_solid(&wall(Vec::new())).unwrap();
        assert!((s.volume() - 5.0 * 3.0 * 0.2).abs() < 1e-12);
        let (lo, hi) = s.bounds();
        assert!((lo - DVec3::new(0.0, -0.1, 0.0)).length() < 1e-12 && (hi - DVec3::new(5.0, 0.1, 3.0)).length() < 1e-12, "{lo} {hi}");
    }

    #[test]
    fn openings_cut_the_wall_without_booleans() {
        let door = at(0.5, 0.0, 0.9, 2.1);
        let window = Opening { along: 2.0, sill: 0.9, width: 1.5, height: 1.2, framing: None };
        let s = planar_solid(&wall(vec![door, window])).unwrap();
        let expected = (5.0 * 3.0 - 0.9 * 2.1 - 1.5 * 1.2) * 0.2;
        assert!((s.volume() - expected).abs() < 1e-12, "{} vs {expected}", s.volume());
    }

    #[test]
    fn openings_that_take_nothing_touch_or_take_everything_are_refused() {
        assert_eq!(planar_solids(&wall(vec![at(5.5, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "past the end");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 3.2, 1.0, 1.0)])), Err(Refusal::OpeningOutside), "above the top");
        assert_eq!(planar_solids(&wall(vec![at(-2.0, 0.9, 2.0, 1.0)])), Err(Refusal::OpeningOutside), "before the start");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(1.5, 1.5, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap));
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 1.0, 1.0), at(2.0, 0.9, 1.0, 1.0)])), Err(Refusal::OpeningsOverlap), "touching");
        assert_eq!(planar_solids(&wall(vec![at(1.0, 0.9, 0.0, 1.0)])), Err(Refusal::NotPositive));
        assert_eq!(planar_solids(&wall(vec![at(-0.1, -0.1, 5.2, 3.2)])), Err(Refusal::Empty), "the whole wall");
    }

    #[test]
    fn an_opening_may_reach_the_top_or_an_end() {
        let volume = |o: Opening| planar_solid(&wall(vec![o])).unwrap().volume();
        let cases = [
            (at(0.0, 0.9, 1.0, 1.0), 1.0, "at the start"),
            (at(4.5, 0.9, 0.5, 1.0), 0.5, "at the end"),
            (at(1.0, 2.0, 1.0, 1.0), 1.0, "to the top"),
            (at(1.0, 2.5, 1.0, 1.0), 0.5, "past the top: what lies in the wall"),
            (at(1.0, -0.1, 1.0, 1.0), 0.9, "from below the base: a door"),
            (at(4.0, 2.0, 1.5, 1.5), 1.0, "in the top corner at the end"),
            (at(-0.3, 0.0, 1.3, 2.1), 2.1, "a door at the start, past it"),
        ];
        for (o, area, what) in cases {
            assert!((volume(o) - less(area)).abs() < 1e-12, "{what}: {} vs {}", volume(o), less(area));
        }
    }

    #[test]
    fn an_opening_through_the_whole_height_or_length_splits_the_wall() {
        // a door to the top: two pieces, from the start
        let pieces = planar_solids(&wall(vec![at(2.0, 0.0, 1.0, 3.0)])).unwrap();
        assert_eq!(pieces.len(), 2);
        let (lo, hi) = pieces[0].bounds();
        assert!(lo.x.abs() < 1e-12 && (hi.x - 2.0).abs() < 1e-12, "{lo} {hi}");
        let (lo, hi) = pieces[1].bounds();
        assert!((lo.x - 3.0).abs() < 1e-12 && (hi.x - 5.0).abs() < 1e-12, "{lo} {hi}");
        assert!(pieces.iter().all(|p| (p.volume() - 2.0 * 3.0 * 0.2).abs() < 1e-12));
        // a band through the whole length: below and above it, bottom up
        let pieces = planar_solids(&wall(vec![at(-0.5, 1.0, 6.0, 1.0)])).unwrap();
        assert_eq!(pieces.len(), 2);
        assert!((pieces[0].bounds().1.z - 1.0).abs() < 1e-12 && (pieces[1].bounds().0.z - 2.0).abs() < 1e-12);
        // with a window in one piece and a door to the top beside it
        let pieces = planar_solids(&wall(vec![at(0.5, 0.9, 1.0, 1.2), at(2.0, 0.0, 1.0, 3.0), at(3.5, 0.0, 0.9, 2.1)])).unwrap();
        let volume: f64 = pieces.iter().map(Solid::volume).sum();
        assert_eq!(pieces.len(), 2);
        assert!((volume - less(1.2 + 3.0 + 0.9 * 2.1)).abs() < 1e-12, "{volume}");
        // every layer of a layered wall is split alike
        let mut w = cavity_wall();
        w.openings = vec![at(2.0, 0.0, 1.0, 3.0)];
        let parts = planar_parts(&w).unwrap();
        assert_eq!(parts.len(), 3 * 2);
        let volume: f64 = parts.iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - 4.0 * 3.0 * 0.32).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn edges_within_the_tolerance_of_the_wall_or_each_other_are_taken_as_one() {
        // a window a micrometre from the start and one whose edge lies a nanometre beside the first's
        let near = 1e-7;
        let s = planar_solid(&wall(vec![at(near, 0.9, 1.0, 1.0), at(2.0, 0.9 + 1e-9, 1.0, 1.0)])).unwrap();
        // the first taken onto the start, so it is that much wider; the second onto the first's lines
        assert!((s.volume() - less(2.0 + near)).abs() < 1e-12, "{}", s.volume());
        assert_eq!(s.positions.len(), 2 * (8 + 4), "an outline of eight corners and a hole of four on either face: no slivers");
    }

    #[test]
    fn a_slab_hangs_under_its_top() {
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        // rising at 45° from its first edge, its top goes up as far as it goes in, and it is as
        // much longer as the slope is
        let pitched = PlanarDef { form: PlanarForm::Slab { outline: outline.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: SlabRole::Roof, slope: Some(Slope { edge: 0, angle: std::f64::consts::FRAC_PI_4 }) }, thickness: 0.25, openings: Vec::new(), build_up: None };
        let roof = planar_solid(&pitched).unwrap();
        let level = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline: outline.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None }).unwrap();
        assert!((roof.volume() - level.volume() * std::f64::consts::SQRT_2).abs() < 1e-9, "{} {}", roof.volume(), level.volume());
        let depth = outline.iter().map(|p| (*p - outline[0]).dot((outline[1] - outline[0]).normalize().perp()).abs()).fold(0.0, f64::max);
        let highest = roof.bounds().1.z;
        assert!((highest - (3.0 + depth)).abs() < 1e-9, "{highest} {depth}");
        let s = planar_solid(&PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.25, openings: Vec::new(), build_up: None }).unwrap();
        let (lo, hi) = s.bounds();
        assert_eq!((lo.z, hi.z), (2.75, 3.0));
        assert!((s.volume() - 4.0 * 3.0 * 0.25).abs() < 1e-12);
    }

    fn cavity_wall() -> PlanarDef {
        let layer = |material: &str, thickness, cavity| LayerDef { material: material.into(), thickness, cavity };
        let build_up = BuildUpDef {
            id: "wall.cavity-pir-360".into(),
            layers: vec![layer("brick", 0.1, false), layer("air", 0.04, true), layer("insulation-pir", 0.1, false), layer("sand-lime", 0.12, false)],
        };
        wall(Vec::new()).with_build_up(Some(build_up))
    }

    #[test]
    fn a_layered_wall_is_a_solid_per_layer_from_its_exterior_on_the_right() {
        let w = cavity_wall();
        assert!((w.thickness - 0.36).abs() < 1e-12, "the thickness follows the layers");
        let parts = planar_parts(&w).unwrap();
        let materials: Vec<&str> = parts.iter().map(|(m, _)| m.as_deref().unwrap()).collect();
        assert_eq!(materials, ["brick", "insulation-pir", "sand-lime"], "the cavity has no solid");
        // the wall runs along +x, so its right (exterior) side is -y; it is centred on its axis
        let (lo, hi) = parts[0].1.bounds();
        assert!((lo.y + 0.18).abs() < 1e-12 && (hi.y + 0.08).abs() < 1e-12, "the brick outside: {lo} {hi}");
        let (lo, hi) = parts[2].1.bounds();
        assert!((lo.y - 0.06).abs() < 1e-12 && (hi.y - 0.18).abs() < 1e-12, "the sand-lime inside: {lo} {hi}");
        let volume: f64 = parts.iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - 5.0 * 3.0 * 0.32).abs() < 1e-9, "all but the 40 mm cavity: {volume}");
        assert!((planar_solid(&w).unwrap().volume() - 5.0 * 3.0 * 0.36).abs() < 1e-9, "one body through all layers");
    }

    #[test]
    fn openings_go_through_every_layer() {
        let mut w = cavity_wall();
        w.openings = vec![Opening { along: 1.0, sill: 0.9, width: 1.2, height: 1.2, framing: None }];
        let volume: f64 = planar_parts(&w).unwrap().iter().map(|(_, s)| s.volume()).sum();
        assert!((volume - (5.0 * 3.0 - 1.2 * 1.2) * 0.32).abs() < 1e-9, "{volume}");
    }

    #[test]
    fn a_layered_slab_has_its_first_layer_on_top() {
        let layer = |material: &str, thickness| LayerDef { material: material.into(), thickness, cavity: false };
        let outline = vec![DVec2::ZERO, DVec2::new(4.0, 0.0), DVec2::new(4.0, 3.0), DVec2::new(0.0, 3.0)];
        let slab = PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: Default::default(), slope: None }, thickness: 0.0, openings: Vec::new(), build_up: None }
            .with_build_up(Some(BuildUpDef { id: "floor".into(), layers: vec![layer("screed", 0.05), layer("concrete", 0.2), layer("insulation", 0.1)] }));
        let parts = planar_parts(&slab).unwrap();
        let z = |i: usize| {
            let (lo, hi) = parts[i].1.bounds();
            (lo.z, hi.z)
        };
        let near = |(a, b): (f64, f64), (c, d): (f64, f64)| (a - c).abs() < 1e-12 && (b - d).abs() < 1e-12;
        assert!(near(z(0), (2.95, 3.0)), "the screed on top: {:?}", z(0));
        assert!(near(z(1), (2.75, 2.95)), "{:?}", z(1));
        assert!(near(z(2), (2.65, 2.75)), "{:?}", z(2));
    }

    #[test]
    fn a_build_up_of_cavities_or_empty_layers_is_refused() {
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers.iter_mut().for_each(|l| l.cavity = true);
        assert_eq!(planar_parts(&w), Err(Refusal::NoLayers));
        assert_eq!(planar_solid(&w), Err(Refusal::NoLayers));
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers[1].thickness = 0.0;
        assert_eq!(planar_parts(&w), Err(Refusal::NotPositive));
        let mut w = cavity_wall();
        w.build_up.as_mut().unwrap().layers.clear();
        assert_eq!(planar_parts(&w), Err(Refusal::NoLayers), "no layers at all");
    }

    #[test]
    fn a_build_up_comes_from_the_library_with_its_cavities() {
        let library = bim_library::Library::starter();
        let b = BuildUpDef::from_library(&library, "wall.cavity-pir-360").unwrap();
        assert_eq!(b, cavity_wall().build_up.unwrap());
        assert!(BuildUpDef::from_library(&library, "wall.nothing").is_none());
    }

    #[test]
    fn a_layered_definition_round_trips_and_an_old_one_still_reads() {
        let w = cavity_wall();
        let json = serde_json::to_string(&w).unwrap();
        assert!(json.contains("\"cavity\":true") && json.matches("cavity").count() == 2, "{json}");
        assert_eq!(serde_json::from_str::<PlanarDef>(&json).unwrap(), w);
        let old = r#"{"form":{"wall":{"start":[0.0,0.0],"end":[5.0,0.0],"base":0.0,"height":3.0}},"thickness":0.2}"#;
        assert_eq!(serde_json::from_str::<PlanarDef>(old).unwrap(), wall(Vec::new()));
    }

    #[test]
    fn linear_elements_have_their_class_by_direction() {
        assert_eq!(linear_class(DVec3::new(5.0, 0.0, 0.0)), "IfcBeam");
        assert_eq!(linear_class(DVec3::new(0.0, 0.0, 3.0)), "IfcColumn");
        assert_eq!(linear_class(DVec3::new(3.0, 0.0, 3.0)), "IfcMember");
    }

    #[test]
    fn a_beam_has_its_profile_upright() {
        let l = LinearDef { start: DVec3::ZERO, end: DVec3::new(6.0, 0.0, 0.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0, role: Default::default() };
        let s = linear_solid(&l, None, None).unwrap();
        let (lo, hi) = s.bounds();
        assert!((hi - lo - DVec3::new(6.0, 0.2, 0.4)).length() < 1e-12, "{}", hi - lo);
        let column = LinearDef { start: DVec3::ZERO, end: DVec3::new(0.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.3, height: 0.5 }, roll: 0.0, role: Default::default() };
        let (lo, hi) = linear_solid(&column, None, None).unwrap().bounds();
        assert!((hi - lo - DVec3::new(0.3, 0.5, 3.0)).length() < 1e-12, "{}", hi - lo);
    }

    #[test]
    fn a_system_fills_its_domain_with_members_and_panels() {
        let s = SystemDef { start: DVec2::ZERO, end: DVec2::new(3.0, 0.0), base: 0.0, height: 3.0, spacing: DVec2::new(1.5, 1.5), member: DVec2::new(0.05, 0.15), panel: 0.03, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), grid: None, division: None };
        let parts = system_parts(&s).unwrap();
        let members = parts.iter().filter(|p| p.class == "IfcMember").count();
        let panels = parts.iter().filter(|p| p.class == "IfcPlate").count();
        // 3 mullions, 2 bays × 3 transoms, 2 × 2 panels
        assert_eq!((members, panels), (3 + 6, 4));
        let glass: f64 = parts.iter().filter(|p| p.class == "IfcPlate").map(|p| p.solid.volume()).sum();
        assert!((glass - 4.0 * 1.45 * 1.45 * 0.03).abs() < 1e-12);
        let tight = SystemDef { spacing: DVec2::new(0.04, 1.0), ..s };
        assert_eq!(system_parts(&tight), Err(Refusal::NotPositive));
    }

    #[test]
    fn an_element_lives_in_the_tile_of_its_placement() {
        let grid = TileGrid::default();
        let shape = Shape::Point(PointDef { at: DVec3::new(1500.25, -20.5, 1.0), rotation: 0.0, size: DVec3::ONE, opening: None, role: Default::default() });
        let mut e = Element::in_project(&grid, "Box", None, shape.clone());
        assert_eq!(e.tile, TileKey { x: 1, y: -1 });
        assert_eq!(e.shape_in_project(&grid), shape);
        e.translate(&grid, DVec3::new(-600.0, 0.0, 0.0));
        assert_eq!(e.tile, TileKey { x: 0, y: -1 }, "it moved to the tile that holds it now");
        let Shape::Point(p) = &e.shape else { unreachable!() };
        assert_eq!(p.at, DVec3::new(900.25, 979.5, 1.0));
    }

    #[test]
    fn definitions_round_trip_through_json_exactly() {
        let grid = TileGrid::default();
        let shape = Shape::Planar(PlanarDef {
            form: PlanarForm::Wall { start: DVec2::new(0.1, 0.2), end: DVec2::new(7.3, 0.2), base: 0.0, height: 2.7 },
            thickness: 0.214,
            openings: vec![Opening { along: 1.0, sill: 0.0, width: 0.93, height: 2.315, framing: None }],
            build_up: None,
        });
        let e = Element::in_project(&grid, "Wall 1", Some("level-path".into()), shape);
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
    }

    #[test]
    fn an_element_keeps_its_top_when_reshaped_and_reads_without_one() {
        let grid = TileGrid::default();
        let wall = |x: f64| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(x, 0.0), end: DVec2::new(x + 4.0, 0.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let mut e = Element::in_project(&grid, "Wall 1", Some("ground".into()), wall(0.0));
        assert_eq!(e.top, None);
        // a file of before tops reads without one
        let old = serde_json::to_string(&e).unwrap();
        assert!(!old.contains("top\""), "{old}");
        assert_eq!(serde_json::from_str::<Element>(&old).unwrap().top, None);
        e.id = ElementId(7);
        e.top = Some(Top { level: "first".into(), offset: -0.3 });
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
        // reshaped: the id, name, level and top stay, the shape is the new one
        let moved = e.reshaped(&grid, wall(5.0));
        assert_eq!((moved.id, moved.name.as_str(), moved.level.as_deref(), moved.top.clone()), (e.id, "Wall 1", Some("ground"), e.top.clone()));
        assert_eq!(moved.shape_in_project(&grid), wall(5.0));
    }

    #[test]
    fn an_element_keeps_its_grade_when_reshaped_and_reads_without_one() {
        let grid = TileGrid::default();
        let beam = |x: f64| Shape::Linear(LinearDef { start: DVec3::new(x, 0.0, 3.0), end: DVec3::new(x + 4.0, 0.0, 3.0), profile: ProfileDef::Rectangle { width: 0.2, height: 0.4 }, roll: 0.0, role: Default::default() });
        let mut e = Element::in_project(&grid, "Beam 1", None, beam(0.0));
        let old = serde_json::to_string(&e).unwrap();
        assert!(!old.contains("grade"), "{old}");
        assert_eq!(serde_json::from_str::<Element>(&old).unwrap().grade, None);
        e.grade = Some("S355".into());
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<Element>(&json).unwrap(), e, "{json}");
        assert_eq!(e.reshaped(&grid, beam(5.0)).grade.as_deref(), Some("S355"));
    }

    #[test]
    fn a_stair_drawn_as_a_line_has_a_tread_on_every_step_but_the_last() {
        let stair = |path: Vec<DVec2>, risers: usize| PlanarDef { form: PlanarForm::Stair { base: 0.0, height: 3.0, risers, shape: StairShape::Run { path, width: 1.0 }, railing: false }, thickness: 0.15, openings: Vec::new(), build_up: None };
        // 16 steps of 187.5 mm over 3.75 m: 15 treads of 250 mm
        let straight = stair(vec![DVec2::ZERO, DVec2::new(3.75, 0.0)], 16);
        let solids = planar_solids(&straight).unwrap();
        assert_eq!(solids.len(), 15);
        let (lo, hi) = solids[0].bounds();
        assert!((lo - DVec3::new(0.0, -0.5, 0.0)).length() < 1e-9 && (hi - DVec3::new(0.25, 0.5, 0.1875)).length() < 1e-9, "{lo} {hi}");
        let (lo, hi) = solids[14].bounds();
        // the top tread is one step under the floor it reaches, and 0.15 thick under the step before it
        assert!((hi.z - (3.0 - 0.1875)).abs() < 1e-9 && (hi.x - 3.75).abs() < 1e-9 && (lo.z - (3.0 - 2.0 * 0.1875 - 0.15)).abs() < 1e-9, "{lo} {hi}");
        assert!(solids.iter().all(|s| s.check().is_ok()));
        // a quarter turn: a landing 1 by 1 on the corner, a tread among the others
        let turned = stair(vec![DVec2::ZERO, DVec2::new(3.0, 0.0), DVec2::new(3.0, 3.0)], 16);
        let PlanarForm::Stair { risers, shape, .. } = &turned.form else { panic!() };
        let treads = stair_treads(*risers, shape).unwrap();
        assert_eq!(treads.len(), 15);
        assert_eq!(treads.iter().map(|t| t.1).collect::<Vec<_>>(), (1..=15).collect::<Vec<_>>());
        let area = |ring: &[DVec2]| (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f64>() / 2.0;
        let landing = treads.iter().find(|t| (area(&t.0) - 1.0).abs() < 1e-9).expect("a landing of 1 by 1");
        assert!(landing.0.iter().all(|p| (2.5 - 1e-9..=3.5 + 1e-9).contains(&p.x) && (-0.5 - 1e-9..=0.5 + 1e-9).contains(&p.y)), "{:?}", landing.0);
        assert_eq!(planar_solids(&turned).unwrap().len(), 15);
        // too few steps for its flights and landing, or none at all, is refused
        assert_eq!(planar_solids(&stair(vec![DVec2::ZERO, DVec2::new(3.0, 0.0), DVec2::new(3.0, 3.0)], 3)).err(), Some(Refusal::NotPositive));
        assert_eq!(planar_solids(&stair(vec![DVec2::ZERO, DVec2::new(3.0, 0.0)], 1)).err(), Some(Refusal::NotPositive));
        // with a railing: on each side a baluster on every tread and a handrail between them, but
        // none over the landing
        let railed = |p: &PlanarDef| {
            let mut p = p.clone();
            if let PlanarForm::Stair { railing, .. } = &mut p.form {
                *railing = true;
            }
            planar_solids(&p).unwrap()
        };
        let with = railed(&straight);
        assert_eq!(with.len(), 15 + 2 * 15 + 2 * 14);
        assert!(with.iter().all(|s| s.check().is_ok()));
        let (_, hi) = with.iter().map(|s| s.bounds()).fold((DVec3::MAX, DVec3::MIN), |(lo, hi), b| (lo.min(b.0), hi.max(b.1)));
        assert!((hi.z - (3.0 - 0.1875 + RAILING_HEIGHT)).abs() < 0.05, "{hi}");
        assert_eq!(railed(&turned).len(), 15 + 2 * 14 + 2 * 12);
        // it is an IfcStair, moves with its points and is kept in the file
        let e = Element::in_project(&TileGrid::default(), "Stair", None, Shape::Planar(turned.clone()));
        assert_eq!(e.class(), "IfcStair");
        assert_eq!(serde_json::from_str::<PlanarDef>(&serde_json::to_string(&turned).unwrap()).unwrap(), turned);
    }

    #[test]
    fn a_stair_in_an_outline_is_the_outline_cut_into_treads_across_its_way() {
        // a tapering footprint: 2 wide at its foot, 1 wide at its head, 3 long; 11 steps: 10 treads of 0.3
        let outline = vec![DVec2::new(0.0, -1.0), DVec2::new(3.0, -0.5), DVec2::new(3.0, 0.5), DVec2::new(0.0, 1.0)];
        let shape = StairShape::Outline { outline, foot: DVec2::ZERO, head: DVec2::new(3.0, 0.0) };
        let treads = stair_treads(11, &shape).unwrap();
        assert_eq!(treads.len(), 10);
        let area = |ring: &[DVec2]| (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f64>() / 2.0;
        // together they are the footprint (4.5 m2), each narrower than the one before
        assert!((treads.iter().map(|t| area(&t.0)).sum::<f64>() - 4.5).abs() < 1e-9);
        assert!(treads.windows(2).all(|t| area(&t[1].0) < area(&t[0].0)));
        let p = PlanarDef { form: PlanarForm::Stair { base: 1.0, height: 2.2, risers: 11, shape, railing: true }, thickness: 0.1, openings: Vec::new(), build_up: None };
        let solids = planar_solids(&p).unwrap();
        assert!(solids.iter().all(|s| s.check().is_ok()));
        let (lo, hi) = solids[9].bounds();
        assert!((hi.z - (1.0 + 2.0)).abs() < 1e-9 && (lo.x - 2.7).abs() < 1e-9, "{lo} {hi}");
    }

    #[test]
    fn a_roof_by_footprint_has_a_plane_from_every_eave() {
        let p = DVec2::new;
        let area = |ring: &[DVec2]| (0..ring.len()).map(|i| ring[i].perp_dot(ring[(i + 1) % ring.len()])).sum::<f64>() / 2.0;
        let rect = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 6.0), p(0.0, 6.0)];
        let deg = |d: f64| d.to_radians();
        // every edge an eave at 40 degrees: a hip roof, two trapezia and two triangles
        let hip = roof_planes(&rect, &[deg(40.0); 4]).unwrap();
        let mut sizes: Vec<f64> = hip.iter().map(|(ring, _)| area(ring)).collect();
        sizes.sort_by(f64::total_cmp);
        assert_eq!(hip.len(), 4);
        assert!((sizes[0] - 9.0).abs() < 1e-9 && (sizes[1] - 9.0).abs() < 1e-9 && (sizes[2] - 21.0).abs() < 1e-9 && (sizes[3] - 21.0).abs() < 1e-9, "{sizes:?}");
        // each plane's eave is the edge of the outline it rises from, and it makes a slab that holds
        for (ring, slope) in &hip {
            let slope = slope.unwrap();
            let (a, b) = (ring[slope.edge], ring[(slope.edge + 1) % ring.len()]);
            assert!(rect.iter().zip(rect.iter().cycle().skip(1)).any(|(c, d)| (a.distance(*c) < 1e-9 && b.distance(*d) < 1e-9)), "{ring:?} {slope:?}");
            let slab = PlanarDef { form: PlanarForm::Slab { outline: ring.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: SlabRole::Roof, slope: Some(slope) }, thickness: 0.2, openings: Vec::new(), build_up: None };
            assert!(planar_solids(&slab).unwrap().iter().all(|s| s.check().is_ok()));
        }
        // the short sides without an angle: gables, and two planes that meet on the ridge
        let gable = roof_planes(&rect, &[deg(40.0), 0.0, deg(40.0), 0.0]).unwrap();
        assert_eq!(gable.len(), 2);
        assert!(gable.iter().all(|(ring, _)| (area(ring) - 30.0).abs() < 1e-9));
        // one eave: a single pitch over the whole outline; none: flat
        let mono = roof_planes(&rect, &[deg(15.0), 0.0, 0.0, 0.0]).unwrap();
        assert_eq!((mono.len(), mono[0].1), (1, Some(Slope { edge: 0, angle: deg(15.0) })));
        assert_eq!(roof_planes(&rect, &[]).unwrap(), vec![(rect.to_vec(), None)]);
        // two different angles: the ridge lies nearer the steeper eave
        let uneven = roof_planes(&rect, &[deg(30.0), 0.0, deg(60.0), 0.0]).unwrap();
        assert!(area(&uneven[0].0) > area(&uneven[1].0) * 2.9, "{uneven:?}");
        // an L is roofed in its two wings, 10 by 4 and 4 by 10: a hip roof each, and flat it is one
        let ell = [p(0.0, 0.0), p(10.0, 0.0), p(10.0, 4.0), p(4.0, 4.0), p(4.0, 10.0), p(0.0, 10.0)];
        let wings = roof_planes(&ell, &[deg(40.0); 6]).unwrap();
        assert_eq!(wings.len(), 8);
        let mut sizes: Vec<f64> = wings.iter().map(|(ring, _)| (area(ring) * 1e6).round() / 1e6).collect();
        sizes.sort_by(f64::total_cmp);
        assert_eq!(sizes, [4.0, 4.0, 4.0, 4.0, 16.0, 16.0, 16.0, 16.0]);
        assert_eq!(roof_planes(&ell, &[]).unwrap().len(), 1);
        // every plane of every wing is a slab the kernel makes
        for (ring, slope) in &wings {
            let slab = PlanarDef { form: PlanarForm::Slab { outline: ring.clone(), holes: Vec::new(), top: 3.0, bulges: Vec::new(), role: SlabRole::Roof, slope: *slope }, thickness: 0.2, openings: Vec::new(), build_up: None };
            assert!(planar_solids(&slab).is_ok(), "{ring:?} {slope:?}");
        }
        // a U has four inner cuts and three wings' worth of planes, none lost
        let you = [p(0.0, 0.0), p(12.0, 0.0), p(12.0, 10.0), p(8.0, 10.0), p(8.0, 4.0), p(4.0, 4.0), p(4.0, 10.0), p(0.0, 10.0)];
        let wings = roof_planes(&you, &[deg(30.0); 8]).unwrap();
        let covered = |q: DVec2| wings.iter().any(|(ring, _)| (0..ring.len()).all(|k| (ring[(k + 1) % ring.len()] - ring[k]).perp_dot(q - ring[k]) >= -1e-9));
        assert!([p(1.0, 9.0), p(11.0, 9.0), p(6.0, 1.0), p(2.0, 2.0), p(10.0, 5.0)].into_iter().all(covered));
    }
}
