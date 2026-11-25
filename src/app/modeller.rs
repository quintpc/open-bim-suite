//! The modeller: the five base elements of `bim-core` (point, linear, planar, system and
//! connection) placed with tools on a work plane, and shown as a model of their own.
//!
//! The elements are kept in [`Modeller::model`] as definitions in tile frames. After every change
//! they are evaluated again and shown as one viewer model (a layer per IFC class), which is an
//! entry of `App::models`: selected, hidden, measured and cut like any other model. Its display
//! coordinates are those of the scene it is modelled in.

use super::{App, ModelEntry};
use crate::model::{Layer, Model};
use crate::selection::Pickable;
use bim_core::element::PlanarForm;
use super::placement::Settings;
use bim_core::{ConnectionDef, ConnectionKind, Element, ElementId, LinearDef, Mesh, PlanarDef, PointDef, ProfileDef, Refusal, Shape, SystemDef};
use glam::{DVec2, DVec3, Vec3};
use openaec_ui::Language;
use std::path::PathBuf;
use std::sync::Arc;

pub mod grids;
pub mod browser;
pub mod cursor3d;
pub mod curtain;
pub mod dim_refs;
pub mod explorer;
pub mod grab;
pub(super) mod file_types;
pub mod key_actions;
pub mod keys;
pub mod select;
pub mod services;
pub mod levels;
pub(super) mod shape_handles;
pub mod slab_handles;
pub mod snaps;
pub mod spaces;
mod pile_symbols;
pub mod structure;
pub mod temp_dims;
pub mod sketch;
pub mod tops;
pub mod typed_length;
pub mod typed_move;
#[cfg(test)]
mod showcase;

/// A tool that places a base element with clicks on the work plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModelTool {
    Point,
    Column,
    Beam,
    Wall,
    Slab,
    System,
    /// A door in the wall clicked: an opening down to the floor.
    Door,
    /// A window in the wall clicked: an opening above a sill.
    Window,
    /// Runs of building services: a rectangular and a round air duct, a pipe and a cable tray.
    Duct,
    RoundDuct,
    Pipe,
    CableTray,
    /// A terminal or an appliance of a building service: a radiator, a boiler, a wash basin.
    Appliance,
    /// A stair: two clicks draw a straight one along the line between them; its sketch draws one
    /// in an outline or along a line with landings (see `sketch`).
    Stair,
}

impl ModelTool {
    /// The tools that place an element of their own.
    pub const ALL: [ModelTool; 7] = [ModelTool::Wall, ModelTool::Slab, ModelTool::Column, ModelTool::Beam, ModelTool::System, ModelTool::Point, ModelTool::Stair];
    /// The tools that make an opening in a wall.
    pub const OPENINGS: [ModelTool; 2] = [ModelTool::Door, ModelTool::Window];
    /// The tools that draw a run of a building service.
    pub const SERVICES: [ModelTool; 4] = [ModelTool::Duct, ModelTool::RoundDuct, ModelTool::Pipe, ModelTool::CableTray];
    /// The tools that place what the runs of the services serve.
    pub const APPLIANCES: [ModelTool; 1] = [ModelTool::Appliance];

    /// The role of the runs it draws; none for a tool of the building itself.
    pub fn service(self) -> Option<bim_core::LinearRole> {
        match self {
            ModelTool::Duct | ModelTool::RoundDuct => Some(bim_core::LinearRole::Duct),
            ModelTool::Pipe => Some(bim_core::LinearRole::Pipe),
            ModelTool::CableTray => Some(bim_core::LinearRole::CableTray),
            _ => None,
        }
    }

    /// Whether it makes an opening in a wall rather than an element.
    pub fn opens(self) -> bool {
        matches!(self, ModelTool::Door | ModelTool::Window)
    }

    /// Its command id.
    pub fn id(self) -> &'static str {
        match self {
            ModelTool::Point => "model.point",
            ModelTool::Column => "model.column",
            ModelTool::Beam => "model.beam",
            ModelTool::Wall => "model.wall",
            ModelTool::Slab => "model.slab",
            ModelTool::System => "model.system",
            ModelTool::Door => "model.door",
            ModelTool::Window => "model.window",
            ModelTool::Duct => "mep.duct",
            ModelTool::RoundDuct => "mep.round-duct",
            ModelTool::Pipe => "mep.pipe",
            ModelTool::CableTray => "mep.cable-tray",
            ModelTool::Appliance => "mep.appliance",
            ModelTool::Stair => "model.stair",
        }
    }

    /// Its name on the ribbon, which is also the name of what it places.
    pub fn label(self, l: Language) -> &'static str {
        match self {
            ModelTool::Point => l.text("Object", "Object"),
            ModelTool::Column => l.text("Kolom", "Column"),
            ModelTool::Beam => l.text("Balk", "Beam"),
            ModelTool::Wall => l.text("Wand", "Wall"),
            ModelTool::Slab => l.text("Vloer", "Slab"),
            ModelTool::System => l.text("Vliesgevel", "Curtain wall"),
            ModelTool::Door => l.text("Deur", "Door"),
            ModelTool::Window => l.text("Raam", "Window"),
            ModelTool::Duct => l.text("Kanaal", "Duct"),
            ModelTool::RoundDuct => l.text("Rond kanaal", "Round duct"),
            ModelTool::Pipe => l.text("Leiding", "Pipe"),
            ModelTool::CableTray => l.text("Kabelgoot", "Cable tray"),
            ModelTool::Appliance => l.text("Toestel", "Appliance"),
            ModelTool::Stair => l.text("Trap", "Stair"),
        }
    }

    /// Its ribbon icon.
    pub fn icon(self) -> &'static str {
        match self {
            ModelTool::Point | ModelTool::Appliance => "draw.coordinate",
            ModelTool::Column => "pile.columns",
            ModelTool::Beam => "draw.line",
            ModelTool::Wall => "construction.types",
            ModelTool::Slab => "draw.filled-area",
            ModelTool::Stair => "draw.polyline",
            ModelTool::System => "curtain.new",
            ModelTool::Door => "frame.new-door",
            ModelTool::Window => "frame.new-turn-tilt",
            ModelTool::Duct | ModelTool::CableTray => "draw.l-shape",
            ModelTool::RoundDuct | ModelTool::Pipe => "draw.line",
        }
    }

    /// The base element it places.
    pub fn base(self) -> bim_core::BaseKind {
        use bim_core::BaseKind;
        match self {
            ModelTool::Point | ModelTool::Appliance => BaseKind::Point,
            ModelTool::Column | ModelTool::Beam | ModelTool::Duct | ModelTool::RoundDuct | ModelTool::Pipe | ModelTool::CableTray => BaseKind::Linear,
            ModelTool::Wall | ModelTool::Slab | ModelTool::Stair | ModelTool::Door | ModelTool::Window => BaseKind::Planar,
            ModelTool::System => BaseKind::System,
        }
    }

    /// The clicks it takes.
    pub(super) fn clicks(self) -> usize {
        match self {
            ModelTool::Point | ModelTool::Appliance | ModelTool::Column | ModelTool::Door | ModelTool::Window => 1,
            ModelTool::Beam | ModelTool::Wall | ModelTool::Slab | ModelTool::Stair | ModelTool::System | ModelTool::Duct | ModelTool::RoundDuct | ModelTool::Pipe | ModelTool::CableTray => 2,
        }
    }

    /// Whether the next element can start where the last one ended (the strip's Chain).
    pub(super) fn chains(self) -> bool {
        matches!(self, ModelTool::Wall | ModelTool::Beam | ModelTool::System) || self.service().is_some()
    }

    /// What to click next, `placed` points in.
    pub fn hint(self, l: Language, placed: usize) -> &'static str {
        match (self, placed) {
            (ModelTool::Point, _) => l.text("Klik waar het object staat · Esc: stoppen", "Click where the object stands · Esc: stop"),
            (ModelTool::Appliance, _) => l.text("Klik waar het toestel staat · Esc: stoppen", "Click where the appliance stands · Esc: stop"),
            (ModelTool::Column, _) => l.text("Klik waar de kolom staat · Esc: stoppen", "Click where the column stands · Esc: stop"),
            (ModelTool::Door, _) => l.text("Klik op een wand waar de deur komt · Esc: stoppen", "Click a wall where the door goes · Esc: stop"),
            (ModelTool::Window, _) => l.text("Klik op een wand waar het raam komt · Esc: stoppen", "Click a wall where the window goes · Esc: stop"),
            (ModelTool::Stair, 0) => l.text("Klik de voet van de trap · Esc: stoppen", "Click the foot of the stair · Esc: stop"),
            (ModelTool::Stair, _) => l.text("Klik de richting waarin de trap omhoog loopt · Esc: opnieuw", "Click the way the stair climbs · Esc: start again"),
            (ModelTool::Slab, 0) => l.text("Klik een hoek van de vloer · Esc: stoppen", "Click a corner of the slab · Esc: stop"),
            (ModelTool::Slab, _) => l.text("Klik de andere hoek · Esc: opnieuw", "Click the opposite corner · Esc: start again"),
            (_, 0) => l.text("Klik het beginpunt · Esc: stoppen", "Click the start point · Esc: stop"),
            (_, _) => l.text("Klik het eindpunt · Esc: opnieuw", "Click the end point · Esc: start again"),
        }
    }
}

/// Sizes the tools give what they place (metres).
pub(super) const WALL_THICKNESS: f64 = 0.2;
pub(super) const SLAB_THICKNESS: f64 = 0.25;
pub(super) const COLUMN: f64 = 0.3;
pub(super) const BEAM: (f64, f64) = (0.2, 0.4);
/// A rectangular duct's width and height, a round duct's and a pipe's diameter, a cable tray's
/// width and height, and how high above its level the centre of a run lies.
pub(super) const DUCT: (f64, f64) = (0.4, 0.2);
pub(super) const ROUND_DUCT: f64 = 0.25;
pub(super) const PIPE: f64 = 0.054;
pub(super) const CABLE_TRAY: (f64, f64) = (0.3, 0.06);
pub(super) const SERVICE_HEIGHT: f64 = 2.6;
pub(super) const OBJECT: DVec3 = DVec3::new(0.6, 0.6, 0.9);
const SYSTEM_SPACING: DVec2 = DVec2::new(1.5, 1.5);
const SYSTEM_MEMBER: DVec2 = DVec2::new(0.05, 0.15);
const SYSTEM_PANEL: f64 = 0.02;
/// The height of what stands on the work plane when no level lies above it.
const STOREY: f64 = 3.0;
/// Clicks within this of a point of an element (logical pixels) take that point.
pub(super) const SNAP_PIXELS: f32 = 12.0;
/// A second point within this angle of the first point's horizontal or vertical is put on it.
const ORTHO_DEGREES: f64 = 4.0;
/// Clicks off every snap are rounded to this (metres).
const ROUND: f64 = 0.01;

