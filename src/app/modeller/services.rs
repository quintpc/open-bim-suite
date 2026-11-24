//! Building services on their appliances: the connectors of a boiler, a radiator or a basin are
//! points a run lands on, a run drawn at another height gets its riser to the connector, and what
//! is connected is a network that is selected as one.

use crate::app::{prop_row, App, Message};
use crate::selection::Pickable;
use crate::theme::Palette;
use bim_core::{Connector, Element, LinearDef, Shape};
use iced::widget::{button, text, Column};
use openaec_ui::design::a11y;
use openaec_ui::Language;

use super::ModelTool;

/// How near in plan a run's end is to a connector to be on it (metres).
const ON: f64 = 1e-3;

impl App {
    /// The service the runs of `tool` are drawn in, when the strip has one of its kind.
    pub(in crate::app) fn tool_service(&self, tool: ModelTool) -> Option<bim_core::Service> {
        self.modeller.placement.of(tool).service.filter(|v| tool == ModelTool::Appliance || Some(v.role()) == tool.service())
    }

    /// The open connectors a run of `tool` can land on: those of its kind of run and of its
    /// service, no run on them yet.
    pub(in crate::app) fn open_connectors(&self, tool: ModelTool) -> Vec<Connector> {
        let Some(role) = tool.service() else { return Vec::new() };
        let service = self.tool_service(tool);
        let model = &self.modeller.model;
        model
            .iter()
            .filter(|e| matches!(e.shape, Shape::Point(_)))
            .flat_map(|e| model.connectors(e.id))
            .filter(|(c, run)| run.is_none() && c.role == role && (service.is_none() || c.service.is_none() || c.service == service))
            .map(|(c, _)| c)
            .collect()
    }

    /// The risers a run just drawn needs to the connectors its ends are over or under: a
    /// vertical run of its profile from each such end to the connector. Taken before the run is
    /// added, so the connectors are still open.
    pub(in crate::app) fn risers_to_connectors(&self, tool: ModelTool, shape: &Shape) -> Vec<Shape> {
        let Shape::Linear(run) = shape else { return Vec::new() };
        let open = self.open_connectors(tool);
        [run.start, run.end]
            .into_iter()
            .filter_map(|end| {
                let c = open.iter().find(|c| (c.at - end).truncate().length() < ON)?;
                ((c.at.z - end.z).abs() > 1e-6).then(|| Shape::Linear(LinearDef { start: end, end: c.at, profile: run.profile.clone(), roll: 0.0, role: run.role }))
            })
            .collect()
    }

    /// Selects the network of the selected element: every run, fitting and appliance connected
    /// to it.
    pub(in crate::app) fn select_network(&mut self) {
        let Some(id) = self.selected_modelled().first().copied() else { return };
        let network = self.modeller.model.network(id);
        if network.len() < 2 {
            return;
        }
        let shown: Vec<Pickable> = network.iter().filter_map(|id| self.shown_modelled(*id)).map(Pickable::Element).collect();
        self.sel.clear();
        self.sel.extend(shown);
        self.selection_changed();
    }

