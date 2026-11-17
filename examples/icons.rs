//! Writes `icons/<entry>.svg`: each entry's rotor seen along its axis, drawn from the plugin's
//! own geometry (the blade planform is the outline of every section's leading and trailing
//! edge). 64 × 64 view box, artwork inside 5…59, 2.55-unit round lines, `currentColor` only.
//!
//! cargo run --example icons
use propellers::blade::{bounds, placed};
use propellers::design::Rotor;
use propellers::{design, toroid_design, Kind};
use serde_json::json;
use std::f64::consts::PI;
use std::fmt::Write;

const STROKE: f64 = 2.55;

fn path(pts: &[[f64; 2]], closed: bool) -> String {
    let mut d = String::new();
    for (i, p) in pts.iter().enumerate() {
        let _ = write!(d, "{}{:.2} {:.2}", if i == 0 { "M" } else { " L" }, p[0], p[1]);
    }
    if closed {
        d.push_str(" Z");
    }
    d
}

fn circle(cx: f64, cy: f64, r: f64) -> String {
    format!(r#"<circle cx="{cx:.2}" cy="{cy:.2}" r="{r:.2}" fill="none" stroke="currentColor" stroke-width="{STROKE}"/>"#)
}

/// The blade's outline seen along the axis: (radial, tangential) of each section's extremes.
fn planform(rotor: &Rotor) -> Vec<[f64; 2]> {
    let mut lead = Vec::new();
    let mut trail = Vec::new();
    for s in &rotor.stations {
        let (y0, y1, _, _) = bounds(&placed(s, 12, 0.0, rotor.suction_up, rotor.mirror));
        lead.push([s.r, y1]);
        trail.push([s.r, y0]);
    }
    // Start just inside the hub so the outline meets its circle.
    let rh = rotor.hub_radius * 0.9;
    let mut out = vec![[rh, trail[0][1]]];
    out.extend(trail);
    out.extend(lead.into_iter().rev());
    out.push([rh, out[out.len() - 1][1]]);
    out
}

fn icon(kind: Kind, params: serde_json::Value, blades: u32, duct: bool) -> String {
    let (rotor, _) = design(kind, &params).expect("defaults build");
    let outer = if duct { rotor.radius * 1.16 } else { rotor.radius };
    // Fit the rotor (and duct) into 5…59 with half a line to spare.
    let scale = (27.0 - STROKE / 2.0) / outer;
    let blade = planform(&rotor);
    let mut body = String::new();
    for k in 0..blades {
        // Turned so the picture sits square: the first blade points up.
        let a = PI / 2.0 + 2.0 * PI * f64::from(k) / f64::from(blades);
        let (s, c) = a.sin_cos();
        let pts: Vec<[f64; 2]> = blade
            .iter()
            .map(|p| {
                // Radial along (c, s), tangential along (-s, c); SVG y points down.
                let (x, y) = (p[0] * c - p[1] * s, p[0] * s + p[1] * c);
                [32.0 + scale * x, 32.0 - scale * y]
            })
            .collect();
        let _ = write!(
            body,
            r#"<path d="{}" fill="none" stroke="currentColor" stroke-width="{STROKE}" stroke-linejoin="round" stroke-linecap="round"/>"#,
            path(&pts, false)
        );
    }
    body.push_str(&circle(32.0, 32.0, scale * rotor.hub_radius));
    if duct {
        body.push_str(&circle(32.0, 32.0, scale * rotor.radius * 1.1));
    }
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">{body}</svg>"#)
}

/// A toroidal propeller seen along its axis: each loop's outline, from the extremes of every
/// section across the loop's centreline.
fn toroid_icon(marine: bool, params: serde_json::Value) -> String {
    let t = toroid_design(marine, &params).expect("defaults");
    let l = t.design().expect("defaults build");
    let scale = (27.0 - STROKE / 2.0) / (t.diameter / 2.0);
    let world: Vec<Vec<[f64; 2]>> =
        l.sections.iter().map(|((o, x, y), pts)| pts.iter().map(|p| [o[0] + p[0] * x[0] + p[1] * y[0], o[1] + p[0] * x[1] + p[1] * y[1]]).collect()).collect();
    let origins: Vec<[f64; 2]> = l.sections.iter().map(|((o, ..), _)| [o[0], o[1]]).collect();
    let (mut outer, mut inner) = (Vec::new(), Vec::new());
    for (k, pts) in world.iter().enumerate() {
        let (a, b) = (origins[k.saturating_sub(1)], origins[(k + 1).min(origins.len() - 1)]);
        let (tx, ty) = (b[0] - a[0], b[1] - a[1]);
        let n = [-ty, tx];
        let key = |p: &[f64; 2]| p[0] * n[0] + p[1] * n[1];
        let hi = pts.iter().copied().fold(pts[0], |m, p| if key(&p) > key(&m) { p } else { m });
        let lo = pts.iter().copied().fold(pts[0], |m, p| if key(&p) < key(&m) { p } else { m });
        outer.push(hi);
        inner.push(lo);
    }
    let mut body = String::new();
    for k in 0..t.loops {
        let a = PI / 2.0 + 2.0 * PI * f64::from(k) / f64::from(t.loops);
        let (s, c) = a.sin_cos();
        let place = |p: &[f64; 2]| [32.0 + scale * (p[0] * c - p[1] * s), 32.0 - scale * (p[0] * s + p[1] * c)];
        for side in [&outer, &inner] {
            let pts: Vec<[f64; 2]> = side.iter().map(place).collect();
            let _ = write!(
                body,
                r#"<path d="{}" fill="none" stroke="currentColor" stroke-width="{STROKE}" stroke-linejoin="round" stroke-linecap="round"/>"#,
                path(&pts, false)
            );
        }
    }
    body.push_str(&circle(32.0, 32.0, scale * l.hub_radius));
    format!(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64">{body}</svg>"#)
}

fn main() {
    let icons = [
        ("propeller", icon(Kind::Propeller, json!({"max_chord": 34, "root_chord": 20, "tip_chord": 14, "hub_diameter": 30}), 2, false)),
        ("marine", icon(Kind::Marine, json!({"skew": 30}), 3, false)),
        ("wind-turbine", icon(Kind::WindTurbine, json!({"design_cl": 0.5, "hub_diameter": 160}), 3, false)),
        ("ducted-fan", icon(Kind::DuctedFan, json!({"blades": 6, "hub_diameter": 26}), 6, true)),
        ("toroidal", toroid_icon(false, json!({"root_chord": 30, "tip_chord": 28, "hub_diameter": 44, "spread": 0.6}))),
        ("toroidal-marine", toroid_icon(true, json!({"root_chord": 40, "tip_chord": 40, "hub_diameter": 70, "spread": 0.7}))),
    ];
    std::fs::create_dir_all("icons").expect("icons folder");
    for (name, svg) in icons {
        std::fs::write(format!("icons/{name}.svg"), svg + "\n").expect("write icon");
        println!("icons/{name}.svg");
    }
}