/// The modeller's state.
#[derive(Debug, Clone, Default)]
pub struct Modeller {
    /// The modelled elements.
    pub model: bim_core::Model,
    pub rebar_edit: Option<super::reinforcement::Editor>,
    /// The tool placing elements, and the name its elements get (in the interface language).
    pub tool: Option<(ModelTool, &'static str)>,
    /// The branches of the model explorer that are open, by their keys (see `explorer`).
    pub explorer_open: std::collections::HashSet<String>,
    /// The offset of a top being typed in the properties: the element and the text (see `tops`).
    pub top_offset: Option<(ElementId, String)>,
    /// The level being renamed or moved in the Levels tab (see `levels`).
    pub level_edit: Option<levels::LevelEdit>,
    /// The level worked on when no floor plan is shown (its path): PageUp and PageDown step it.
    pub level: Option<String>,
    /// The point a click stands for when it was given in metres and not by the cursor
    /// (see `place_model_point`).
    pub exact: Option<DVec3>,
    /// The regions the walls of each level close in, as last found: the walls they were found
    /// for and the regions (see `spaces`).
    pub regions: std::cell::RefCell<Vec<(Vec<spaces::Bound>, Vec<Vec<DVec2>>)>>,
    /// What was shown of the model the last time: a change makes only what it touches again.
    pub kept: Kept,
    /// The Grid tool is on: two clicks draw a grid line (see `grids`).
    pub grid_tool: bool,
    /// The reinforcement tool that is on (see `rebar`), and the element its first click was on:
    /// that click's place is the first of `points`.
    pub rebar_tool: Option<super::rebar::RebarTool>,
    pub rebar_on: Option<ElementId>,
    /// Where the stretch or the line under way would end with a click where the cursor is
    /// (see `rebar::App::rebar_place`): the preview runs to it.
    pub rebar_at: Option<DVec3>,
    /// The points clicked so far for the tool (scene coordinates).
    pub points: Vec<DVec3>,
    /// The viewer model that shows the elements (its id), an entry of `App::models`.
    pub shown: Option<u64>,
    /// Why the last step was refused.
    pub refusal: Option<Refusal>,
    /// What joins made automatically are called, in the interface language.
    pub join_word: &'static str,
    /// The words fittings made automatically are named with: a bend, a tee, a transition.
    pub fitting_words: [&'static str; 3],
    /// What the top of a beam is called on a structural plan, in the interface language ("b.k.").
    pub top_word: &'static str,
    /// What the pile schedule and its columns are called, in the interface language (see
    /// `structure::PILE_WORDS`), and the name the schedule has now.
    pub pile_words: [&'static str; 10],
    pub pile_schedule: String,
    /// What the bar schedule, its columns and its kinds of bar are called, in the interface
    /// language (see `structure::BAR_WORDS`), and the name the schedule has now.
    pub bar_words: [&'static str; 19],
    pub bar_schedule: String,
    /// The same of the precast schedule (see `structure::PLANK_WORDS`).
    pub plank_words: [&'static str; 8],
    pub plank_schedule: String,
    /// What a space is called, in the interface language ("Space 1").
    pub space_word: &'static str,
    /// The length being typed for what a tool draws, in millimetres (see `typed_length`).
    pub length_typed: String,
    /// The kind of snap that took the cursor's point where it is, for its mark; and what that
    /// kind is called, in the interface language.
    pub snap_mark: Option<snaps::Snap>,
    pub snap_words: [&'static str; 8],
    /// The series of grid lines being typed beside the picked one (see `grids`); `None`: closed.
    pub grid_series: Option<String>,
    /// The tool on the grid of curtain walls that is on (see `curtain`).
    pub curtain: Option<curtain::CurtainTool>,
    /// The picked grid lines being dragged (see `grids::GridDrag`).
    pub grid_drag: Option<grids::GridDrag>,
    /// The angle typed for the turn under way (degrees).
    pub turn_typed: String,
    /// Which snaps are on (see `snaps`).
    pub snaps: snaps::Snaps,
    /// The names given to spaces, each at a point of its space (see `spaces`).
    pub rooms: Vec<spaces::Room>,
    /// The name being typed for a space: which one, and the text.
    pub space_typing: Option<(usize, String)>,
    /// Edit mode (Tab) on an element, or a drag of a selected element's grip.
    pub edit: Option<super::edit::Edit>,
    /// The value being typed in a temporary dimension's field: which one, and the text (see
    /// `temp_dims`).
    pub temp_dim: Option<(usize, String)>,
    /// The selected elements grabbed with G, following the cursor (see `grab`).
    pub grab: Option<grab::ObjectGrab>,
    /// The length being typed for the picked dimension (see `dim_refs`).
    pub dimension_value: Option<String>,
    /// The 3D cursor (see `cursor3d`).
    pub cursor3d: cursor3d::Cursor3d,
    /// The file the modelled project was opened from, when that file holds the project and
    /// nothing else: the model shown is that file, under its name.
    pub file: Option<PathBuf>,
    /// A grip of a selected floor being dragged (see `slab_handles`).
    pub slab_drag: Option<slab_handles::SlabDrag>,
    /// The sketch of a floor outline, open (see `sketch`).
    pub sketch: Option<sketch::Sketch>,
    /// An unfinished floor outline kept while its project is being inspected.
    pub parked_sketch: Option<sketch::Sketch>,
    /// Where the next click of the tool would place a point (snapped, scene coordinates), while
    /// the cursor is over the viewport: the preview runs to it.
    pub cursor: Option<DVec3>,
    /// Where a door or window would go, while the cursor is over a wall.
    pub aim: Option<super::openings::Aim>,
    /// The type and sizes each tool places with, typed in the strip over the view.
    pub placement: super::placement::Placement,
    /// The Modeller product is shown: there a selected wall of an opened file has grips, and a
    /// drag takes it over (see `take_over`).
    pub in_modeller: bool,
    /// The IFCX document the project became when it took over an opened file's first wall: the
    /// file's name with `.ifcx`, in its folder.
    pub project_file: Option<PathBuf>,
    /// A value being typed in the Definition section: the element, the field, the text.
    pub typing: Option<(ElementId, super::element_props::ElementField, String)>,
    /// The model before each step that changed it, to take the step back (`super::undo`).
    /// A move, copy, turn or mirror of the selected elements under way (`super::transform`).
    pub transform: Option<super::transform::Transform>,
    /// Offset is on (`super::drawn_edit`): the distance typed for it, in millimetres.
    pub offset: Option<String>,
    /// Trim, Extend, Fillet or Chamfer is on (`super::drawn_edit`).
    pub pair_edit: Option<super::drawn_edit::PairEdit>,
    pub history: crate::history::History<Step>,
    /// The project as its last step left it: what a change is measured against.
    pub base: Step,
    /// The walls taken over from opened files: the file (the id of its model as it was opened)
    /// and the wall's id in it (see `super::take_over`).
    pub taken: Arc<Vec<(u64, u64)>>,
    /// The opened files a wall was taken over from, by the id of their model as it was opened:
    /// that model, and the id of the model shown in its place (it without the walls taken over).
    pub files: std::collections::HashMap<u64, (Arc<Model>, u64)>,
    /// The elements were shown again since the history last looked: they may have changed.
    pub touched: bool,
}

/// The modelled project at one step of its history: the elements, the walls taken over from
/// opened files, and the IFCX document the project became when it took over its first wall.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Step {
    pub model: Arc<bim_core::Model>,
    pub taken: Arc<Vec<(u64, u64)>>,
    pub project_file: Option<PathBuf>,
    /// The levels of the document: a level moves with what stands on it and reaches up to it.
    pub levels: Arc<Vec<crate::document::Level>>,
    /// The project's library: a type is taken back with the elements that followed it, and a
    /// change of the library alone is a step too. `None` until the history has looked at it.
    pub library: Option<Arc<bim_library::Library>>,
    /// The project's drawing standard, kept as the library is.
    pub standard: Option<Arc<bim_library::drawing::Standard>>,
    /// The house style the project follows, kept as the library is.
    pub house: Option<Arc<bim_library::house::HouseStyle>>,
    /// What is drawn by hand on the views: a line drawn, moved or removed is a step too.
    pub drawn: Arc<crate::document::Drawn>,
}

impl App {
    /// Takes over the elements an IFCX file this viewer wrote defines (`model` was just loaded),
    /// and shows them; a definition that cannot be read is left out.
    pub(super) fn adopt_elements(&mut self, model: &Model) {
        if model.project.elements.is_empty() {
            return;
        }
        for definition in &model.project.elements {
            // a grid line comes under its own key, an element as it is
            if let Some(library) = definition.get(crate::export::ifcx::LIBRARY) {
                // the file's library: what this one lacks of it is added
                if let Ok(library) = serde_json::from_value::<bim_library::Library>(library.clone()) {
                    self.library.merge(&library);
                }
            } else if let Some(standards) = definition.get(crate::export::ifcx::STANDARDS) {
                self.adopt_standard(standards);
            } else if let Some(style) = definition.get(crate::export::ifcx::HOUSE) {
                self.adopt_house(style);
            } else if let Some(rooms) = definition.get(crate::export::ifcx::ROOMS) {
                self.modeller.rooms = serde_json::from_value(rooms.clone()).unwrap_or_default();
            } else if let Some(grid) = definition.get(crate::export::ifcx::GRID) {
                if let Ok(grid) = serde_json::from_value::<bim_core::GridDef>(grid.clone()) {
                    let _ = self.modeller.model.insert_grid(grid);
                }
            } else if let Ok(element) = serde_json::from_value::<Element>(definition.clone()) {
                self.modeller.model.insert(element);
            }
        }
        // a file of the project alone (no meshes of its own) is the modelled project: the model
        // shown takes its place and its name, not a second model beside it
        if model.layers.iter().all(|l| l.indices.is_empty()) {
            self.modeller.shown = Some(model.id);
            self.modeller.file = Some(model.path.clone());
        }
        self.show_modelled();
        // what a file gives is where its history starts
        self.settle_modelled();
        self.fit();
    }

    /// An empty model to model in, with a first level at 0 named `level`; the model shown before
    /// (if any) goes.
    pub(super) fn new_model(&mut self, level: &str) {
        let empty = Arc::new(Model::new(PathBuf::from("model.ifcx"), "IFCX", Vec::new()));
        self.set_model(empty.clone());
        self.modeller = Modeller { shown: Some(empty.id), in_modeller: self.modeller.in_modeller, ..Modeller::default() };
        let _ = self.document.add_level(level, 0.0);
        // the empty project with its first level is where its history starts
        self.settle_modelled();
        self.grid = true;
        // nothing to fit yet: look at the work plane from above and aside, 20 m around
        self.camera.target = Vec3::ZERO;
        self.camera.scene_radius = 20.0;
        self.camera.distance = 45.0;
    }

    /// Switches a tool on (its elements named `name`), or off.
    pub(super) fn set_model_tool(&mut self, tool: Option<(ModelTool, &'static str)>) {
        // the sketch of a new floor belongs to the Slab tool: it goes with it
        if self.modeller.sketch.as_ref().is_some_and(|s| s.new.is_some()) {
            self.modeller.sketch = None;
        }
        let same = tool.map(|t| t.0) == self.modeller.tool.map(|t| t.0);
        self.modeller.rebar_edit = None;
        self.modeller.tool = if same { None } else { tool };
        self.modeller.length_typed.clear();
        self.modeller.grid_tool = false;
        self.modeller.rebar_tool = None;
        self.modeller.rebar_on = None;
        self.modeller.rebar_at = None;
        self.modeller.points.clear();
        self.modeller.cursor = None;
        self.modeller.aim = None;
        self.modeller.placement.last_wall = None;
        // what was being typed in a field of the strip was the tool's before: the same field of
        // the next tool shows its own value, not that text
        self.modeller.placement.typing = None;
        self.modeller.refusal = None;
        if self.modeller.tool.is_some() {
            self.measure.tool = None;
            self.plane_picking = false;
        }
        // a fresh 3D cursor is where a wall, a beam or a system starts
        self.tool_from_cursor3d();
    }

    /// Esc while placing: drop the points clicked, else switch the tool off. False when no tool
    /// was on.
    pub(super) fn escape_model_tool(&mut self) -> bool {
        if self.modeller.tool.is_none() {
            return false;
        }
        self.modeller.refusal = None;
        if self.modeller.points.is_empty() {
            self.modeller.tool = None;
            self.modeller.cursor = None;
            self.modeller.aim = None;
        } else {
            self.modeller.points.clear();
            self.modeller.placement.last_wall = None;
        }
        true
    }

    /// The height of the work plane and how high a storey reaches from it: the level of the floor
    /// plan shown, else the level nearest 0; the storey up to the next level, else [`STOREY`].
    pub(super) fn work_plane(&self) -> (f64, f64, Option<String>) {
        let levels = self.document.levels();
        let shown = self.active_plan.as_deref().and_then(|p| self.document.plan(p)).and_then(|plan| self.document.level(&plan.level));
        // the floor plan shown, else the level stepped to, else the one nearest zero
        let stepped = self.modeller.level.as_deref().and_then(|p| self.document.level(p));
        let level = shown.or(stepped).or_else(|| levels.iter().min_by(|a, b| a.elevation.abs().total_cmp(&b.elevation.abs())));
        let Some(level) = level else { return (0.0, STOREY, None) };
        let above = levels.iter().map(|l| l.elevation).filter(|&z| z > level.elevation + 0.1).fold(f64::INFINITY, f64::min);
        let height = if above.is_finite() { above - level.elevation } else { STOREY };
        (level.elevation, height, Some(level.path.clone()))
    }

    /// The point of the work plane under the cursor (scene coordinates).
    pub(super) fn plane_point(&self, x: f32, y: f32, width: f32, height: f32, z: f64) -> Option<DVec3> {
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        let b = inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3();
        let d = b - a;
        if d.z.abs() < 1e-12 {
            return None;
        }
        let t = (z - a.z) / d.z;
        (t >= 0.0).then(|| a + d * t)
    }

    /// Where the cursor's ray meets the plane a drag runs in, through `through`: the level plane
    /// when the view looks down on it, else (an elevation, a section) the upright plane that
    /// faces the view. So what is dragged in a floor plan moves over the floor, and what is
    /// dragged in a section moves along the section and up and down.
    pub(super) fn drag_plane_point(&self, x: f32, y: f32, width: f32, height: f32, through: DVec3) -> Option<DVec3> {
        /// A view looks from the side when its rays rise or fall less than this (the sine of
        /// their slope).
        const FROM_THE_SIDE: f64 = 0.2;
        let inv = (self.camera.view_proj(width / height) * self.model_matrix()).inverse();
        let (nx, ny) = (2.0 * x / width - 1.0, 1.0 - 2.0 * y / height);
        let a = inv.project_point3(Vec3::new(nx, ny, 0.0)).as_dvec3();
        let d = inv.project_point3(Vec3::new(nx, ny, 1.0)).as_dvec3() - a;
        let along = d.try_normalize()?;
        let normal = if along.z.abs() >= FROM_THE_SIDE { DVec3::Z } else { DVec3::new(along.x, along.y, 0.0).try_normalize()? };
        let denom = d.dot(normal);
        if denom.abs() < 1e-12 {
            return None;
        }
        let t = (through - a).dot(normal) / denom;
        (t >= 0.0).then(|| a + d * t)
    }

    /// Where a scene point shows in the viewport (logical pixels), if in front of the camera.
    pub(crate) fn on_screen(&self, p: DVec3, width: f32, height: f32) -> Option<(f32, f32)> {
        let c = (self.camera.view_proj(width / height) * self.model_matrix()).project_point3(p.as_vec3());
        ((0.0..=1.0).contains(&c.z)).then(|| ((c.x + 1.0) / 2.0 * width, (1.0 - c.y) / 2.0 * height))
    }

    /// The points of the modelled elements a click snaps to (scene coordinates): ends, corners
    /// and placements.
    fn snap_points(&self) -> Vec<DVec3> {
        self.snap_points_except(ElementId(0))
    }

    /// The points a click snaps to, of every modelled element but `except` (ids start at 1).
    pub(super) fn snap_points_except(&self, except: ElementId) -> Vec<DVec3> {
        self.snap_marks_except(except).into_iter().map(|(p, _)| p).collect()
    }

    /// The points of [`App::snap_points_except`], each with the kind of snap it is.
    pub(super) fn snap_marks_except(&self, except: ElementId) -> Vec<(DVec3, snaps::Snap)> {
        use snaps::Snap;
        let grid = self.modeller.model.grid;
        // the 3D cursor, once placed, is a point to land on
        let mut points: Vec<(DVec3, Snap)> = self.cursor3d_snap().into_iter().map(|p| (p, Snap::End)).collect();
        let snaps = self.modeller.snaps;
        // the ends of a line when ends are on, its middle when middles are
        let mut line = |a: DVec3, b: DVec3| {
            if snaps.end {
                points.extend([(a, Snap::End), (b, Snap::End)]);
            }
            if snaps.middle {
                points.push(((a + b) / 2.0, Snap::Middle));
            }
        };
        let (mut corners, mut middles) = (Vec::new(), Vec::new());
        // a run of a building service lands on the open connectors of its kind and service
        if let Some((tool, _)) = self.modeller.tool {
            corners.extend(self.open_connectors(tool).into_iter().map(|c| c.at));
        }
        for e in self.modeller.model.iter().filter(|e| e.id != except) {
            match e.shape_in_project(&grid) {
                Shape::Point(p) => corners.push(p.at),
                Shape::Linear(l) => line(l.start, l.end),
                Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => line(start.extend(base), end.extend(base)),
                // a stair: the ends of its line, or the corners of its outline
                Shape::Planar(PlanarDef { form: PlanarForm::Stair { base, mut shape, .. }, .. }) => corners.extend(shape.points_mut().map(|p| p.extend(base))),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => {
                    let n = outline.len();
                    for k in 0..n {
                        // each corner once: as the start of its edge
                        corners.push(outline[k].extend(top));
                        middles.push(((outline[k] + outline[(k + 1) % n]) / 2.0).extend(top));
                    }
                }
                Shape::System(s) => line(s.start.extend(s.base), s.end.extend(s.base)),
                Shape::Connection(_) => {}
            }
        }
        if snaps.end {
            points.extend(corners.into_iter().map(|p| (p, Snap::End)));
        }
        if snaps.middle {
            points.extend(middles.into_iter().map(|p| (p, Snap::Middle)));
        }
        if snaps.grid {
            points.extend(self.grid_snap_points().into_iter().map(|p| (p, Snap::Grid)));
            // where a grid line crosses the side or the axis of a wall or the edge of a floor: a
            // column goes against the inside of a wall on its grid line
            let grids = self.modeller.model.grids();
            for (a, b, z) in self.outline_lines(except) {
                for g in grids {
                    let (u, v) = (b - a, g.end - g.start);
                    let det = u.perp_dot(v);
                    if det.abs() < 1e-9 * u.length() * v.length() {
                        continue;
                    }
                    let (t, w) = ((g.start - a).perp_dot(v) / det, (g.start - a).perp_dot(u) / det);
                    if (-1e-6..=1.0 + 1e-6).contains(&t) && (-1e-6..=1.0 + 1e-6).contains(&w) {
                        points.push(((a + u * t).extend(z), Snap::Grid));
                    }
                }
            }
        }
        points
    }

    /// The modelled element whose line passes nearest viewport point (x, y), within
    /// [`NEAR_PIXELS`]: the axis of a wall, a curtain wall or a member (at its foot and at its
    /// top), the edge of a floor. For a click that hits no body: a curtain wall or a beam seen
    /// from above is a few pixels wide, too thin to click on.
    pub(crate) fn element_near(&self, x: f32, y: f32, width: f32, height: f32) -> Option<ElementId> {
        let grid = self.modeller.model.grid;
        let mut best: Option<(f32, ElementId)> = None;
        for e in self.modeller.model.iter() {
            let mut lines: Vec<(DVec3, DVec3)> = Vec::new();
            match e.shape_in_project(&grid) {
                Shape::Linear(l) => lines.push((l.start, l.end)),
                Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, base, height }, .. }) => lines.extend([(start.extend(base), end.extend(base)), (start.extend(base + height), end.extend(base + height))]),
                Shape::System(s) if !s.lies() => lines.extend([(s.start.extend(s.base), s.end.extend(s.base)), (s.start.extend(s.base + s.height), s.end.extend(s.base + s.height))]),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => {
                    let n = outline.len();
                    lines.extend((0..n).map(|k| (outline[k].extend(top), outline[(k + 1) % n].extend(top))));
                }
                _ => {}
            }
            for (a, b) in lines {
                let (Some(a), Some(b)) = (self.on_screen(a, width, height), self.on_screen(b, width, height)) else { continue };
                let (a, b, c) = (glam::Vec2::new(a.0, a.1), glam::Vec2::new(b.0, b.1), glam::Vec2::new(x, y));
                let along = b - a;
                let t = if along.length_squared() > 0.0 { ((c - a).dot(along) / along.length_squared()).clamp(0.0, 1.0) } else { 0.0 };
                let d = (a + along * t).distance(c);
                if d <= NEAR_PIXELS && best.is_none_or(|(least, _)| d < least) {
                    best = Some((d, e.id));
                }
            }
        }
        best.map(|(_, id)| id)
    }

    /// The lines of the outlines of the elements in plan, each with the height it lies at: both
    /// sides and the axis of a wall and of a standing system, the edges of a floor. Not those of
    /// element `except`.
    fn outline_lines(&self, except: ElementId) -> Vec<(DVec2, DVec2, f64)> {
        let grid = self.modeller.model.grid;
        let mut lines = Vec::new();
        let mut band = |start: DVec2, end: DVec2, half: f64, z: f64| {
            let Some(along) = (end - start).try_normalize() else { return };
            let side = along.perp() * half;
            lines.extend([(start, end, z), (start + side, end + side, z), (start - side, end - side, z)]);
        };
        let mut edges = Vec::new();
        for e in self.modeller.model.iter().filter(|e| e.id != except) {
            match e.shape_in_project(&grid) {
                Shape::Planar(p @ PlanarDef { form: PlanarForm::Wall { start, end, base, .. }, .. }) => band(start, end, p.total_thickness() / 2.0, base),
                Shape::System(s) if !s.lies() => band(s.start, s.end, s.member.max_element() / 2.0, s.base),
                Shape::Planar(PlanarDef { form: PlanarForm::Slab { outline, top, .. }, .. }) => {
                    let n = outline.len();
                    edges.extend((0..n).map(|k| (outline[k], outline[(k + 1) % n], top)));
                }
                _ => {}
            }
        }
        lines.extend(edges);
        lines
    }

    /// A click on the work plane at `p`: onto a point of an element near the cursor, else in line
    /// with the last point when nearly so, else rounded.
    pub(super) fn snapped(&self, p: DVec3, x: f32, y: f32, width: f32, height: f32) -> DVec3 {
        self.snapped_as(p, x, y, width, height).0
    }

    /// The point of [`App::snapped`], and the kind of snap that took it there (none for a click
    /// that is where it is, rounded).
    pub(super) fn snapped_as(&self, p: DVec3, x: f32, y: f32, width: f32, height: f32) -> (DVec3, Option<snaps::Snap>) {
        let (q, kind) = self.snapped_free(p, x, y, width, height);
        // square only: along x or y from the last point, whatever it landed on
        match self.modeller.points.last().filter(|_| self.modeller.snaps.square) {
            Some(from) => (snaps::squared(from.truncate(), q.truncate()).extend(q.z), kind.or(Some(snaps::Snap::Square))),
            None => (q, kind),
        }
    }

    /// The point of [`App::snapped_as`] before Square only holds it to an axis.
    fn snapped_free(&self, p: DVec3, x: f32, y: f32, width: f32, height: f32) -> (DVec3, Option<snaps::Snap>) {
        let near = self
            .snap_marks_except(ElementId(0))
            .into_iter()
            .filter_map(|(q, kind)| self.on_screen(q, width, height).map(|s| (q, kind, (s.0 - x).hypot(s.1 - y))))
            .filter(|(_, _, d)| *d <= SNAP_PIXELS)
            .min_by(|a, b| a.2.total_cmp(&b.2));
        if let Some((q, kind, _)) = near {
            return (DVec3::new(q.x, q.y, p.z), Some(kind));
        }
        let snaps = self.modeller.snaps;
        // the nearest point of a line under the cursor
        if snaps.line {
            let on_line = self
                .snap_lines()
                .into_iter()
                .filter_map(|(a, b)| {
                    let d = b - a;
                    let c = a + d * ((p.truncate() - a).dot(d) / d.length_squared()).clamp(0.0, 1.0);
                    let s = self.on_screen(c.extend(p.z), width, height)?;
                    Some((c, (s.0 - x).hypot(s.1 - y)))
                })
                .filter(|(_, d)| *d <= SNAP_PIXELS)
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((c, _)) = on_line {
                return (c.extend(p.z), Some(snaps::Snap::Line));
            }
        }
        let mut straight = None;
        let round = |v: f64| if snaps.round { (v / ROUND).round() * ROUND } else { v };
        let mut p = DVec3::new(round(p.x), round(p.y), p.z);
        if let Some(first) = self.modeller.points.last().filter(|_| snaps.ortho) {
            let d = p - *first;
            let angle = d.y.atan2(d.x).to_degrees().rem_euclid(90.0);
            if angle < ORTHO_DEGREES || angle > 90.0 - ORTHO_DEGREES {
                if d.x.abs() >= d.y.abs() {
                    p.y = first.y;
                } else {
                    p.x = first.x;
                }
                straight = Some(snaps::Snap::Ortho);
            }
        }
        if straight.is_none() && snaps.polar {
            if let Some(start) = self.modeller.points.last() {
                let delta = (p - *start).truncate();
                let heading = delta.y.atan2(delta.x);
                let step = std::f64::consts::FRAC_PI_4;
                let ray = (heading / step).round() * step;
                if delta.length_squared() > 0.0 && (heading - ray).abs().to_degrees() < ORTHO_DEGREES {
                    let direction = DVec2::new(ray.cos(), ray.sin());
                    p = (start.truncate() + direction * delta.dot(direction)).extend(p.z);
                    straight = Some(snaps::Snap::Polar);
                }
            }
        }
        (p, straight)
    }

    /// Project a metric work-plane point onto the active viewport, then use the GUI placement path.
    pub(crate) fn place_model_point(&mut self, x: f64, y: f64, rotation: Option<f64>, mirrored: Option<bool>) -> Result<serde_json::Value, String> {
        if !self.annotates || !self.modeller.in_modeller || self.active_doc_tab() != 0 { return Err("Open the Modeller 3D view first".into()) }
        // a sketch that is open takes the point: of a floor's outline, in plan
        if let Some(sketch) = &self.modeller.sketch {
            if sketch.face.is_some() { return Err("The sketch open lies on a wall: its points are clicked in the viewport (pick)".into()) }
            let lines = self.sketch_place_at(DVec2::new(x, y)).ok_or("The sketch tool that is on takes no points: press Line, Rectangle, Circle or Arc first")?;
            return Ok(serde_json::json!({"point": [x, y, self.work_plane().0], "snap": null, "created": [], "sketch_lines": lines}));
        }
        let (tool, _) = self.modeller.tool.ok_or("Choose a modelling tool in the ribbon first")?;
        if tool.opens() {
            // a door or a window with the way its width runs given: it stands there, its foot's
            // middle at the point, and cuts the walls it stands in
            let degrees = rotation.ok_or("A door or a window is clicked on a wall in the viewport (pick), or placed here with rotation: the way its width runs, in degrees from the x axis")?;
            let before = self.modeller.model.iter().map(|e| e.id.0).max().unwrap_or(0);
            self.place_opening_at(tool, x, y, degrees.to_radians(), mirrored);
            if let Some(reason) = self.modeller.refusal { return Err(reason.reason().into()) }
            let created: Vec<_> = self.modeller.model.iter().filter(|e| e.id.0 > before).map(|e| e.id.0).collect();
            return Ok(serde_json::json!({"point": [x, y, self.work_plane().0], "snap": null, "created": created}));
        }
        let (z, _, _) = self.work_plane();
        let (w, h) = self.viewport_size;
        let target = DVec3::new(x, y, z);
        let (px, py) = self.on_screen(target, w, h).filter(|(a,b)| (0.0..w).contains(a) && (0.0..h).contains(b)).ok_or("Point outside the current viewport")?;
        let before = self.modeller.model.iter().map(|e| e.id.0).max().unwrap_or(0);
        self.modeller.exact = Some(target);
        let clicked = self.click_work_plane(tool, px, py, w, h);
        self.modeller.exact = None;
        let (point, snap) = clicked.ok_or("The work plane is edge-on")?;
        if let Some(reason) = self.modeller.refusal { return Err(reason.reason().into()) }
        let created: Vec<_> = self.modeller.model.iter().filter(|e| e.id.0 > before).map(|e| e.id.0).collect();
        Ok(serde_json::json!({"point": point.to_array(), "snap": snap.map(|s| s.label(openaec_ui::Language::English)), "created": created}))
    }

    /// A click in the viewport while a tool is on.
    pub(super) fn model_click(&mut self, x: f32, y: f32, width: f32, height: f32) {
        let Some((tool, _)) = self.modeller.tool else { return };
        if tool.opens() {
            return self.place_opening(tool, x, y, width, height);
        }
        let _ = self.click_work_plane(tool, x, y, width, height);
    }

    fn click_work_plane(&mut self, tool: ModelTool, x: f32, y: f32, width: f32, height: f32) -> Option<(DVec3, Option<snaps::Snap>)> {
        let (_, name) = self.modeller.tool?;
        let (z, _, level) = self.work_plane();
        let p = self.plane_point(x, y, width, height, z)?;
        let (p, snap) = self.snapped_as(p, x, y, width, height);
        // a point given in metres (see `place_model_point`) is taken as given where no snap
        // moved it: the way through the screen and back costs it its last digits
        let p = match self.modeller.exact.take() {
            Some(given) if snap.is_none() && p.distance(given) < 1e-4 => given,
            _ => p,
        };
        // walls as a rectangle: the second click is the opposite corner, and four walls go
        // round, each from the end of the one before, as a chain does
        if tool == ModelTool::Wall && self.modeller.placement.rectangle {
            let Some(&a) = self.modeller.points.first() else {
                self.modeller.points.push(p);
                self.modeller.refusal = None;
                return Some((p, snap));
            };
            if (a.x - p.x).abs() < 1e-6 || (a.y - p.y).abs() < 1e-6 {
                return Some((p, snap));
            }
            let chained = std::mem::replace(&mut self.modeller.placement.chain, true);
            self.modeller.points.clear();
            self.modeller.placement.last_wall = None;
            for corner in [a, DVec3::new(p.x, a.y, a.z), DVec3::new(p.x, p.y, a.z), DVec3::new(a.x, p.y, a.z), a] {
                self.model_point(tool, name, level.clone(), corner);
            }
            self.modeller.points.clear();
            self.modeller.placement.last_wall = None;
            self.modeller.placement.chain = chained;
            return Some((p, snap));
        }
        self.model_point(tool, name, level, p);
        Some((p, snap))
    }

    /// The next point of `tool`, already snapped: with its last point the element is placed.
    fn model_point(&mut self, tool: ModelTool, name: &'static str, level: Option<String>, p: DVec3) {
        self.modeller.length_typed.clear();
        self.modeller.points.push(p);
        self.modeller.refusal = None;
        if self.modeller.points.len() < tool.clicks() {
            return;
        }
        let (a, b) = (self.modeller.points[0], *self.modeller.points.last().unwrap());
        let shape = self.shape_for(tool, a, b);
        // a wall on a face, chained: the last one's end goes to where the axes cross
        let joined = match (&shape, self.modeller.placement.last_wall.filter(|_| tool == ModelTool::Wall && self.modeller.points.len() > 1)) {
            (Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, .. }, .. }), Some(last)) => Some((last, *start)),
            _ => None,
        };
        self.modeller.points.clear();
        if let Some((last, start)) = joined {
            self.move_wall_end(last, start);
        }
        let name = self.placed_name(tool, name);
        // a run of a service starting where one ends at another height: a riser between them
        let service = self.tool_service(tool);
        if let Some(riser) = self.riser_to(tool, service, &shape) {
            let _ = self.add_element_served(name, level.clone(), None, service, riser);
        }
        // and one from each end that is over or under a connector of an appliance
        let to_connectors = self.risers_to_connectors(tool, &shape);
        let rise = if let Shape::Linear(l) = &shape { l.end.z - l.start.z } else { 0.0 };
        let connector_level = level.clone();
        // with no height typed, what stands reaches the level above, when there is one
        let storey = matches!(tool, ModelTool::Wall | ModelTool::Column | ModelTool::System) && self.modeller.placement.of(tool).height.is_none() && tops::has_top(&shape);
        let top = if storey { self.level_above(level.as_deref()).map(|level| bim_core::Top { level, offset: 0.0 }) } else { None };
        // a column or a beam takes the strip's strength class
        let grade = self.modeller.placement.of(tool).grade.filter(|_| matches!(tool, ModelTool::Column | ModelTool::Beam));
        let added = self.add_element_served(name, level, top, service, shape);
        if let (Ok(id), Some(grade)) = (&added, grade) {
            if let Some(mut graded) = self.modeller.model.get(*id).cloned() {
                graded.grade = Some(grade);
                let _ = self.modeller.model.replace(graded);
                self.show_modelled();
            }
        }
        match added {
            Ok(_) if tool.service().is_some() => {
                self.modeller.placement.risen(tool, a.truncate().distance(b.truncate()) < 1e-6, rise);
                // a run that ended on a connector is done: the chain stops there
                let ended = to_connectors.iter().any(|r| matches!(r, Shape::Linear(l) if (l.start - b).truncate().length() < 1e-6));
                for riser in to_connectors {
                    let _ = self.add_element_served(name, connector_level.clone(), None, service, riser);
                }
                if self.modeller.placement.chain && !ended {
                    self.modeller.points = vec![b];
                }
            }
            Ok(id) if tool.chains() && self.modeller.placement.chain => {
                self.modeller.points = vec![b];
                self.modeller.placement.last_wall = (tool == ModelTool::Wall).then_some(id);
            }
            Ok(_) => self.modeller.placement.last_wall = None,
            Err(refusal) => self.modeller.refusal = Some(refusal),
        }
    }