    /// The rows of an element of the building services: the connectors of an appliance, each
    /// connected or open, and the network it is part of with a button that selects it.
    pub(in crate::app) fn service_rows<'a>(&self, e: &Element, mut rows: Column<'a, Message>, p: Palette, l: Language) -> Column<'a, Message> {
        let model = &self.modeller.model;
        for (c, run) in model.connectors(e.id) {
            let what = match c.service {
                Some(service) => crate::app::placement::service_label(service, l),
                None if c.role == bim_core::LinearRole::Duct => l.text("Kanaal", "Duct"),
                None => l.text("Leiding", "Pipe"),
            };
            let state = match run.and_then(|id| model.get(id)) {
                Some(run) => run.name.clone(),
                None => l.text("open", "open").to_string(),
            };
            rows = rows.push(prop_row(p, format!("{} {}", l.text("Aansluiting", "Connector"), what.to_lowercase()), state));
        }
        let network = model.network(e.id);
        if network.len() < 2 {
            return rows;
        }
        let count = |of: fn(&Shape) -> bool| network.iter().filter(|id| model.get(**id).is_some_and(|e| of(&e.shape))).count();
        let (runs, fittings, appliances) = (count(|s| matches!(s, Shape::Linear(_))), count(|s| matches!(s, Shape::Connection(_))), count(|s| matches!(s, Shape::Point(_))));
        let length: f64 = network.iter().filter_map(|id| model.get(*id)).filter_map(|e| if let Shape::Linear(l) = &e.shape { Some((l.end - l.start).length()) } else { None }).sum();
        rows = rows
            .push(prop_row(p, l.text("Netwerk", "Network").to_string(), format!("{runs} {} · {fittings} {} · {appliances} {}", l.text("leidingen", "runs"), l.text("hulpstukken", "fittings"), l.text("toestellen", "appliances"))))
            .push(prop_row(p, l.text("Lengte netwerk", "Network length").to_string(), format!("{length:.2} m")));
        let label = l.text("Netwerk selecteren", "Select network");
        rows.push(a11y::named(button(text(label).size(12)).padding([3, 10]).style(move |_, s| p.row_button(s)).on_press(Message::SelectNetwork), a11y::Role::Button, label))
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, modelling};
    use super::*;
    use crate::app::placement::PlaceMessage;
    use bim_core::{PointRole, Service};
    use glam::DVec3;

    #[test]
    fn a_pipe_lands_on_the_connector_of_its_service_and_gets_a_riser_to_it() {
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Appliance, "Appliance")));
        click(&mut app, 2.0, 3.0);
        let radiator = app.modeller.model.iter().next().unwrap().id;
        // the supply pipe sees the supply connector only, and a click near it lands on it
        app.set_model_tool(Some((ModelTool::Pipe, "Pipe")));
        app.place_message(PlaceMessage::Service(Some(Service::HeatingSupply)));
        let open = app.open_connectors(ModelTool::Pipe);
        assert_eq!(open.len(), 1);
        assert!((open[0].at - DVec3::new(1.5, 3.0, 0.21)).length() < 1e-9, "{:?}", open[0].at);
        click(&mut app, 1.53, 3.04);
        click(&mut app, 1.5, 8.0);
        let lines = |app: &App| -> Vec<(DVec3, DVec3)> {
            app.modeller.model.iter().filter_map(|e| if let Shape::Linear(l) = e.shape_in_project(&app.modeller.model.grid) { Some((l.start, l.end)) } else { None }).collect()
        };
        let h = super::super::SERVICE_HEIGHT;
        assert_eq!(lines(&app), [(DVec3::new(1.5, 3.0, h), DVec3::new(1.5, 8.0, h)), (DVec3::new(1.5, 3.0, h), DVec3::new(1.5, 3.0, 0.21))]);
        assert_eq!(app.modeller.model.iter().filter(|e| e.class() == "IfcPipeFitting").count(), 1, "a bend at the top of the riser");
        // the return pipe ends over its connector: a riser down, and the chain stops
        app.modeller.points.clear();
        app.place_message(PlaceMessage::Service(Some(Service::HeatingReturn)));
        assert!(app.open_connectors(ModelTool::Pipe).iter().all(|c| c.service == Some(Service::HeatingReturn)));
        click(&mut app, 6.0, 3.0);
        click(&mut app, 2.5, 3.0);
        assert!(app.modeller.points.is_empty());
        assert_eq!(lines(&app).len(), 4);
        assert!(app.modeller.model.connectors(radiator).iter().all(|(_, run)| run.is_some()));
        assert!(app.open_connectors(ModelTool::Pipe).is_empty());
        // the radiator's network is all of it, selected as one
        app.set_model_tool(None);
        app.select_modelled(radiator);
        app.select_network();
        assert_eq!(app.selected_modelled().len(), 7, "a radiator, four pipes and two bends");
        assert_eq!(PointRole::Radiator.class(), "IfcSpaceHeater");
    }

    #[test]
    fn a_waste_pipe_falls_as_it_runs_and_the_next_goes_on_from_where_it_ended() {
        use crate::app::placement::PlaceField;
        let mut app = modelling();
        app.set_model_tool(Some((ModelTool::Pipe, "Pipe")));
        app.place_message(PlaceMessage::Service(Some(Service::Waste)));
        app.place_message(PlaceMessage::Field(PlaceField::Height, "1".into()));
        // typed as 1:50 or as 50
        app.place_message(PlaceMessage::Field(PlaceField::Sill, "1:50".into()));
        click(&mut app, 0.0, 0.0);
        click(&mut app, 5.0, 0.0);
        click(&mut app, 5.0, 10.0);
        let ends: Vec<(f64, f64)> = app.modeller.model.iter().filter_map(|e| if let Shape::Linear(l) = &e.shape { Some((l.start.z, l.end.z)) } else { None }).collect();
        assert_eq!(ends.len(), 2);
        assert!((ends[0].0 - 1.0).abs() < 1e-9 && (ends[0].1 - 0.9).abs() < 1e-9, "{ends:?}");
        assert!((ends[1].0 - 0.9).abs() < 1e-9 && (ends[1].1 - 0.7).abs() < 1e-9, "{ends:?}");
        assert_eq!(app.modeller.model.iter().filter(|e| e.class() == "IfcPipeFitting").count(), 1, "a bend where it turns");
        // its fall shows in its properties and is typed there: 1:100 keeps the high end
        app.set_model_tool(None);
        let first = app.modeller.model.iter().next().unwrap().id;
        app.select_modelled(first);
        let e = app.defined().unwrap().clone();
        assert_eq!(app.element_field_text(&e, crate::app::element_props::ElementField::Fall), "50");
        app.type_element_field(crate::app::element_props::ElementField::Fall, "100".into());
        let Shape::Linear(l) = &app.modeller.model.get(first).unwrap().shape else { panic!() };
        assert!((l.start.z - 1.0).abs() < 1e-9 && (l.end.z - 0.95).abs() < 1e-9);
    }
}