    /// The riser a run of a building service needs before it: when `shape`, a run `tool` is
    /// about to place, starts straight above or below the end of a run of the same kind and
    /// service (and it starts on none), a vertical run of that one's profile from that end to the
    /// start. A rectangular riser keeps its width across the run it comes from.
    fn riser_to(&self, tool: ModelTool, service: Option<bim_core::Service>, shape: &Shape) -> Option<Shape> {
        let role = tool.service()?;
        let Shape::Linear(run) = shape else { return None };
        // a run that is a riser itself needs none before it
        if (run.end - run.start).truncate().length() < 1e-6 {
            return None;
        }
        let grid = self.modeller.model.grid;
        let mut found: Option<(DVec3, DVec3, ProfileDef)> = None;
        for e in self.modeller.model.iter() {
            let Shape::Linear(l) = e.shape_in_project(&grid) else { continue };
            if l.role != role || e.service != service {
                continue;
            }
            // it starts on a run (a riser passing by, say): that is where it joins
            if bim_core::join::to_segment(run.start, l.start, l.end) < 1e-3 {
                return None;
            }
            for (end, other) in [(l.start, l.end), (l.end, l.start)] {
                let apart = end - run.start;
                if apart.truncate().length() > 1e-3 {
                    continue;
                }
                if apart.z.abs() < 1e-6 {
                    return None;
                }
                if found.as_ref().is_none_or(|(at, ..)| apart.z.abs() < (*at - run.start).z.abs()) {
                    found = Some((end, end - other, l.profile.clone()));
                }
            }
        }
        let (from, along, profile) = found?;
        let flat = along.truncate().try_normalize().or_else(|| (run.end - run.start).truncate().try_normalize())?;
        // across the run it comes from: the riser's own x, by its roll (see `linear_axes`)
        let across = DVec2::new(-flat.y, flat.x);
        let roll = if run.start.z > from.z { across.y.atan2(across.x) } else { (-across.y).atan2(across.x) };
        Some(Shape::Linear(LinearDef { start: from, end: run.start, profile, roll, role }))
    }

    /// Adds an element defined in scene coordinates, named `name` and a number, and shows it.
    pub(super) fn add_element(&mut self, name: &str, level: Option<String>, shape: Shape) -> Result<ElementId, Refusal> {
        self.add_element_reaching(name, level, None, shape)
    }

    /// As [`App::add_element`], the element reaching `top` with its top (see `tops`).
    pub(super) fn add_element_reaching(&mut self, name: &str, level: Option<String>, top: Option<bim_core::Top>, shape: Shape) -> Result<ElementId, Refusal> {
        self.add_element_served(name, level, top, None, shape)
    }

    /// As [`App::add_element_reaching`], the element of `service` when one is given: it joins
    /// only what is of its own service or of none.
    pub(super) fn add_element_served(&mut self, name: &str, level: Option<String>, top: Option<bim_core::Top>, service: Option<bim_core::Service>, shape: Shape) -> Result<ElementId, Refusal> {
        let grid = self.modeller.model.grid;
        let mut element = Element::in_project(&grid, name, level, shape);
        element.top = top;
        element.service = service;
        if let Some((tool,_)) = self.modeller.tool.filter(|(tool,_)| tool.service().is_some()) {
            if matches!(&element.shape, Shape::Linear(_)) {
                element.service_type = self.modeller.placement.of(tool).kind;
            }
        }
        // a build-up the library lacks (a material and a thickness of the strip) is added to it,
        // and is the type the tool goes on with
        if let Shape::Planar(PlanarDef { build_up: Some(b), form, .. }) = &element.shape {
            if self.library.buildup(&b.id).is_none() {
                let role = match form {
                    PlanarForm::Slab { role, .. } => super::placement::slab_role(*role),
                    _ => bim_library::Role::Wall,
                };
                let layers = b.layers.iter().map(|l| bim_library::Layer::new(&l.material, l.thickness)).collect();
                if self.library.add_buildup(bim_library::BuildUp::new(&b.id, role, layers)).is_ok() {
                    if let Some((tool, _)) = self.modeller.tool.filter(|(tool, _)| matches!(tool, ModelTool::Wall | ModelTool::Slab)) {
                        let mut s = self.modeller.placement.of(tool);
                        s.kind = Some(b.id.clone());
                        self.modeller.placement.set(tool, s);
                    }
                }
            }
        }
        // numbered per IFC class: Wall 1, Wall 2, Slab 1
        let number = self.modeller.model.iter().filter(|e| e.class() == element.class()).count() + 1;
        element.name = format!("{name} {number}");
        let id = self.modeller.model.add(element)?;
        self.joins_follow(&[id]);
        self.show_modelled();
        Ok(id)
    }

    /// The joins follow a change of elements `changed`: joins whose members parted go, and their
    /// ends that meet another linear element get one (see `bim_core::Model::update_joins`).
    pub(super) fn joins_follow(&mut self, changed: &[ElementId]) {
        let word = if self.modeller.join_word.is_empty() { "Connection" } else { self.modeller.join_word };
        let (added, _) = self.modeller.model.update_joins(word, Some(changed));
        // a fitting is named for what it is: a bend, a tee, a transition
        for id in added {
            let Some(mut e) = self.modeller.model.get(id).cloned() else { continue };
            let Shape::Connection(c) = &e.shape else { continue };
            let [bend, tee, transition] = self.modeller.fitting_words;
            let word = match c.kind {
                ConnectionKind::Bend(_) if !bend.is_empty() => bend,
                ConnectionKind::Bend(_) => "Bend",
                ConnectionKind::Tee(_) if !tee.is_empty() => tee,
                ConnectionKind::Tee(_) => "Tee",
                ConnectionKind::Transition(_) if !transition.is_empty() => transition,
                ConnectionKind::Transition(_) => "Transition",
                _ => continue,
            };
            let number = self.modeller.model.iter().filter(|o| matches!(&o.shape, Shape::Connection(k) if std::mem::discriminant(&k.kind) == std::mem::discriminant(&c.kind))).count();
            e.name = format!("{word} {number}");
            let _ = self.modeller.model.replace(e);
        }
    }

    /// The modelled elements among the selection, in pick order.
    pub(crate) fn selected_modelled(&self) -> Vec<ElementId> {
        let (Some(model), Some(shown)) = (&self.model, self.modeller.shown) else { return Vec::new() };
        let Some(entry) = self.models.iter().position(|e| e.model.id == shown) else { return Vec::new() };
        // a model shown alone keeps its own ids
        let entry = if self.models.len() == 1 { 0 } else { entry as u32 };
        self.sel
            .items()
            .iter()
            .filter_map(|item| match item {
                Pickable::Element(id) => {
                    let key = model.element_key(*id);
                    // a space is made from the walls: no element of the modelled project
                    (key.model == entry && key.source < spaces::SPACE_IDS).then_some(ElementId(key.source))
                }
                Pickable::Layer(_) => None,
            })
            .filter(|id| self.modeller.model.get(*id).is_some())
            .collect()
    }

    /// How many modelled linear elements are selected.
    pub(crate) fn selected_linear(&self) -> usize {
        self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(|e| e.base() == bim_core::BaseKind::Linear)).count()
    }

    /// Whether two modelled elements that a butt connection can join are selected: two linear
    /// ones, or two building systems.
    pub(crate) fn selected_joinable(&self) -> bool {
        let bases: Vec<bim_core::BaseKind> = self.selected_modelled().into_iter().filter_map(|id| self.modeller.model.get(id).map(|e| e.base())).filter(|b| *b != bim_core::BaseKind::Connection).collect();
        matches!(bases[..], [a, b] if a == b && matches!(a, bim_core::BaseKind::Linear | bim_core::BaseKind::System))
    }

    /// How many modelled elements with a body (not connections) are selected.
    pub(crate) fn selected_bodies(&self) -> usize {
        self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(|e| e.base() != bim_core::BaseKind::Connection)).count()
    }

    /// Joins the two linear elements or the two building systems selected: the first is the
    /// primary.
    pub(super) fn connect_selected(&mut self, kind: ConnectionKind, name: &str) {
        // a join takes two linear elements, butt or apart also two building systems (the kernel
        // refuses a mixed pair); a cut any two with a body, the first cutting the second
        let fits = |e: &Element| match kind {
            ConnectionKind::Cut => e.base() != bim_core::BaseKind::Connection,
            ConnectionKind::Butt | ConnectionKind::Apart => matches!(e.base(), bim_core::BaseKind::Linear | bim_core::BaseKind::System),
            ConnectionKind::Mitre | ConnectionKind::EndPlate(_) | ConnectionKind::FinPlate(_) | ConnectionKind::Bend(_) | ConnectionKind::Tee(_) | ConnectionKind::Transition(_) | ConnectionKind::Splice(_) | ConnectionKind::BasePlate(_) => e.base() == bim_core::BaseKind::Linear,
        };
        let picked: Vec<ElementId> = self.selected_modelled().into_iter().filter(|id| self.modeller.model.get(*id).is_some_and(fits)).collect();
        let [primary, secondary] = picked[..] else {
            self.modeller.refusal = Some(if kind == ConnectionKind::Cut { Refusal::NoBody } else { Refusal::NotLinear });
            return;
        };
        // two members joined already: their join takes the new kind (and this order)
        let pair = |c: &ConnectionDef| c.kind != ConnectionKind::Cut && ((c.primary, c.secondary) == (primary, secondary) || (c.primary, c.secondary) == (secondary, primary));
        let existing = self.modeller.model.iter().find(|e| matches!(&e.shape, Shape::Connection(c) if pair(c))).cloned();
        if let Some(mut joined) = existing.filter(|_| kind != ConnectionKind::Cut) {
            joined.shape = Shape::Connection(ConnectionDef { kind, primary, secondary });
            match self.modeller.model.replace(joined) {
                Ok(()) => self.show_modelled(),
                Err(refusal) => self.modeller.refusal = Some(refusal),
            }
            return;
        }
        let level = self.modeller.model.get(primary).and_then(|e| e.level.clone());
        if let Err(refusal) = self.add_element(name, level, Shape::Connection(ConnectionDef { kind, primary, secondary })) {
            self.modeller.refusal = Some(refusal);
        }
    }

    /// Removes the modelled elements selected (and the connections that join them). False when
    /// none was selected.
    pub(super) fn delete_modelled(&mut self) -> bool {
        let picked = self.selected_modelled();
        if picked.is_empty() {
            return false;
        }
        for id in picked {
            self.modeller.model.remove(id);
        }
        if self.modeller.edit.as_ref().is_some_and(|e| self.modeller.model.get(e.element).is_none()) {
            self.modeller.edit = None;
        }
        self.show_modelled();
        true
    }

    /// Shows the modelled elements again after a change: their viewer model takes the place of
    /// the one shown before, and the scene is put together again without moving the camera.
    pub(super) fn show_modelled(&mut self) {
        self.modeller.touched = true;
        self.issue_pile_numbers();
        let levels: Vec<(String, String)> = self.document.levels().iter().map(|l| (l.path.clone(), l.name.clone())).collect();
        let (mut layers, parts, assemblies, bars, symbolised) = viewer_layers_kept(&self.modeller.model, &levels, &self.library, &mut self.modeller.kept);
        // what is kept of the unchanged elements is what making them all again gives
        #[cfg(test)]
        {
            let whole = viewer_layers(&self.modeller.model, &levels, &self.library);
            let same = |a: &Layer, b: &Layer| a.name == b.name && a.hidden == b.hidden && a.positions == b.positions && a.normals == b.normals && a.colors == b.colors && a.indices == b.indices && a.elements == b.elements && a.props == b.props;
            assert!(whole.0.len() == layers.len() && whole.0.iter().zip(&layers).all(|(a, b)| same(a, b)) && whole.1 == parts && whole.2 == assemblies && whole.3 == bars && whole.4 == symbolised, "the model shown after a change is not the model made anew");
        }
        // the spaces its walls close in stand beside its elements, on a layer of their own
        layers.extend(spaces::space_layer(&self.spaces(), &levels));
        let mut built = Model::new(PathBuf::from("model.ifcx"), "IFCX", layers);
        built.project.grids = self.modelled_grid_lines();
        // the project's library goes into its file: materials and types, those taken from a file too;
        // a material with a hatch of its own in the hatch catalogue is cut with it. Made again
        // when the library changed, not with every change of the model
        let (types, cut_patterns, part_materials) = self.modeller.kept.of_library.get_or_insert_with(|| (bim_library::ifcx::nodes(&self.library), crate::hatching::library_patterns(&self.library), crate::hatching::part_materials(&self.library))).clone();
        built.project.types = types;
        built.project.cut_patterns = cut_patterns;
        // an element of several materials is cut a face per part, each hatched as its material
        built.project.parts = parts;
        // a framed system is an assembly per prefab unit
        built.project.assemblies = assemblies;
        built.project.bars = bars;
        built.project.symbolised = symbolised;
        built.project.part_materials = part_materials;
        // and its drawing standard: the kinds of drawing and their settings, a node each
        built.project.types.extend(bim_library::drawing::nodes(&self.standard));
        // and the house style it follows
        built.project.types.push(bim_library::house::node(&self.house));
        // and the names given to its spaces
        if !self.modeller.rooms.is_empty() {
            built.project.types.push(serde_json::json!({ "path": "openaec-rooms", "attributes": { crate::export::ifcx::ROOMS: self.modeller.rooms } }));
        }
        if let Some(file) = &self.modeller.file {
            built.path = file.clone();
        }
        let built = Arc::new(built);
        let before = self.modeller.shown.replace(built.id);
        if self.models.is_empty() {
            if let Some(m) = self.model.clone().filter(|m| Some(m.id) != before) {
                self.models.push(ModelEntry::new(m, 0));
            }
        }
        match before.and_then(|id| self.models.iter().position(|e| e.model.id == id)) {
            Some(k) => {
                let old = std::mem::replace(&mut self.models[k], ModelEntry::new(built, 0));
                // its layers keep their switches by name; new ones show
                let switched: std::collections::HashMap<&str, bool> = old.model.layers.iter().map(|l| l.name.as_str()).zip(old.layer_visibility.iter().copied()).collect();
                let entry = &mut self.models[k];
                entry.visible = old.visible;
                for (layer, shown) in entry.model.layers.iter().zip(entry.layer_visibility.iter_mut()) {
                    if let Some(&s) = switched.get(layer.name.as_str()) {
                        *shown = s;
                    }
                }
            }
            None => self.models.push(ModelEntry::new(built, 0)),
        }
        let mut first = 0;
        for entry in &mut self.models {
            entry.layers = first..first + entry.model.layers.len();
            first = entry.layers.end;
        }
        let scene = if self.models.len() == 1 {
            self.models[0].model.clone()
        } else {
            let sources: Vec<Arc<Model>> = self.models.iter().map(|e| e.model.clone()).collect();
            let mut combined = Model::combine(&sources);
            // a project that took over an opened file's walls is an IFCX document of its own
            if let Some(file) = &self.modeller.project_file {
                combined.path = file.clone();
                combined.format = "IFCX".into();
            }
            Arc::new(combined)
        };
        self.replace_scene(scene);
        // the dimensions that hold on to elements follow them
        self.follow_dimensions();
        // and the floor plans say their spaces and draw their doors' swings again
        self.tag_plans();
        self.list_piles();
        self.list_bars();
        self.list_planks();
    }

    /// Puts `model` in the place of the scene shown, keeping the camera, the views and the
    /// document: what follows the scene's layers and ids is built again.
    fn replace_scene(&mut self, model: Arc<Model>) {
        self.model = Some(model.clone());
        self.visible = model.layers.iter().map(|l| !l.hidden).collect();
        self.restore_model_visibility();
        self.sel.clear();
        self.selected = None;
        self.selection.clear();
        self.pick_index = None;
        self.hover_scene = None;
        self.measure.forget_edges();
        self.cuts.clear();
        self.cut_epoch += 1;
        self.explode_index = Some(Arc::new(crate::explode::ExplodeIndex::new(&model)));
        self.update_explode();
        if let Some(b) = self.world_bounds() {
            self.camera.scene_radius = self.camera.scene_radius.max(b.radius());
        }
    }
}

#[cfg(test)]
impl App {
    /// A small building of all five base elements, for renders: a slab, four walls (one with a
    /// door and a window), columns and beams joined butt and mitre, a curtain wall and an object.
    pub(crate) fn add_demo_building(&mut self) {
        use bim_core::Opening;
        self.new_model("Level 0");
        let _ = self.document.add_level("Level 1", 3.0);
        let level = self.work_plane().2;
        let p = DVec2::new;
        let add = |app: &mut App, name: &str, shape: Shape| app.add_element(name, level.clone(), shape).expect("the demo evaluates");
        let slab = PlanarForm::Slab { outline: vec![p(-0.1, -0.1), p(10.1, -0.1), p(10.1, 6.1), p(-0.1, 6.1)], holes: Vec::new(), top: 0.0, bulges: Vec::new(), role: Default::default(), slope: None };
        add(self, "Slab", Shape::Planar(PlanarDef { form: slab, thickness: SLAB_THICKNESS, openings: Vec::new(), build_up: None }));
        let wall = |a: DVec2, b: DVec2, openings: Vec<Opening>| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: a, end: b, base: 0.0, height: 3.0 }, thickness: WALL_THICKNESS, openings, build_up: None });
        let door = Opening { along: 1.0, sill: 0.0, width: 1.0, height: 2.3, framing: None };
        let window = Opening { along: 3.5, sill: 0.9, width: 2.4, height: 1.5, framing: None };
        add(self, "Wall", wall(p(0.0, 0.0), p(10.0, 0.0), vec![door, window]));
        let side = add(self, "Wall", wall(p(10.0, 0.0), p(10.0, 6.0), Vec::new()));
        add(self, "Wall", wall(p(0.0, 6.0), p(0.0, 0.0), Vec::new()));
        add(self, "Curtain wall", Shape::System(SystemDef { start: p(10.0, 6.0), end: p(0.0, 6.0), base: 0.0, height: 3.0, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), grid: None, division: None }));
        // a pergola beside it: two columns and three beams in a U; the joins come by themselves
        // (butts at the columns, mitres at the far corners), and one butt becomes an end plate
        let column = |at: DVec2| Shape::Linear(LinearDef { start: at.extend(0.0), end: at.extend(3.0), profile: ProfileDef::Rectangle { width: COLUMN, height: COLUMN }, roll: 0.0, role: Default::default() });
        let c1 = add(self, "Column", column(p(13.0, 0.0)));
        add(self, "Column", column(p(13.0, 6.0)));
        let beam = |a: DVec2, b: DVec2| Shape::Linear(LinearDef { start: a.extend(2.6), end: b.extend(2.6), profile: ProfileDef::Rectangle { width: BEAM.0, height: BEAM.1 }, roll: 0.0, role: Default::default() });
        add(self, "Beam", beam(p(13.0, 6.0), p(17.0, 6.0)));
        let b3 = add(self, "Beam", beam(p(13.0, 0.0), p(17.0, 0.0)));
        add(self, "Beam", beam(p(17.0, 0.0), p(17.0, 6.0)));
        let butt = self.modeller.model.iter().find(|e| matches!(&e.shape, Shape::Connection(c) if c.primary == c1 && c.secondary == b3)).cloned();
        if let Some(mut joint) = butt {
            joint.shape = Shape::Connection(ConnectionDef { kind: ConnectionKind::EndPlate(bim_core::join::EndPlate::default()), primary: c1, secondary: b3 });
            self.modeller.model.replace(joint).expect("the demo's end plate");
            self.show_modelled();
        }
        add(self, "Object", Shape::Point(PointDef { at: DVec3::new(7.0, 3.0, 0.0), rotation: 0.4, size: OBJECT, opening: None, mirrored: false, role: Default::default() }));
        // a beam through the side wall, cutting its way
        let through = add(self, "Beam", beam(p(8.5, 3.0), p(13.0, 3.0)));
        add(self, "Cut-out", Shape::Connection(ConnectionDef { kind: ConnectionKind::Cut, primary: through, secondary: side }));
        let _ = self.update(super::Message::Fit);
    }

}

/// What `tool` places from its first point `a` to its last `b` (scene coordinates), on a work
/// plane at height `z` with a storey of `storey` above it, with the type and sizes `s` (types
/// from `library`). A wall's `a` and `b` are its axis ends, already on its location line.
/// The profile of a run of a building service: a duct of 1 mm sheet `width` × `depth`, a round
/// duct or a pipe of diameter `width` (a pipe's wall a twentieth of it, at least 1 mm), a cable
/// tray `width` wide with sides `depth` high, of 2 mm sheet, open above.
pub(super) fn service_profile(tool: ModelTool, width: f64, depth: f64, wall: Option<f64>) -> ProfileDef {
    let mm = |v: f64| format!("{}", (v * 1000.0).round());
    match tool {
        ModelTool::RoundDuct => ProfileDef::Catalogue { designation: format!("Ø{}", mm(width)), shape: bim_library::ProfileShape::Chs { d: width, t: wall.unwrap_or(0.001) } },
        ModelTool::Pipe => ProfileDef::Catalogue { designation: format!("Ø{}", mm(width)), shape: bim_library::ProfileShape::Chs { d: width, t: wall.unwrap_or_else(|| (width / 20.0).max(0.001)) } },
        ModelTool::CableTray => {
            let (w, h, t) = (width / 2.0, depth, wall.unwrap_or(0.002));
            let outer = vec![DVec2::new(-w, -h / 2.0), DVec2::new(w, -h / 2.0), DVec2::new(w, h / 2.0), DVec2::new(w - t, h / 2.0), DVec2::new(w - t, -h / 2.0 + t), DVec2::new(-w + t, -h / 2.0 + t), DVec2::new(-w + t, h / 2.0), DVec2::new(-w, h / 2.0)];
            ProfileDef::Polygon { outer, holes: Vec::new() }
        }
        _ => ProfileDef::Catalogue { designation: format!("{}×{}", mm(width), mm(depth)), shape: bim_library::ProfileShape::Rhs { h: depth, b: width, t: wall.unwrap_or(0.001), r_out: 0.0, r_in: 0.0 } },
    }
}

pub(super) fn tool_shape(tool: ModelTool, a: DVec3, b: DVec3, z: f64, storey: f64, s: &Settings, library: &bim_library::Library) -> Shape {
    // up to the next level, unless a height was typed
    let up_to = |base: f64| s.height.unwrap_or(z + storey - base);
    // a steel profile by its designation, else a linear type of the library, named as the library
    // names it and whole without it
    let typed = |id: &str| {
        let t = library.linear_type(id)?;
        let designation = library.linear_name(id, bim_library::Lang::En).unwrap_or_else(|| id.to_string());
        Some(ProfileDef::Catalogue { designation, shape: t.profile.clone() })
    };
    let profile = || s.kind.as_deref().and_then(|id| ProfileDef::steel(id).or_else(|| typed(id))).unwrap_or(ProfileDef::Rectangle { width: s.width, height: s.depth });
    // a type of one layer is as thick as was typed: the type of that material and thickness (the
    // library's own, else one added to it when the element is: see `add_element_served`)
    let build_up = |id: &Option<String>, role: bim_library::Role| {
        let of = library.buildup(id.as_deref()?)?;
        match &of.layers[..] {
            [layer] if (layer.thickness - s.width).abs() > 1e-6 && s.width > 0.0 => {
                let own = super::buildups::one_layer(library, role, &layer.material, s.width);
                let cavity = library.material(&layer.material).is_some_and(|m| m.hatch == bim_library::HatchKind::Air);
                Some(bim_core::BuildUpDef { id: own.id, layers: vec![bim_core::LayerDef { material: layer.material.clone(), thickness: s.width, cavity }] })
            }
            _ => bim_core::BuildUpDef::from_library(library, &of.id),
        }
    };
    match tool {
        // an opening tool places none of its own (see `openings`): an object stands for it
        ModelTool::Appliance => Shape::Point(PointDef { at: a + DVec3::Z * s.offset, rotation: 0.0, size: DVec3::new(s.width, s.depth.max(0.01), s.height.unwrap_or(OBJECT.z)), opening: None, mirrored: false, role: s.appliance }),
        ModelTool::Point | ModelTool::Door | ModelTool::Window => {
            Shape::Point(PointDef { at: a + DVec3::Z * s.offset, rotation: 0.0, size: DVec3::new(s.width, s.depth.max(0.01), s.height.unwrap_or(OBJECT.z)), opening: None, mirrored: false, role: Default::default() })
        }
        ModelTool::Column => {
            let base = z + s.offset;
            Shape::Linear(LinearDef { start: a.truncate().extend(base), end: a.truncate().extend(base + up_to(base)), profile: profile(), roll: 0.0, role: Default::default() })
        }
        // a beam's top lies at the top of the storey, or at the height typed above its level
        ModelTool::Beam => {
            let profile = profile();
            let half = profile.profile().map_or(s.depth / 2.0, |p| p.outer().iter().map(|q| q.y).fold(0.0, f64::max));
            let up = DVec3::Z * (z + s.height.unwrap_or(storey) - half - a.z);
            Shape::Linear(LinearDef { start: a + up, end: b + up, profile, roll: 0.0, role: Default::default() })
        }
        ModelTool::Duct | ModelTool::RoundDuct | ModelTool::Pipe | ModelTool::CableTray => {
            let up = DVec3::Z * (z + s.height.unwrap_or(SERVICE_HEIGHT) - a.z);
            let role = tool.service().unwrap_or_default();
            // its rise takes its end up (a slope; a riser when both clicks are one point)
            // and a pipe with a fall drops as it runs: 1 in `sill`
            let fall = if tool == ModelTool::Pipe && s.sill > 0.0 { a.truncate().distance(b.truncate()) / s.sill } else { 0.0 };
            Shape::Linear(LinearDef { start: a + up, end: b + up + DVec3::Z * (s.offset - fall), profile: service_profile(tool, s.width, s.depth, s.kind.as_deref().and_then(|id| library.service_type(id)).and_then(|series| series.sizes.iter().find(|size| (size.width-s.width).abs() < 1e-6 && ((size.height-s.depth).abs() < 1e-6 || matches!(tool, ModelTool::Pipe | ModelTool::RoundDuct)))).map(|size| size.wall)), roll: 0.0, role })
        }
        ModelTool::Wall => {
            let base = z + s.offset;
            let wall = PlanarDef { form: PlanarForm::Wall { start: a.truncate(), end: b.truncate(), base, height: up_to(base) }, thickness: s.width, openings: Vec::new(), build_up: None };
            Shape::Planar(wall.with_build_up(build_up(&s.kind, bim_library::Role::Wall)))
        }
        ModelTool::Stair => {
            let base = z + s.offset;
            let height = up_to(base);
            let risers = stair_risers(s, height);
            // with a going given the stair is as long as its treads make it, along the way clicked;
            // with none it runs from click to click
            let (from, to) = (a.truncate(), b.truncate());
            let end = match (to - from).try_normalize() {
                Some(way) if s.depth > 0.0 => from + way * (s.depth * (risers - 1) as f64),
                _ => to,
            };
            let shape = bim_core::StairShape::Run { path: vec![from, end], width: s.width };
            Shape::Planar(PlanarDef { form: PlanarForm::Stair { base, height, risers, shape, railing: s.railing }, thickness: STAIR_WAIST, openings: Vec::new(), build_up: None })
        }
        ModelTool::Slab => {
            let (lo, hi) = (a.truncate().min(b.truncate()), a.truncate().max(b.truncate()));
            let outline = vec![lo, DVec2::new(hi.x, lo.y), hi, DVec2::new(lo.x, hi.y)];
            let slab = PlanarDef { form: PlanarForm::Slab { outline, holes: Vec::new(), top: z + s.offset, bulges: Vec::new(), role: s.role, slope: None }, thickness: s.width, openings: Vec::new(), build_up: None };
            Shape::Planar(slab.with_build_up(build_up(&s.kind, super::placement::slab_role(s.role))))
        }
        ModelTool::System => {
            let base = z + s.offset;
            let generic = SystemDef {
                start: a.truncate(),
                end: b.truncate(),
                base,
                height: up_to(base),
                spacing: SYSTEM_SPACING,
                member: SYSTEM_MEMBER,
                panel: SYSTEM_PANEL,
                system_type: None,
                openings: Vec::new(),
                breaks: Vec::new(),
                outline: Vec::new(), grid: None,
                division: None,
            };
            // a lying type (a floor, a ceiling) is laid in the rectangle between the two clicks, as a
            // slab is: its line along the rectangle's near side, its width to the far one, its top
            // at the height its type lies above the level
            if let Some((t, above)) = s.kind.as_deref().and_then(|id| library.system_type(id)).and_then(|t| Some((t, t.lying?))) {
                let (lo, hi) = (a.truncate().min(b.truncate()), a.truncate().max(b.truncate()));
                let rectangle = SystemDef { start: lo, end: DVec2::new(hi.x, lo.y), base: base + above, height: hi.y - lo.y, ..generic.clone() };
                if let Some(laid) = rectangle.laid(t) {
                    return Shape::System(laid);
                }
            }
            let typed = s.kind.as_deref().and_then(|id| generic.clone().typed_from(library, id));
            Shape::System(typed.unwrap_or(generic))
        }
    }
}

impl App {
    /// What the tool places from `a` to `b` (scene coordinates, as clicked) with the strip's type
    /// and sizes: a wall on its location line, a beam with the side picked on the line.
    pub(super) fn shape_for(&self, tool: ModelTool, a: DVec3, b: DVec3) -> Shape {
        let (z, storey, _) = self.work_plane();
        let s = self.modeller.placement.of(tool);
        let (a, b) = match tool {
            ModelTool::Wall => self.wall_axis(a, b, &s),
            ModelTool::Beam => self.beam_axis(a, b, &s),
            _ => (a, b),
        };
        tool_shape(tool, a, b, z, storey, &s, &self.library)
    }

    /// Moves the end of wall `id` to `to` (in plan), if the kernel can make it so.
    fn move_wall_end(&mut self, id: ElementId, to: DVec2) {
        let Some(e) = self.modeller.model.get(id).cloned() else { return };
        let grid = self.modeller.model.grid;
        let mut shape = e.shape_in_project(&grid);
        if let Shape::Planar(PlanarDef { form: PlanarForm::Wall { end, .. }, .. }) = &mut shape {
            *end = to;
        }
        let mut moved = e.reshaped(&grid, shape);
        moved.id = id;
        let _ = self.modeller.model.replace(moved);
    }
}

/// How thick a stair is under its treads, as its tool makes it (metres).
pub const STAIR_WAIST: f64 = 0.15;
/// The highest a step of a stair is when its tool counts the steps (metres).
pub const HIGHEST_RISER: f64 = 0.185;

/// How many steps the Stair tool gives a stair `height` high: what its strip says, else as few
/// as keep every step under [`HIGHEST_RISER`].
pub(super) fn stair_risers(s: &Settings, height: f64) -> usize {
    let typed = s.sill.round() as usize;
    if typed >= bim_core::element::FEWEST_RISERS {
        return typed;
    }
    ((height / HIGHEST_RISER).ceil().clamp(2.0, 1000.0)) as usize
}

/// A click this near the line of an element selects it when it hits no body (logical pixels).
const NEAR_PIXELS: f32 = 6.0;

/// The colour of what belongs to a service (sRGB): supply warm or fresh, return and waste dull.
fn service_colour(service: bim_core::Service) -> [f32; 3] {
    use bim_core::Service as S;
    match service {
        S::SupplyAir => [0.30, 0.58, 0.90],
        S::ReturnAir => [0.80, 0.50, 0.72],
        S::ExhaustAir => [0.56, 0.70, 0.36],
        S::HeatingSupply => [0.86, 0.26, 0.22],
        S::HeatingReturn => [0.24, 0.36, 0.72],
        S::ColdWater => [0.30, 0.74, 0.86],
        S::HotWater => [0.94, 0.52, 0.20],
        S::Waste => [0.52, 0.42, 0.32],
        S::RainWater => [0.28, 0.62, 0.58],
        S::Gas => [0.92, 0.82, 0.24],
        S::Power => [0.88, 0.72, 0.28],
        S::Data => [0.58, 0.46, 0.80],
    }
}

/// The colour of a layer of modelled elements, by IFC class (sRGB).
fn class_colour(class: &str) -> [f32; 3] {
    match class {
        "IfcWall" => [0.86, 0.84, 0.80],
        "IfcSlab" => [0.70, 0.70, 0.72],
        "IfcRoof" => [0.66, 0.42, 0.36],
        "IfcCovering" => [0.93, 0.93, 0.90],
        "IfcColumn" | "IfcBeam" => [0.55, 0.62, 0.72],
        "IfcMember" => [0.45, 0.47, 0.52],
        "IfcPlate" => [0.62, 0.80, 0.92],
        "IfcCurtainWall" => [0.62, 0.80, 0.92],
        "IfcDoor" => [0.72, 0.55, 0.38],
        "IfcWindow" => [0.95, 0.95, 0.93],
        "IfcDuctSegment" | "IfcDuctFitting" => [0.45, 0.68, 0.86],
        "IfcPipeSegment" | "IfcPipeFitting" => [0.36, 0.66, 0.42],
        "IfcCableCarrierSegment" | "IfcCableCarrierFitting" => [0.88, 0.72, 0.28],
        // a stair as concrete is
        "IfcStair" => [0.74, 0.74, 0.72],
        "IfcSpaceHeater" | "IfcSanitaryTerminal" => [0.94, 0.94, 0.95],
        "IfcBoiler" | "IfcPump" | "IfcFan" | "IfcUnitaryEquipment" | "IfcTank" | "IfcValve" => [0.72, 0.74, 0.78],
        "IfcAirTerminal" | "IfcOutlet" | "IfcLightFixture" => [0.90, 0.90, 0.86],
        _ => [0.90, 0.60, 0.30],
    }
}

/// The viewer model of modelled elements: one layer per IFC class, every element's parts as its
/// triangles (each part in the colour of its own class: a curtain wall's members and panels), its
/// definition as its properties. A refused element shows nothing.
/// `levels` names the levels by path.
/// A part of a layered element has the colour of its layer's material in `library`.
pub fn viewer_model(model: &bim_core::Model, levels: &[(String, String)], library: &bim_library::Library) -> Model {
    let (layers, parts, assemblies, bars, symbolised) = viewer_layers(model, levels, library);
    let mut built = Model::new(PathBuf::from("model.ifcx"), "IFCX", layers);
    built.project.symbolised = symbolised;
    built.project.parts = parts;
    built.project.assemblies = assemblies;
    built.project.bars = bars;
    built.project.part_materials = crate::hatching::part_materials(library);
    built.project.cut_patterns = crate::hatching::library_patterns(library);
    built
}

/// The layers of the model shown: one per class, each element with its properties; and the
/// parts of the elements that are of several materials (see `ProjectData::parts`): the layer,
/// the first index and the index count of each part, and its material in the library's order.
/// And the assemblies of the framed systems (see `ProjectData::assemblies`): the parts of such
/// a system are drawn assembly by assembly. And the runs of reinforcing bars (see
/// `ProjectData::bars`).
#[allow(clippy::type_complexity)]
fn viewer_layers(model: &bim_core::Model, levels: &[(String, String)], library: &bim_library::Library) -> (Vec<Layer>, Vec<(usize, u32, u32, u32)>, Vec<crate::model::Assembly>, Vec<(usize, u32, u32)>, Vec<(usize, u32, u32)>) {
    viewer_layers_kept(model, levels, library, &mut Kept::default())
}

/// What one element shows: its triangles (the indices from its own first vertex), its parts of
/// several materials, its assemblies and its bars (their places from its own first index and
/// vertex), and its properties.
#[derive(Debug)]
struct Shown {
    class: &'static str,
    positions: Vec<f32>,
    normals: Vec<f32>,
    colours: Vec<[u8; 4]>,
    indices: Vec<u32>,
    /// The first index and the index count of each part, and its material.
    parts: Vec<(u32, u32, u32)>,
    /// The first vertex and the vertex count of each reinforcing bar.
    bars: Vec<(u32, u32)>,
    /// The first vertex and the vertex count of what a floor plan draws by a symbol: a door's
    /// leaf, a window's pane.
    symbol: Option<(u32, u32)>,
    /// Its assemblies: `layer` is not set, `first` counts from its own first index.
    assemblies: Vec<crate::model::Assembly>,
    props: String,
}

/// What was shown of the model the last time, kept so that a change makes only the elements it
/// touches again (see `bim_core::Model::touched_since`): a model of a thousand walls is not made
/// again for one wall more. Everything is made again when the levels' names or the library
/// (a material's colour, a type's name) changed.
#[derive(Debug, Clone, Default)]
pub struct Kept {
    model: Option<bim_core::Model>,
    levels: Vec<(String, String)>,
    library: Option<bim_library::Library>,
    shown: std::collections::HashMap<ElementId, Option<Arc<Shown>>>,
    /// What follows from the library alone: its nodes in the file, its materials' cut patterns
    /// and the materials of the parts.
    #[allow(clippy::type_complexity)]
    of_library: Option<(Vec<serde_json::Value>, std::collections::HashMap<String, crate::hatching::Pattern>, Vec<crate::hatching::PartMaterial>)>,
}

/// As `viewer_layers`, making again only what changed since `kept` was made, and keeping what
/// is shown now in it.
#[allow(clippy::type_complexity)]
fn viewer_layers_kept(model: &bim_core::Model, levels: &[(String, String)], library: &bim_library::Library, kept: &mut Kept) -> (Vec<Layer>, Vec<(usize, u32, u32, u32)>, Vec<crate::model::Assembly>, Vec<(usize, u32, u32)>, Vec<(usize, u32, u32)>) {
    let same = kept.levels == levels && kept.library.as_ref() == Some(library);
    let touched = kept.model.as_ref().filter(|_| same).and_then(|before| model.touched_since(before));
    match &touched {
        Some(touched) => kept.shown.retain(|id, _| !touched.contains(id) && model.get(*id).is_some()),
        None => kept.shown.clear(),
    }
    if !same {
        kept.levels = levels.to_vec();
        kept.library = Some(library.clone());
        kept.of_library = None;
    }
    kept.model = Some(model.clone());
    let mut layers: Vec<Layer> = Vec::new();
    let mut of_parts: Vec<(usize, u32, u32, u32)> = Vec::new();
    let has_rebar_areas = model.iter().any(|e| !e.reinforcement_areas.is_empty());
    let mut of_assemblies: Vec<crate::model::Assembly> = Vec::new();
    let mut bars: Vec<(usize, u32, u32)> = Vec::new();
    let mut symbolised: Vec<(usize, u32, u32)> = Vec::new();
    for element in model.iter() {
        let shown = kept.shown.entry(element.id).or_insert_with(|| shown_element(model, element, levels, library).map(Arc::new));
        let Some(shown) = shown else { continue };
        let class = shown.class;
        let k = match layers.iter().position(|l| l.name == class) {
            Some(k) => k,
            None => {
                layers.push(Layer { name: class.into(), hidden: class == "IfcReinforcingBar" && !has_rebar_areas, color: class_colour(class), ..Layer::default() });
                layers.len() - 1
            }
        };
        let layer = &mut layers[k];
        let first = layer.indices.len() as u32;
        let base = (layer.positions.len() / 3) as u32;
        layer.positions.extend_from_slice(&shown.positions);
        layer.normals.extend_from_slice(&shown.normals);
        layer.colors.get_or_insert_with(Vec::new).extend_from_slice(&shown.colours);
        layer.indices.extend(shown.indices.iter().map(|i| i + base));
        of_parts.extend(shown.parts.iter().map(|&(part_first, count, material)| (k, first + part_first, count, material)));
        for &(bar_first, count) in &shown.bars {
            match bars.last_mut() {
                Some(run) if run.0 == k && run.1 + run.2 == base + bar_first => run.2 += count,
                _ => bars.push((k, base + bar_first, count)),
            }
        }
        symbolised.extend(shown.symbol.map(|(symbol_first, count)| (k, base + symbol_first, count)));
        of_assemblies.extend(shown.assemblies.iter().map(|a| crate::model::Assembly { layer: k, first: first + a.first, ..a.clone() }));
        layer.elements.push((element.id.0, first, shown.indices.len() as u32));
        layer.props.push((element.id.0, shown.props.clone()));
    }
    of_parts.sort_unstable();
    of_assemblies.sort_by_key(|a| (a.layer, a.first));
    bars.sort_unstable();
    symbolised.sort_unstable();
    (layers, of_parts, of_assemblies, bars, symbolised)
}

/// What `element` shows; `None` for one without a body (a connection without parts, one that
/// cannot be made).
fn shown_element(model: &bim_core::Model, element: &Element, levels: &[(String, String)], library: &bim_library::Library) -> Option<Shown> {
    let grid = model.grid;
    let mut parts = model.evaluate(element.id).ok()?;
    if parts.is_empty() {
        return None;
    }
    // a framed system: its parts in the order of its prefab units, and where each unit ends
    let units = bim_core::system::assemblies(model, element, &parts).unwrap_or_default();
    let mut ends: Vec<usize> = Vec::with_capacity(units.len());
    if !units.is_empty() && units.iter().map(|u| u.parts.len()).sum::<usize>() == parts.len() {
        let mut taken: Vec<Option<bim_core::Part>> = parts.into_iter().map(Some).collect();
        parts = Vec::with_capacity(taken.len());
        for u in &units {
            parts.extend(u.parts.iter().filter_map(|&i| taken[i].take()));
            ends.push(parts.len());
        }
    }
    let mut unit_firsts: Vec<u32> = Vec::with_capacity(ends.len() + 1);
    let mut shown = Shown { class: element.class(), positions: Vec::new(), normals: Vec::new(), colours: Vec::new(), indices: Vec::new(), parts: Vec::new(), bars: Vec::new(), symbol: None, assemblies: Vec::new(), props: String::new() };
    // of several materials: each part is told from the others where it is cut
    // (its reinforcing bars are no part of that: no drawing cuts them)
    let bar = |p: &bim_core::Part| p.class == bim_core::reinforcement::BAR_CLASS;
    let several = parts.iter().filter(|p| !bar(p)).filter_map(|p| p.material.as_deref()).collect::<std::collections::HashSet<_>>().len() > 1;
    let filling = match &element.shape {
        Shape::Point(p) if p.opening.is_some() && parts.len() > 1 => Some(parts.len() - 1),
        _ => None,
    };
    for (n, part) in parts.iter().enumerate() {
        if n == 0 || ends.contains(&n) {
            unit_firsts.push(shown.indices.len() as u32);
        }
        let Ok(mesh) = Mesh::of(&part.solid) else { continue };
        let part_first = shown.indices.len() as u32;
        // tile-local f64 to the scene's f32, through the tile origin in f64
        let origin = grid.origin(part.tile);
        let base = (shown.positions.len() / 3) as u32;
        if part.class == bim_core::reinforcement::BAR_CLASS {
            shown.bars.push((base, mesh.positions.len() as u32));
        }
        // what fills a door or a window (the last of its solids, see `opening_solids`)
        if filling == Some(n) {
            shown.symbol = Some((base, mesh.positions.len() as u32));
        }
        let material = part.material.as_deref().and_then(|key| library.material(key));
        // of a service: in its colour, so the systems are told apart
        let plain = element.service.map_or_else(|| class_colour(part.class), service_colour);
        let colour = material.map_or_else(|| plain.map(|c| (c * 255.0).round() as u8), |m| m.colour);
        shown.colours.extend(std::iter::repeat_n([colour[0], colour[1], colour[2], 255], mesh.positions.len()));
        for (p, n) in mesh.positions.iter().zip(&mesh.normals) {
            shown.positions.extend((origin + *p).as_vec3().to_array());
            shown.normals.extend(n.as_vec3().to_array());
        }
        shown.indices.extend(mesh.indices.iter().map(|i| i + base));
        if let Some(material) = part.material.as_deref().filter(|_| several && !bar(part)).and_then(|key| library.materials.iter().position(|m| m.key == key)) {
            shown.parts.push((part_first, shown.indices.len() as u32 - part_first, material as u32));
        }
    }
    unit_firsts.push(shown.indices.len() as u32);
    if !ends.is_empty() {
        let mut from = 0;
        for ((u, range), &to) in units.iter().zip(unit_firsts.windows(2)).zip(&ends) {
            shown.assemblies.push(crate::model::Assembly { layer: 0, first: range[0], count: range[1] - range[0], element: element.id.0, number: u.number as u32, of: units.len() as u32, mark: u.mark(element), length: u.length(), parts: (to - from) as u32 });
            from = to;
        }
    }
    let level = element.level.as_deref().and_then(|path| levels.iter().find(|(p, _)| p == path)).map(|(_, name)| name.as_str());
    shown.props = properties(model, element, &parts, level, library);
    Some(shown)
}

/// The NL-SfB code of an element: of its build-up, its linear type (found by the name its
/// profile has) or its system type as the library has it; of a type the library lacks, the
/// rule's for its layers. None for what has no type.
fn nlsfb_of(e: &Element, library: &bim_library::Library) -> Option<String> {
    match &e.shape {
        Shape::Planar(p) => {
            let b = p.build_up.as_ref()?;
            let role = match p.form {
                PlanarForm::Wall { .. } => bim_library::Role::Wall,
                _ => library.buildup(&b.id).map_or(bim_library::Role::Floor, |of| of.role),
            };
            let of = library.buildup(&b.id).cloned().unwrap_or_else(|| bim_library::BuildUp::new(&b.id, role, b.layers.iter().map(|l| bim_library::Layer::new(&l.material, l.thickness)).collect()));
            Some(of.nlsfb(&library.materials))
        }
        Shape::Linear(l) => {
            let name = l.profile.name();
            library.linear_types.iter().find(|t| library.linear_name(&t.id, bim_library::Lang::En).as_deref() == Some(name.as_str())).map(|t| t.nlsfb())
        }
        Shape::System(s) => library.system_type(&s.system_type.as_ref()?.id).map(|t| t.nlsfb()),
        _ => None,
    }
}

/// An element's properties as a flat JSON object.
fn properties(model: &bim_core::Model, e: &Element, parts: &[bim_core::Part], level: Option<&str>, library: &bim_library::Library) -> String {
    let grid = model.grid;
    let m = |v: f64| format!("{v:.3} m");
    let mut props = serde_json::Map::new();
    let mut put = |k: &str, v: String| {
        props.insert(k.into(), serde_json::Value::String(v));
    };
    put("Name", e.name.clone());
    put("class", e.class().into());
    put("Base element", e.base().name().into());
    put("Tile", format!("{}, {}", e.tile.x, e.tile.y));
    put("id", e.id.0.to_string());
    if let Some(level) = level {
        put("level", level.into());
    }
    // the service it is of, and the kind of system that is in IFC
    if let Some(key) = &e.service_type { put("Service type", key.clone()); }
    if let Some(service) = e.service {
        put("Service", service.name().into());
        put("System type", service.ifc_system().into());
    }
    // the definitions an IFCX file keeps and gives back: the element's, and those of the
    // connections it is the primary member of (a connection has no body, so no node of its own)
    let joints = model.connections_of(e.id).filter(|c| matches!(&c.shape, Shape::Connection(j) if j.primary == e.id));
    let definitions: Vec<&Element> = std::iter::once(e).chain(joints).collect();
    put(crate::export::ifcx::DEFINITION, serde_json::to_string(&definitions).unwrap_or_default());
    match e.shape_in_project(&grid) {
        Shape::Point(p) => {
            put("Size", format!("{} × {} × {}", m(p.size.x), m(p.size.y), m(p.size.z)));
        }
        Shape::Linear(l) => {
            put("Profile", l.profile.name());
            if let Some(grade) = &e.grade {
                put("Grade", grade.clone());
            }
            let span = l.end.distance(l.start);
            put("Length", m(e.hairpin_leg.map_or(span, |leg| 2.0 * leg + std::f64::consts::PI * span / 2.0)));
            if let Some(r) = &e.reinforcement {
                put("Reinforcement", crate::app::element_props::bars_text(r));
            }
        }
        Shape::Planar(p) => {
            put("Thickness", m(p.total_thickness()));
            if let Some(n) = &e.nets {
                put("Reinforcement", crate::app::element_props::nets_text(n, "bottom", "top"));
            }
            if let Some(b) = &p.build_up {
                // property values are data, in English as an IFC file's
                put("Type", library.name(&b.id, bim_library::Lang::En).unwrap_or_else(|| b.id.clone()));
                let layer = |l: &bim_core::LayerDef| format!("{} {}", library.material(&l.material).map_or(l.material.as_str(), |m| m.name(bim_library::Lang::En)), bim_library::millimetres(l.thickness, bim_library::Lang::En));
                put("Layers", b.layers.iter().map(layer).collect::<Vec<_>>().join(", "));
            }
            if let PlanarForm::Wall { start, end, height, .. } = p.form {
                put("Length", m((end - start).length()));
                put("Height", m(height));
            }
            // a stair: its steps, how high each is and, where they are all one depth, its going
            if let PlanarForm::Stair { height, risers, shape, .. } = &p.form {
                put("Height", m(*height));
                put("Risers", risers.to_string());
                put("Riser height", format!("{} mm", bim_library::millimetres(height / *risers as f64, bim_library::Lang::En)));
                if let bim_core::StairShape::Run { path, width } = shape {
                    put("Width", m(*width));
                    if let ([a, b], true) = (&path[..], *risers > 1) {
                        put("Going", format!("{} mm", bim_library::millimetres(a.distance(*b) / (*risers - 1) as f64, bim_library::Lang::En)));
                    }
                }
            }
        }
        Shape::System(s) => {
            put("Length", m((s.end - s.start).length()));
            put("Height", m(s.height));
            put("Parts", parts.len().to_string());
            // the prefab units it is made of, by their marks
            let units = bim_core::system::assemblies(model, e, parts).unwrap_or_default();
            if !units.is_empty() {
                put("Assemblies", units.iter().map(|u| u.mark(e)).collect::<Vec<_>>().join(", "));
            }
            if let Some(t) = &s.system_type {
                put("Type", library.system_name(&t.id, bim_library::Lang::En).unwrap_or_else(|| t.id.clone()));
                // a system is a curtain wall by class, which carries nothing: its type's group
                // says whether it does, and so whether a structural plan draws it
                if let Some(group) = library.system_type(&t.id).map(|t| t.group) {
                    put("LoadBearing", group.carries().to_string());
                }
            }
        }
        Shape::Connection(c) => {
            let kind = match c.kind {
                ConnectionKind::Butt => "Butt",
                ConnectionKind::Mitre => "Mitre",
                ConnectionKind::Cut => "Cut",
                ConnectionKind::EndPlate(_) => "End plate",
                ConnectionKind::FinPlate(_) => "Fin plate",
                ConnectionKind::Splice(_) => "Splice",
                ConnectionKind::Bend(_) => "Bend",
                ConnectionKind::Tee(_) => "Tee",
                ConnectionKind::Transition(_) => "Transition",
                ConnectionKind::BasePlate(_) => "Base plate",
                ConnectionKind::Apart => "Apart",
            };
            put("Kind", kind.into());
            match c.kind.fixity() {
                Some(bim_core::Fixity::Pinned) => put("Fixity", "Pinned".into()),
                Some(bim_core::Fixity::Rigid) => put("Fixity", "Rigid".into()),
                None => {}
            }
            put("Primary", c.primary.0.to_string());
            put("Secondary", c.secondary.0.to_string());
        }
    }
    // its NL-SfB code: that of its type in the library, else the rule's for what it is
    if let Some(code) = nlsfb_of(e, library) {
        put(crate::export::ifc::NLSFB, code);
    }
    // what it is made of, when it is of one material: the cut of it takes that material's hatch
    // (the bars in it apart: concrete with bars is concrete)
    let body = || parts.iter().filter(|p| p.class != bim_core::reinforcement::BAR_CLASS);
    if let Some(key) = body().next().and_then(|p| p.material.as_deref()).filter(|key| body().all(|p| p.material.as_deref() == Some(key))) {
        put("Material", library.material(key).map_or(key, |m| m.name(bim_library::Lang::En)).to_string());
    } else if e.reinforcement.is_some() || e.nets.is_some() {
        // of no material told, with bars in it: reinforced concrete, and cut as concrete
        put("Material", "Reinforced concrete".into());
    }
    let volume: f64 = parts.iter().map(|p| p.solid.volume()).sum();
    if volume > 0.0 {
        put("Volume", format!("{volume:.3} m³"));
    }
    serde_json::Value::Object(props).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::Navigate;
    use crate::app::Message;

    /// An app with a new model, seen from the top, the viewport 1000 × 800 with the origin in
    /// its middle and 10 px per metre.
    pub(super) fn modelling() -> App {
        let mut app = App::default();
        app.new_model("Level 0");
        app.viewport_size = (1000.0, 800.0);
        app.camera.set_preset(crate::viewport::camera::ViewPreset::Top);
        app.camera.ortho = true;
        app.camera.target = Vec3::ZERO;
        // ortho framing: half height = distance · tan(fov / 2) = 40 m for 400 px
        app.camera.distance = 40.0 / (app.camera.fov_y * 0.5).tan();
        app
    }

    /// Clicks at scene point (x, y) on the work plane.
    pub(super) fn click(app: &mut App, x: f64, y: f64) {
        let (sx, sy) = app.on_screen(DVec3::new(x, y, 0.0), 1000.0, 800.0).unwrap();
        let _ = app.update(Message::Navigate(Navigate::Pick { x: sx, y: sy, width: 1000.0, height: 800.0, add: false }));
    }

    #[test]
    fn a_change_makes_only_what_it_touches_again() {
        let mut app = modelling();
        let wall = |a: (f64, f64), b: (f64, f64)| Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(a.0, a.1), end: DVec2::new(b.0, b.1), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let near = app.add_element("Wall", None, wall((0.0, 0.0), (5.0, 0.0))).unwrap();
        let far = app.add_element("Wall", None, wall((20.0, 0.0), (25.0, 0.0))).unwrap();
        let shown = |app: &App, id: ElementId| app.modeller.kept.shown[&id].clone().expect("a wall shows");
        let (near_before, far_before) = (shown(&app, near), shown(&app, far));
        // a wall at the corner of the first: that one is cut to the corner, the far one is kept
        // as it was made (every change is checked against the model made anew, see `show_modelled`)
        app.add_element("Wall", None, wall((5.0, 0.0), (5.0, 4.0))).unwrap();
        assert!(!Arc::ptr_eq(&near_before, &shown(&app, near)), "the wall at the corner is made again");
        assert!(Arc::ptr_eq(&far_before, &shown(&app, far)), "the wall away from it is kept");
        // a material's colour changed: everything is made again
        app.library.materials[0].colour = [1, 2, 3];
        app.show_modelled();
        assert!(!Arc::ptr_eq(&far_before, &shown(&app, far)));
        // and a wall removed is no longer kept
        app.select_modelled(far);
        let _ = app.update(Message::DeletePicked);
        assert!(!app.modeller.kept.shown.contains_key(&far));
    }

    #[test]
    fn ducts_drawn_one_after_another_get_a_bend_where_they_turn_and_a_branch_gets_a_tee() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Duct, "Duct")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 10.0, 0.0);
        // the chain goes on from the end of the first
        click(&mut app, 10.0, 8.0);
        app.set_model_tool(Some((ModelTool::RoundDuct, "Round duct")));
        click(&mut app, 5.0, 0.0);
        click(&mut app, 5.0, -6.0);
        app.set_model_tool(None);
        let classes = |app: &App| {
            let mut c: Vec<&'static str> = app.modeller.model.iter().map(|e| e.class()).collect();
            c.sort();
            c
        };
        // two ducts and their bend; the round branch is a duct too, and ends on the first in a tee
        assert_eq!(classes(&app), ["IfcDuctFitting", "IfcDuctFitting", "IfcDuctSegment", "IfcDuctSegment", "IfcDuctSegment"]);
        let grid = app.modeller.model.grid;
        let duct = app.modeller.model.iter().find(|e| e.class() == "IfcDuctSegment").unwrap();
        let Shape::Linear(l) = duct.shape_in_project(&grid) else { panic!() };
        assert_eq!((l.start.z, l.role, l.profile.name().as_str()), (SERVICE_HEIGHT, bim_core::LinearRole::Duct, "400×200"));
        // every fitting has a body
        for e in app.modeller.model.iter().filter(|e| e.class() == "IfcDuctFitting") {
            assert!(!app.modeller.model.evaluate(e.id).unwrap().is_empty(), "{}", e.name);
        }
        // a pipe and a cable tray are runs of their own kind, and get no fitting to a duct
        app.set_model_tool(Some((ModelTool::Pipe, "Pipe")));
        click(&mut app, -20.0, 5.0);
        click(&mut app, -10.0, 5.0);
        app.set_model_tool(Some((ModelTool::CableTray, "Cable tray")));
        click(&mut app, -20.0, 9.0);
        click(&mut app, -10.0, 9.0);
        app.set_model_tool(None);
        let all = classes(&app);
        assert!(all.contains(&"IfcPipeSegment") && all.contains(&"IfcCableCarrierSegment"), "{all:?}");
    }

    #[test]
    fn a_duct_drawn_on_at_another_height_gets_a_riser_and_at_another_size_a_transition() {
        use super::super::placement::{PlaceField, PlaceMessage};
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Duct, "Duct")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 10.0, 0.0);
        // the chain goes on 0.9 higher: a riser at the end of the first, a bend at its foot and top
        app.place_message(PlaceMessage::Field(PlaceField::Height, "3.5".into()));
        click(&mut app, 20.0, 0.0);
        let grid = app.modeller.model.grid;
        let runs: Vec<LinearDef> = app.modeller.model.iter().filter_map(|e| match e.shape_in_project(&grid) { Shape::Linear(l) => Some(l), _ => None }).collect();
        assert_eq!(runs.len(), 3);
        let riser = &runs[1];
        assert!((riser.start - DVec3::new(10.0, 0.0, 2.6)).length() < 1e-9 && (riser.end - DVec3::new(10.0, 0.0, 3.5)).length() < 1e-9, "{riser:?}");
        let fittings: Vec<_> = app.modeller.model.iter().filter(|e| e.class() == "IfcDuctFitting").map(|e| e.id).collect();
        assert_eq!(fittings.len(), 2);
        for id in fittings {
            assert_eq!(app.modeller.model.evaluate(id).unwrap().len(), 6);
        }
        // and on, at the same height but smaller: a transition
        app.place_message(PlaceMessage::Field(PlaceField::Width, "0.2".into()));
        click(&mut app, 30.0, 0.0);
        let kinds: Vec<_> = app.modeller.model.iter().filter_map(|e| match &e.shape { Shape::Connection(c) => Some(c.kind), _ => None }).collect();
        assert!(matches!(kinds.last(), Some(bim_core::ConnectionKind::Transition(_))), "{kinds:?}");
        // a pipe with a rise typed and both clicks on one point is a riser; the next starts at its top
        app.set_model_tool(Some((ModelTool::Pipe, "Pipe")));
        app.place_message(PlaceMessage::Field(PlaceField::Height, "0.3".into()));
        app.place_message(PlaceMessage::Field(PlaceField::Offset, "2.5".into()));
        click(&mut app, 0.0, 20.0);
        click(&mut app, 0.0, 20.0);
        click(&mut app, 6.0, 20.0);
        let pipes: Vec<LinearDef> = app.modeller.model.iter().filter(|e| e.class() == "IfcPipeSegment").filter_map(|e| match e.shape_in_project(&grid) { Shape::Linear(l) => Some(l), _ => None }).collect();
        assert_eq!(pipes.len(), 2);
        assert!((pipes[0].start - DVec3::new(0.0, 20.0, 0.3)).length() < 1e-9 && (pipes[0].end - DVec3::new(0.0, 20.0, 2.8)).length() < 1e-9, "{:?}", pipes[0]);
        assert!((pipes[1].start.z - 2.8).abs() < 1e-9 && (pipes[1].end.z - 2.8).abs() < 1e-9, "{:?}", pipes[1]);
        assert_eq!(app.modeller.model.iter().filter(|e| e.class() == "IfcPipeFitting").count(), 1);
        // a pipe drawn in a service is of it, as is the fitting to the next; one of another service
        // ending at the same point is not joined to them
        use bim_core::Service;
        app.place_message(PlaceMessage::Field(PlaceField::Offset, "0".into()));
        app.modeller.points.clear();
        app.place_message(PlaceMessage::Service(Some(Service::HeatingSupply)));
        click(&mut app, 0.0, 40.0);
        click(&mut app, 6.0, 40.0);
        click(&mut app, 6.0, 46.0);
        app.modeller.points.clear();
        app.place_message(PlaceMessage::Service(Some(Service::ColdWater)));
        click(&mut app, 6.0, 46.0);
        click(&mut app, 12.0, 46.0);
        let served: Vec<(&str, Option<Service>)> = app.modeller.model.iter().filter(|e| e.service.is_some()).map(|e| (e.class(), e.service)).collect();
        let (pipe, fitting) = ("IfcPipeSegment", "IfcPipeFitting");
        assert_eq!(served, [(pipe, Some(Service::HeatingSupply)), (pipe, Some(Service::HeatingSupply)), (fitting, Some(Service::HeatingSupply)), (pipe, Some(Service::ColdWater))]);
        let scene = app.model.as_ref().unwrap();
        assert_eq!(scene.elements_with("System type", "HEATING").len(), 3);
        // fittings are named for what they are, and have no mark over them: they are picked by their body
        let names: Vec<&str> = app.modeller.model.iter().filter(|e| matches!(e.shape, Shape::Connection(_))).map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Bend 1", "Bend 2", "Transition 1", "Bend 3", "Bend 4"]);
        app.set_model_tool(None);
        assert!(app.join_marks(iced::Color::BLACK, iced::Color::WHITE).is_none());
    }

    #[test]
    fn the_appliance_tool_places_a_radiator_of_its_usual_size_in_a_service() {
        use super::super::placement::PlaceMessage;
        use bim_core::{PointRole, Service};
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Appliance, "Appliance")));
        app.place_message(PlaceMessage::Service(Some(Service::HeatingSupply)));
        click(&mut app, 2.0, 3.0);
        // another kind takes its own usual size and class, and keeps the service
        app.place_message(PlaceMessage::Appliance(PointRole::Boiler));
        click(&mut app, 5.0, 3.0);
        let placed: Vec<(&str, &str, Option<Service>, DVec3, f64)> = app
            .modeller
            .model
            .iter()
            .filter_map(|e| match &e.shape {
                Shape::Point(p) => Some((e.name.as_str(), e.class(), e.service, p.size, p.at.z)),
                _ => None,
            })
            .collect();
        assert_eq!(
            placed,
            [
                ("Radiator 1", "IfcSpaceHeater", Some(Service::HeatingSupply), DVec3::new(1.0, 0.1, 0.6), 0.15),
                ("Boiler 1", "IfcBoiler", Some(Service::HeatingSupply), DVec3::new(0.45, 0.35, 0.7), 1.2),
            ]
        );
        // the remote control picks in the strip's lists by name, in English or as shown
        assert!(app.pick_tool_option("kind", "pomp", Language::Dutch) && app.pick_tool_option("service", "Cold water", Language::Dutch));
        assert_eq!(app.tool_settings().map(|(_, s)| (s.appliance, s.service, s.width)), Some((PointRole::Pump, Some(Service::ColdWater), 0.18)));
        assert!(!app.pick_tool_option("kind", "sofa", Language::Dutch));
        assert!(app.pick_tool_option("service", "verwarming aanvoer", Language::Dutch));
        // picked up again, the tool takes its kind, size and service
        let boiler = app.modeller.model.iter().last().unwrap().id;
        app.place_message(PlaceMessage::Appliance(PointRole::Pump));
        app.set_model_tool(None);
        app.select_modelled(boiler);
        app.create_similar(Language::English);
        assert_eq!(app.tool_settings().map(|(tool, s)| (tool, s.appliance, s.service, s.width)), Some((ModelTool::Appliance, PointRole::Boiler, Some(Service::HeatingSupply), 0.45)));
    }

    #[test]
    fn a_column_snaps_to_where_a_grid_line_crosses_the_inside_of_a_wall() {
        let mut app = modelling();
        // a wall along x, 0.3 thick: its sides at y = -0.15 and 0.15; a grid line across it at x = 4
        let form = PlanarForm::Wall { start: DVec2::new(0.0, 0.0), end: DVec2::new(8.0, 0.0), base: 0.0, height: 3.0 };
        app.add_element("Wall", None, Shape::Planar(PlanarDef { form, thickness: 0.3, openings: Vec::new(), build_up: None })).unwrap();
        app.add_grid_line(DVec2::new(4.0, -3.0), DVec2::new(4.0, 5.0)).unwrap();
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        // a click a few centimetres off lands on the crossing with the inner side
        click(&mut app, 4.04, 0.19);
        let column = app.modeller.model.iter().last().unwrap().shape_in_project(&app.modeller.model.grid);
        let Shape::Linear(l) = column else { panic!() };
        assert!((l.start.truncate() - DVec2::new(4.0, 0.15)).length() < 1e-9, "{:?}", l.start);
    }

    #[test]
    fn a_click_beside_a_thin_element_still_selects_it() {
        let mut app = modelling();
        app.modeller.in_modeller = true;
        app.set_model_tool(Some((ModelTool::System, "Curtain wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.0);
        app.set_model_tool(None);
        app.modeller.points.clear();
        let id = app.modeller.model.iter().next().unwrap().id;
        // from above a curtain wall is a line 5 cm wide: a click 30 cm beside it (3 pixels here)
        // takes it, one a metre and a half away does not and clears the selection
        click(&mut app, 3.0, 0.3);
        assert_eq!(app.selected_modelled(), [id]);
        click(&mut app, 3.0, 1.5);
        assert!(app.selected_modelled().is_empty());
    }

    #[test]
    fn square_only_draws_along_x_or_y_whatever_is_clicked_and_off_any_angle() {
        let mut app = modelling();
        let walls = |app: &App| -> Vec<(DVec2, DVec2)> {
            let grid = app.modeller.model.grid;
            app.modeller.model.iter().filter_map(|e| match e.shape_in_project(&grid) { Shape::Planar(PlanarDef { form: PlanarForm::Wall { start, end, .. }, .. }) => Some((start, end)), _ => None }).collect()
        };
        // off (as it starts): a wall at any angle
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        app.modeller.placement.chain = false;
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 3.0);
        assert_eq!(walls(&app), [(DVec2::ZERO, DVec2::new(4.0, 3.0))]);
        // on: the same clicks give a wall along x, the longer way; and one mostly up a wall along y
        let _ = app.update(Message::SnapToggle(snaps::Snap::Square));
        click(&mut app, 0.0, 10.0);
        click(&mut app, 4.0, 13.0);
        click(&mut app, 10.0, 10.0);
        click(&mut app, 11.0, 15.0);
        assert_eq!(walls(&app)[1..], [(DVec2::new(0.0, 10.0), DVec2::new(4.0, 10.0)), (DVec2::new(10.0, 10.0), DVec2::new(10.0, 15.0))]);
        // even a click on the corner of another wall is held to the axis
        click(&mut app, 0.0, 20.0);
        click(&mut app, 4.0, 13.0);
        assert_eq!(walls(&app)[3], (DVec2::new(0.0, 20.0), DVec2::new(0.0, 13.0)));
    }

    #[test]
    fn a_new_model_has_a_level_and_shows_the_viewport() {
        let app = modelling();
        assert!(app.model.is_some());
        assert_eq!(app.document.levels().len(), 1);
        assert_eq!(app.work_plane().0, 0.0);
    }

    #[test]
    fn two_clicks_place_a_wall_and_the_next_one_starts_at_its_end() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        assert_eq!(app.modeller.model.len(), 1);
        let wall = app.modeller.model.iter().next().unwrap();
        assert_eq!(wall.name, "Wall 1");
        assert_eq!(wall.class(), "IfcWall");
        assert_eq!(app.modeller.points.len(), 1, "chained: the next wall starts here");
        click(&mut app, 5.0, 4.0);
        assert_eq!(app.modeller.model.len(), 2);
        // shown: one layer of walls with two elements
        let scene = app.model.as_ref().unwrap();
        let walls = scene.layers.iter().find(|l| l.name.ends_with("IfcWall")).expect("a wall layer");
        assert_eq!(walls.elements.len(), 2);
        assert!(app.escape_model_tool() && app.modeller.tool.is_some(), "Esc drops the points first");
        assert!(app.escape_model_tool() && app.modeller.tool.is_none(), "then the tool");
    }

    #[test]
    fn enter_finishes_a_wall_chain_without_placing_an_extra_segment() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        let _ = app.update(Message::MeasureFinish);
        assert!(app.modeller.tool.is_none() && app.modeller.points.is_empty());
        click(&mut app, 7.0, 2.0);
        assert_eq!(app.modeller.model.len(), 1);
    }

    #[test]
    fn clicks_near_an_end_snap_to_it_and_nearly_straight_lines_straighten() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.15);
        let Shape::Linear(l) = app.modeller.model.iter().next().unwrap().shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!((l.end - l.start).truncate(), DVec2::new(6.0, 0.0), "in line with the first point");
        app.set_model_tool(Some((ModelTool::Column, "Column")));
        // 0.4 m off the beam's end is 4 px on screen: it snaps
        click(&mut app, 6.3, 0.2);
        let column = app.modeller.model.iter().find(|e| e.class() == "IfcColumn").expect("a column");
        let Shape::Linear(c) = column.shape_in_project(&app.modeller.model.grid) else { panic!() };
        assert_eq!(c.start.truncate(), DVec2::new(6.0, 0.0));
        // the beam's end on the column's side: joined, the column running through
        let join = app.modeller.model.iter().find_map(|e| match &e.shape {
            Shape::Connection(j) => Some(j.clone()),
            _ => None,
        });
        assert!(join.is_some_and(|j| j.kind == ConnectionKind::Butt && j.primary == column.id));
    }

    #[test]
    fn every_tool_places_its_base_element() {
        let mut app = modelling();
        for (i, tool) in ModelTool::ALL.into_iter().enumerate() {
            app.set_model_tool(Some((tool, "E")));
            let y = 10.0 * i as f64;
            click(&mut app, 0.0, y);
            if tool.clicks() == 2 {
                click(&mut app, 6.0, y + 3.0);
            }
            app.set_model_tool(None);
        }
        let bases: std::collections::HashSet<_> = app.modeller.model.iter().map(|e| e.base()).collect();
        assert_eq!(bases.len(), 4, "point, linear, planar, system");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
    }

    #[test]
    fn two_selected_members_join_and_delete_takes_the_joint_along() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 3.0);
        app.set_model_tool(None);
        let ids: Vec<u64> = app.modeller.model.iter().map(|e| e.id.0).collect();
        let scene = app.model.clone().unwrap();
        for id in &ids {
            let shown = scene.element_with_key(crate::model::ElementKey { model: if app.models.len() == 1 { 0 } else { (app.models.len() - 1) as u32 }, source: *id }).unwrap();
            app.sel.toggle(Pickable::Element(shown));
        }
        app.connect_selected(ConnectionKind::Mitre, "Joint");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        assert_eq!(app.modeller.model.len(), 3);
        // select the first beam again and delete it: its joint goes with it
        let scene = app.model.clone().unwrap();
        let shown = scene.element_with_key(crate::model::ElementKey { model: if app.models.len() == 1 { 0 } else { (app.models.len() - 1) as u32 }, source: ids[0] }).unwrap();
        app.sel.set(Some(Pickable::Element(shown)));
        assert!(app.delete_modelled());
        assert_eq!(app.modeller.model.len(), 1);
    }

    #[test]
    fn tab_on_a_framed_wall_opens_its_sketch_and_a_rectangle_in_it_is_a_framed_opening() {
        use super::sketch::{Segment, SketchError};
        let mut app = App::default();
        let generic = bim_core::SystemDef { grid: None, start: glam::DVec2::new(2.0, 1.0), end: glam::DVec2::new(6.8, 1.0), base: 0.0, height: 2.7, spacing: glam::DVec2::new(1.5, 1.5), member: glam::DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
        let s = generic.typed_from(&app.library, "system.timber-frame-140").unwrap();
        let grid = app.modeller.model.grid;
        let id = app.modeller.model.add(Element::in_project(&grid, "Frame", None, Shape::System(s))).unwrap();
        app.show_modelled();
        let whole = app.modeller.model.evaluate(id).unwrap().len();
        assert!(app.sketchable(id));
        app.open_sketch(id);
        // its outline in its own face: four lines, upright from its base at its start
        let sketch = app.modeller.sketch.clone().unwrap();
        assert_eq!(sketch.segments.len(), 4);
        let face = sketch.face.unwrap();
        assert_eq!((face.origin, face.along), (glam::DVec3::new(2.0, 1.0, 0.0), glam::DVec3::X));
        // a window drawn in it, 1.2 by 1.2 at 1.2 along and 0.9 up
        let p = glam::DVec2::new;
        let rectangle = |a: glam::DVec2, b: glam::DVec2| [Segment::line(a, p(b.x, a.y)), Segment::line(p(b.x, a.y), b), Segment::line(b, p(a.x, b.y)), Segment::line(p(a.x, b.y), a)];
        app.modeller.sketch.as_mut().unwrap().segments.extend(rectangle(p(1.2, 0.9), p(2.4, 2.1)));
        app.finish_sketch();
        assert!(app.modeller.sketch.is_none() && app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let Shape::System(made) = &app.modeller.model.get(id).unwrap().shape else { panic!() };
        let mm = |v: f64| (v * 1000.0).round() as i64;
        assert_eq!(made.openings.iter().map(|o| (mm(o.along), mm(o.sill), mm(o.width), mm(o.height))).collect::<Vec<_>>(), [(1200, 900, 1200, 1200)]);
        assert!(made.outline.is_empty());
        // framed: a header, a sill, trimmers and king studs more than the studs it took out
        assert!(app.modeller.model.evaluate(id).unwrap().len() > whole);

        // again: the window is there as four lines; a corner off the top at the end
        app.open_sketch(id);
        let sketch = app.modeller.sketch.as_mut().unwrap();
        assert_eq!(sketch.segments.len(), 8);
        sketch.segments.splice(0..4, [Segment::line(p(0.0, 0.0), p(4.8, 0.0)), Segment::line(p(4.8, 0.0), p(4.8, 1.8)), Segment::line(p(4.8, 1.8), p(3.9, 2.7)), Segment::line(p(3.9, 2.7), p(0.0, 2.7)), Segment::line(p(0.0, 2.7), p(0.0, 0.0))]);
        app.finish_sketch();
        assert!(app.modeller.sketch.is_none() && app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let Shape::System(made) = &app.modeller.model.get(id).unwrap().shape else { panic!() };
        assert_eq!((made.outline.len(), made.openings.len(), made.height), (5, 1, 2.7));
        // as high as ever at its start, and under the cut at its end
        let highest = |from: f64, to: f64| app.modeller.model.evaluate(id).unwrap().iter().flat_map(|part| part.solid.positions.clone()).filter(|q| q.x - 2.0 >= from && q.x - 2.0 <= to).map(|q| q.z).fold(0.0, f64::max);
        assert!(highest(0.0, 1.0) > 2.69 && highest(4.6, 4.8) < 2.0 && highest(4.6, 4.8) > 1.7, "{} {}", highest(0.0, 1.0), highest(4.6, 4.8));

        // an opening that is not a rectangle is told, and the sketch stays open
        app.open_sketch(id);
        app.modeller.sketch.as_mut().unwrap().segments.extend([Segment::line(p(0.3, 0.3), p(0.9, 0.3)), Segment::line(p(0.9, 0.3), p(0.6, 0.9)), Segment::line(p(0.6, 0.9), p(0.3, 0.3))]);
        app.finish_sketch();
        assert_eq!(app.modeller.sketch.as_ref().unwrap().error, Some(SketchError::OpeningNotSquare));
    }

    #[test]
    fn an_opening_sketched_in_brickwork_goes_to_the_courses_and_gets_a_lintel() {
        use super::sketch::Segment;
        let mut app = App::default();
        let generic = bim_core::SystemDef { grid: None, start: glam::DVec2::new(2.0, 1.0), end: glam::DVec2::new(6.4, 1.0), base: 0.0, height: 2.5, spacing: glam::DVec2::new(1.5, 1.5), member: glam::DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
        let s = generic.typed_from(&app.library, "system.masonry-waal-stretcher").unwrap();
        let grid = app.modeller.model.grid;
        let id = app.modeller.model.add(Element::in_project(&grid, "Wall", None, Shape::System(s))).unwrap();
        app.show_modelled();
        assert!(app.sketchable(id));
        app.open_sketch(id);
        // a window drawn from 0.92 to 2.13 up: its sill goes to course 15, its head to course 34
        let p = glam::DVec2::new;
        let (a, b) = (p(1.5, 0.92), p(2.5, 2.13));
        app.modeller.sketch.as_mut().unwrap().segments.extend([Segment::line(a, p(b.x, a.y)), Segment::line(p(b.x, a.y), b), Segment::line(b, p(a.x, b.y)), Segment::line(p(a.x, b.y), a)]);
        app.finish_sketch();
        assert!(app.modeller.sketch.is_none() && app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let Shape::System(made) = &app.modeller.model.get(id).unwrap().shape else { panic!() };
        let o = made.openings[0];
        assert!((o.sill - 15.0 * 0.0625).abs() < 1e-9 && (o.sill + o.height - 34.0 * 0.0625).abs() < 1e-9, "{o:?}");
        let parts = app.modeller.model.evaluate(id).unwrap();
        assert_eq!(parts.iter().filter(|p| p.class == "IfcBeam").count(), 1);
        // no brick thinner than a whole one less its joints
        let thin = parts.iter().filter(|p| p.class == "IfcPlate").filter(|p| {
            let (lo, hi) = p.solid.bounds();
            hi.z - lo.z < 0.05
        }).count();
        assert_eq!(thin, 0);
    }

    #[test]
    fn a_timber_frame_is_exported_as_an_assembly_for_every_prefab_unit() {
        // 7.2 m of timber frame: three units of 2400
        let mut app = App::default();
        let generic = bim_core::SystemDef { grid: None, start: glam::DVec2::new(2.0, 1.0), end: glam::DVec2::new(9.2, 1.0), base: 0.0, height: 2.7, spacing: glam::DVec2::new(1.5, 1.5), member: glam::DVec2::new(0.05, 0.15), panel: 0.02, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), division: None };
        let s = generic.typed_from(&app.library, "system.timber-frame-140").unwrap();
        let grid = app.modeller.model.grid;
        let id = app.modeller.model.add(Element::in_project(&grid, "Frame", None, Shape::System(s))).unwrap();
        app.show_modelled();
        let scene = app.model.clone().unwrap();
        let units = &scene.project.assemblies;
        assert_eq!(units.iter().map(|a| (a.element, a.number, a.of, a.mark.as_str())).collect::<Vec<_>>(), [(id.0, 1, 3, "Frame 1-1"), (id.0, 2, 3, "Frame 1-2"), (id.0, 3, 3, "Frame 1-3")]);
        // together they are the element's triangles, one after the other
        let (li, &(_, first, count)) = scene.layers.iter().enumerate().find_map(|(li, l)| l.elements.iter().find(|e| e.0 == id.0).map(|e| (li, e))).unwrap();
        assert!(units.iter().all(|a| a.layer == li && a.count > 0));
        assert_eq!((units[0].first, units[1].first, units[2].first + units[2].count), (first, units[0].first + units[0].count, first + count));
        assert!(scene.element_props(id.0).iter().any(|(k, v)| k == "Assemblies" && v == "Frame 1-1, Frame 1-2, Frame 1-3"));

        // IFCX: the frame without a mesh, its three assemblies under it with theirs
        let bytes = crate::export::to_bytes(&scene, &app.document, "frame.ifcx").expect("IFCX");
        let file: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let nodes = file["data"].as_array().unwrap();
        let class = |n: &serde_json::Value| n["attributes"]["bsi::ifc::class"]["code"].as_str().unwrap_or_default().to_string();
        let frame = nodes.iter().find(|n| class(n) == "IfcCurtainWall").unwrap();
        assert!(frame["attributes"].get("usd::usdgeom::mesh").is_none() && frame["attributes"].get("openaec::element").is_some());
        let children: Vec<&str> = frame["children"].as_object().unwrap().values().filter_map(|v| v.as_str()).collect();
        let made: Vec<&serde_json::Value> = nodes.iter().filter(|n| class(n) == "IfcElementAssembly").collect();
        assert_eq!(made.len(), 3);
        for n in &made {
            assert!(children.contains(&n["path"].as_str().unwrap()));
            assert!(!n["attributes"]["usd::usdgeom::mesh"]["points"].as_array().unwrap().is_empty());
            assert_eq!(n["attributes"]["bsi::ifc::prop::Length"], "2.400 m");
        }
        // read back: the one frame, to be edited as before
        let source = crate::loaders::Source::Bytes { name: "frame.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        assert_eq!(back.modeller.model.iter().count(), 1);

        // IFC: the curtain wall without a shape, three assemblies made in the workshop, part of it
        let ifc = String::from_utf8(crate::export::to_bytes(&scene, &app.document, "frame.ifc").expect("IFC")).unwrap();
        let wall = ifc.lines().find(|l| l.contains("=IFCCURTAINWALL(")).unwrap();
        assert!(wall.contains(",$,'1',"), "no shape: {wall}");
        let made: Vec<&str> = ifc.lines().filter(|l| l.contains("=IFCELEMENTASSEMBLY(")).collect();
        assert_eq!(made.len(), 3);
        assert!(made.iter().all(|l| l.contains(".FACTORY.") && l.contains("'Frame 1-")), "{made:?}");
        let number = |l: &str| l.split('=').next().unwrap().to_string();
        let parts = format!("{},({},{},{}))", number(wall), number(made[0]), number(made[1]), number(made[2]));
        assert!(ifc.lines().any(|l| l.contains("=IFCRELAGGREGATES(") && l.ends_with(&format!("{parts};"))), "{parts}");
        // in the storey: the wall, not its assemblies
        let contained = ifc.lines().find(|l| l.contains("=IFCRELCONTAINEDINSPATIALSTRUCTURE(")).unwrap();
        assert!(contained.contains(&number(wall)) && !contained.contains(&number(made[0])), "{contained}");
    }

    #[test]
    fn an_ifcx_file_keeps_the_definitions_and_gives_editable_elements_back() {
        let mut app = App::default();
        app.add_demo_building();
        let written: Vec<Element> = app.modeller.model.iter().cloned().collect();
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document, "house.ifcx").expect("IFCX");
        let text = String::from_utf8(bytes.clone()).unwrap();
        assert!(text.contains("openaec::element") && text.contains("IfcCurtainWall") && text.contains("IfcWall"));
        assert!(!text.contains(crate::export::ifcx::DEFINITION), "the definition is an attribute, not a property");
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        let read: Vec<Element> = back.modeller.model.iter().cloned().collect();
        assert_eq!(read, written, "every definition comes back exactly");
        // shown once: from the definitions, not also as the file's meshes
        let shown = back.model.as_ref().unwrap();
        let walls = shown.layers.iter().filter(|l| l.name.ends_with("IfcWall")).map(|l| l.elements.len()).sum::<usize>();
        assert_eq!(walls, 3);
        // and editable: a new wall gets the next id
        back.set_model_tool(Some((ModelTool::Wall, "Wall")));
        let id = back.add_element("Wall", None, Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::new(0.0, 10.0), end: DVec2::new(4.0, 10.0), base: 0.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None })).unwrap();
        assert!(id.0 > written.iter().map(|e| e.id.0).max().unwrap());
    }

    /// A wall of a level made in the app still belongs to that level after the project is saved
    /// and opened again.
    #[test]
    fn a_wall_keeps_its_level_when_the_project_is_opened_again() {
        let mut app = modelling();
        let _ = app.document.add_level("Level 1", 3.0);
        let level = app.document.levels().iter().find(|l| l.name == "Level 1").unwrap().path.clone();
        let wall = Shape::Planar(PlanarDef { form: PlanarForm::Wall { start: DVec2::ZERO, end: DVec2::new(5.0, 0.0), base: 3.0, height: 3.0 }, thickness: 0.2, openings: Vec::new(), build_up: None });
        let id = app.add_element("Wall", Some(level), wall).unwrap();
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document, "house.ifcx").expect("IFCX");
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        let names: Vec<&str> = back.document.levels().iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["Level 0", "Level 1"]);
        let kept = back.modeller.model.get(id).and_then(|e| e.level.clone()).expect("the wall names a level");
        let of = back.document.level(&kept).map(|l| l.name.as_str());
        assert_eq!(of, Some("Level 1"), "the wall's level is one of the project's");
    }

    #[test]
    fn the_first_selected_element_cuts_the_second() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 6.0, 0.0);
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 3.0, -2.0);
        click(&mut app, 3.0, 2.0);
        app.set_model_tool(None);
        let (wall, beam) = (ElementId(1), ElementId(2));
        let before = app.modeller.model.evaluate(wall).unwrap()[0].solid.volume();
        app.select_modelled(beam);
        let scene = app.model.clone().unwrap();
        let shown = scene.element_with_key(crate::model::ElementKey { model: 0, source: wall.0 }).unwrap();
        app.sel.toggle(Pickable::Element(shown));
        assert_eq!(app.selected_bodies(), 2);
        app.connect_selected(ConnectionKind::Cut, "Cut-out");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        let after = app.modeller.model.evaluate(wall).unwrap()[0].solid.volume();
        assert!((before - after - 0.2 * 0.4 * 0.2).abs() < 1e-9, "the beam's section through the wall: {before} - {after}");
    }

    #[test]
    fn beams_drawn_corner_to_corner_are_joined_and_picked_by_their_mark() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Beam, "Beam")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        click(&mut app, 4.0, 3.0);
        app.set_model_tool(None);
        let joins: Vec<&Element> = app.modeller.model.iter().filter(|e| e.base() == bim_core::BaseKind::Connection).collect();
        assert_eq!(joins.len(), 1, "the chained beams meet at a corner");
        let joint = joins[0].id;
        assert!(matches!(&joins[0].shape, Shape::Connection(c) if c.kind == ConnectionKind::Mitre));
        // its mark at the corner, at the beams' height
        let marks = app.join_marks(iced::Color::BLACK, iced::Color::WHITE).expect("a mark");
        assert_eq!(marks.marks.len(), 1);
        assert!((marks.marks[0].1 - Vec3::new(4.0, 0.0, 2.8)).length() < 1e-4, "{}", marks.marks[0].1);
        // a click on the mark picks the join; its kind can then change
        let _ = app.update(Message::PickJoin(joint));
        assert_eq!(app.selected_modelled(), vec![joint]);
        // the end plate on the same two beams changes that join, it adds none
        let scene = app.model.clone().unwrap();
        app.sel.clear();
        for id in [ElementId(1), ElementId(2)] {
            let shown = scene.element_with_key(crate::model::ElementKey { model: 0, source: id.0 }).unwrap();
            app.sel.toggle(Pickable::Element(shown));
        }
        app.connect_selected(ConnectionKind::Butt, "Joint");
        let joins: Vec<&Element> = app.modeller.model.iter().filter(|e| e.base() == bim_core::BaseKind::Connection).collect();
        assert_eq!(joins.len(), 1);
        assert!(matches!(&joins[0].shape, Shape::Connection(c) if c.kind == ConnectionKind::Butt));
    }

    #[test]
    fn two_timber_frames_are_connected_at_their_corner() {
        let mut app = modelling();
        let frame = |a: (f64, f64), b: (f64, f64)| {
            let s = SystemDef { start: glam::DVec2::new(a.0, a.1), end: glam::DVec2::new(b.0, b.1), base: 0.0, height: 2.7, spacing: SYSTEM_SPACING, member: SYSTEM_MEMBER, panel: SYSTEM_PANEL, system_type: None, openings: Vec::new(), breaks: Vec::new(), outline: Vec::new(), grid: None, division: None };
            Shape::System(s.typed_from(&bim_library::Library::starter(), "system.timber-frame-140").unwrap())
        };
        let long = app.add_element("Frame", None, frame((0.0, 0.0), (4.8, 0.0))).unwrap();
        let short = app.add_element("Frame", None, frame((4.8, 0.0), (4.8, 3.0))).unwrap();
        let through = |app: &App| bim_core::system::corners(&app.modeller.model, app.modeller.model.get(short).unwrap())[0].map(|m| m.through);
        assert_eq!(through(&app), Some(false), "the longer one runs through");
        // the short one picked first: it runs through
        let scene = app.model.clone().unwrap();
        app.sel.clear();
        for id in [short, long] {
            app.sel.toggle(Pickable::Element(scene.element_with_key(crate::model::ElementKey { model: 0, source: id.0 }).unwrap()));
        }
        assert!(app.selected_joinable());
        app.connect_selected(ConnectionKind::Butt, "Connection");
        assert!(app.modeller.refusal.is_none(), "{:?}", app.modeller.refusal);
        assert_eq!(through(&app), Some(true));
        // its mark at the corner picks it; turned round, the long one runs through again
        let marks = app.join_marks(iced::Color::BLACK, iced::Color::WHITE).expect("a mark");
        assert!((marks.marks[0].1 - Vec3::new(4.8, 0.0, 1.35)).length() < 1e-4, "{}", marks.marks[0].1);
        let _ = app.update(Message::PickJoin(marks.marks[0].0));
        let _ = app.update(Message::SwapJoin);
        assert_eq!(through(&app), Some(false));
        // apart: no corner
        let _ = app.update(Message::JoinKind(ConnectionKind::Apart));
        assert_eq!(through(&app), None);
        // a frame and a beam are not joined
        app.sel.clear();
        assert!(!app.selected_joinable());
    }

    #[test]
    fn a_saved_project_opens_again_as_one_model_under_its_name() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 4.0, 0.0);
        app.set_model_tool(None);
        let scene = app.model.clone().unwrap();
        let bytes = crate::export::to_bytes(&scene, &app.document_now(), "house.ifcx").unwrap();
        let source = crate::loaders::Source::Bytes { name: "house.ifcx".into(), data: Arc::new(bytes) };
        let loaded = crate::loaders::load_with(&source, &crate::loaders::Progress::default()).expect("read back");
        let mut back = App::default();
        back.set_model(Arc::new(loaded));
        // the project, not an empty file with a model beside it
        assert_eq!(back.models.len(), 1);
        assert_eq!(back.modeller.model.len(), 1);
        let shown = back.model.clone().unwrap();
        assert_eq!(shown.path.file_name().and_then(|n| n.to_str()), Some("house.ifcx"));
        assert!(back.title().starts_with("house.ifcx"), "{}", back.title());
        // and it stays so after a change
        back.set_model_tool(Some((ModelTool::Wall, "Wall")));
        back.viewport_size = (1000.0, 800.0);
        let again = back.modeller.model.iter().next().unwrap().shape.clone();
        let _ = back.add_element("Wall", None, again);
        assert_eq!(back.models.len(), 1);
        assert_eq!(back.model.as_ref().unwrap().path.file_name().and_then(|n| n.to_str()), Some("house.ifcx"));
    }

    #[test]
    fn a_refusal_is_kept_to_show() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 1.0, 1.0);
        click(&mut app, 1.0, 1.0);
        assert_eq!(app.modeller.refusal, Some(Refusal::ZeroLength));
        assert!(app.modeller.model.is_empty());
    }

    #[test]
    fn modelled_elements_show_their_definition_as_properties() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Wall, "Wall")));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        let scene = app.model.clone().unwrap();
        let id = scene.layers.iter().flat_map(|l| &l.elements).next().unwrap().0;
        let props: std::collections::HashMap<String, String> = scene.element_props(id).into_iter().collect();
        assert_eq!(props["class"], "IfcWall");
        assert_eq!(props["Length"], "5.000 m");
        assert_eq!(props["Base element"], "Planar");
        assert_eq!(props["Volume"], "3.000 m³");
    }
}
