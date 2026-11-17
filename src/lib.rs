//! SolidCraft propeller plugin: air propellers, marine propellers, wind turbine rotors and
//! ducted fans. Each entry works out the blade's sections and builds the rotor with native
//! sketches, an extrude for the hub and one loft per blade, so the result is ordinary editable
//! features.

pub mod blade;
pub mod design;
pub mod toroid;

use blade::{bounds, placed, self_intersects, Station};
use design::{pitch_at_75, Detail, Duct, DuctedFan, Marine, Propeller, Rotor, WindTurbine};
use serde_json::{json, Value};
use std::f64::consts::PI;

/// A failure reported back to the host, mirroring the WIT `plugin-error` cases we use.
#[derive(Debug)]
pub enum Failure {
    Input(&'static str, String),
    Command(String, String),
    Cancelled,
}

impl From<(&'static str, String)> for Failure {
    fn from((f, r): (&'static str, String)) -> Self {
        Failure::Input(f, r)
    }
}

/// The host calls the plugin needs. Lets `build` run in native tests with a fake host.
pub trait Host {
    fn run(&mut self, command: &str, args: Value) -> Result<Value, Failure>;
    fn progress(&mut self, fraction: f32, message: &str);
    fn cancelled(&mut self) -> bool;
}

fn num(params: &Value, key: &'static str, default: f64) -> Result<f64, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_f64().filter(|x| x.is_finite()).ok_or_else(|| Failure::Input(key, "must be a number".into())),
    }
}

fn int(params: &Value, key: &'static str, default: u32) -> Result<u32, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v.as_u64().and_then(|x| u32::try_from(x).ok()).ok_or_else(|| Failure::Input(key, "must be a whole number".into())),
    }
}

/// One of `options` (the first is the default); returns its index.
fn choice(params: &Value, key: &'static str, options: &[&str]) -> Result<usize, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(v) => {
            v.as_str().and_then(|s| options.iter().position(|o| *o == s)).ok_or_else(|| Failure::Input(key, format!("must be one of {}", options.join(", "))))
        }
    }
}

fn flag(params: &Value, key: &'static str, default: bool) -> Result<bool, Failure> {
    match params.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(Value::Bool(b)) => Ok(*b),
        Some(_) => Err(Failure::Input(key, "must be true or false".into())),
    }
}

/// The plugin's entries.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Propeller,
    Marine,
    WindTurbine,
    DuctedFan,
    Toroidal,
    ToroidalMarine,
}

impl Kind {
    pub fn from_entry(entry: &str) -> Option<Kind> {
        match entry {
            "propeller" => Some(Kind::Propeller),
            "marine" => Some(Kind::Marine),
            "wind-turbine" => Some(Kind::WindTurbine),
            "ducted-fan" => Some(Kind::DuctedFan),
            "toroidal" => Some(Kind::Toroidal),
            "toroidal-marine" => Some(Kind::ToroidalMarine),
            _ => None,
        }
    }
}

/// How each section's outline is drawn.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Curve {
    /// One fit point spline round the leading edge, and a line across the trailing edge.
    Spline,
    /// A closed polyline.
    Polyline,
}

fn detail(params: &Value, sections: u32, points: u32, te: f64, mirror_word: &str) -> Result<Detail, Failure> {
    Ok(Detail {
        sections: int(params, "sections", sections)?,
        points: int(params, "section_points", points)?,
        trailing_edge: num(params, "trailing_edge", te)?,
        mirror: rotation(params)? == mirror_word,
    })
}

fn rotation(params: &Value) -> Result<&'static str, Failure> {
    match params.get("rotation").and_then(Value::as_str) {
        None => Ok("default"),
        Some("ccw") => Ok("ccw"),
        Some("cw") => Ok("cw"),
        Some("right") => Ok("right"),
        Some("left") => Ok("left"),
        Some(_) => Err(Failure::Input("rotation", "must be ccw or cw (right or left for a marine propeller)".into())),
    }
}

/// The rotor an entry's inputs describe, plus facts only that kind knows.
pub fn design(kind: Kind, p: &Value) -> Result<(Rotor, Value), Failure> {
    match kind {
        Kind::Toroidal | Kind::ToroidalMarine => Err(Failure::Input("entry", "a toroidal propeller has loops, not blades: see toroid_design".into())),
        Kind::Propeller => {
            if !matches!(rotation(p)?, "default" | "ccw" | "cw") {
                return Err(Failure::Input("rotation", "must be ccw or cw".into()));
            }
            let pr = Propeller {
                blades: int(p, "blades", 2)?,
                diameter: num(p, "diameter", 254.0)?,
                pitch: num(p, "pitch", 114.3)?,
                hub_diameter: num(p, "hub_diameter", 0.0)?,
                hub_length: num(p, "hub_length", 0.0)?,
                bore: num(p, "bore", 5.0)?,
                root_chord: num(p, "root_chord", 0.0)?,
                max_chord: num(p, "max_chord", 0.0)?,
                max_chord_at: num(p, "max_chord_at", 0.35)?,
                tip_chord: num(p, "tip_chord", 0.0)?,
                root_thickness: num(p, "root_thickness", 0.16)?,
                tip_thickness: num(p, "tip_thickness", 0.08)?,
                camber: num(p, "camber", 4.0)?,
                camber_pos: num(p, "camber_position", 0.4)?,
                detail: detail(p, 12, 16, 0.3, "cw")?,
            };
            let rotor = pr.rotor()?;
            let rpm = num(p, "rpm", 0.0)?;
            let mut facts = json!({
                "rotation": if rotor.mirror { "cw" } else { "ccw" },
                "rotation_note": "seen from the front (+Z, the way the thrust points); the propeller pushes air towards -Z",
                "pitch_mm": pr.pitch,
                "pitch_inches": pr.pitch / 25.4,
                "diameter_inches": pr.diameter / 25.4,
                "pitch_ratio": pr.pitch / pr.diameter,
            });
            if rpm > 0.0 {
                facts["rpm"] = json!(rpm);
                facts["tip_speed_m_s"] = json!(PI * pr.diameter / 1000.0 * rpm / 60.0);
                facts["pitch_speed_m_s"] = json!(pr.pitch / 1000.0 * rpm / 60.0);
                facts["speed_note"] = json!("pitch speed is how fast the propeller would advance with no slip; real flight speed is lower");
            }
            Ok((rotor, facts))
        }
        Kind::DuctedFan => {
            if !matches!(rotation(p)?, "default" | "ccw" | "cw") {
                return Err(Failure::Input("rotation", "must be ccw or cw".into()));
            }
            let duct = if flag(p, "duct", true)? {
                Some(Duct { clearance: num(p, "tip_clearance", 0.3)?, wall: num(p, "duct_wall", 1.5)?, length: num(p, "duct_length", 0.0)? })
            } else {
                None
            };
            let f = DuctedFan {
                blades: int(p, "blades", 7)?,
                diameter: num(p, "diameter", 64.0)?,
                pitch: num(p, "pitch", 60.0)?,
                hub_diameter: num(p, "hub_diameter", 0.0)?,
                hub_length: num(p, "hub_length", 0.0)?,
                bore: num(p, "bore", 2.0)?,
                root_chord: num(p, "root_chord", 0.0)?,
                tip_chord: num(p, "tip_chord", 0.0)?,
                root_thickness: num(p, "root_thickness", 0.12)?,
                tip_thickness: num(p, "tip_thickness", 0.08)?,
                camber: num(p, "camber", 4.0)?,
                duct,
                detail: detail(p, 8, 10, 0.25, "cw")?,
            };
            let rotor = f.rotor()?;
            Ok((
                rotor.clone(),
                json!({
                    "rotation": if rotor.mirror { "cw" } else { "ccw" },
                    "rotation_note": "seen from the front (+Z), the intake side; the fan pushes air towards -Z",
                    "pitch_mm": f.pitch,
                }),
            ))
        }
        Kind::Marine => {
            let mirror = match rotation(p)? {
                "default" | "right" => false,
                "left" => true,
                _ => return Err(Failure::Input("rotation", "must be right or left".into())),
            };
            let m = Marine {
                blades: int(p, "blades", 3)?,
                diameter: num(p, "diameter", 250.0)?,
                pitch: num(p, "pitch", 250.0)?,
                area_ratio: num(p, "area_ratio", 0.55)?,
                skew: num(p, "skew", 15.0)?,
                rake: num(p, "rake", 10.0)?,
                hub_diameter: num(p, "hub_diameter", 0.0)?,
                hub_length: num(p, "hub_length", 0.0)?,
                bore: num(p, "bore", 0.0)?,
                camber: num(p, "camber", 2.0)?,
                thickness_scale: num(p, "thickness_scale", 1.0)?,
                detail: Detail { mirror, ..detail(p, 12, 16, 0.5, "")? },
            };
            let rotor = m.rotor()?;
            Ok((
                rotor,
                json!({
                    "rotation": if mirror { "left" } else { "right" },
                    "rotation_note": "right hand turns clockwise seen from astern (from -Z) when going ahead; ahead is +Z",
                    "pitch_mm": m.pitch,
                    "pitch_inches": m.pitch / 25.4,
                    "diameter_inches": m.diameter / 25.4,
                    "pitch_ratio": m.pitch / m.diameter,
                    "expanded_area_ratio": m.area_ratio,
                    "skew_deg": m.skew,
                    "rake_deg": m.rake,
                    "outline_note": "Wageningen B-series chord and thickness; the outline stops at 0.99 R with a short flat tip",
                }),
            ))
        }
        Kind::WindTurbine => {
            let mirror = match rotation(p)? {
                "default" | "cw" => true,
                "ccw" => false,
                _ => return Err(Failure::Input("rotation", "must be cw or ccw".into())),
            };
            let blade_only = choice(p, "build", &["rotor", "blade"])? == 1;
            let w = WindTurbine {
                blades: int(p, "blades", 3)?,
                diameter: num(p, "diameter", 1000.0)?,
                tip_speed_ratio: num(p, "tip_speed_ratio", 6.0)?,
                design_cl: num(p, "design_cl", 1.0)?,
                angle_of_attack: num(p, "angle_of_attack", 6.0)?,
                hub_diameter: num(p, "hub_diameter", 0.0)?,
                hub_length: num(p, "hub_length", 0.0)?,
                bore: num(p, "bore", 0.0)?,
                root_diameter: num(p, "root_diameter", 0.0)?,
                root_transition: num(p, "root_transition", 0.25)?,
                root_thickness: num(p, "root_thickness", 0.21)?,
                tip_thickness: num(p, "tip_thickness", 0.12)?,
                camber: num(p, "camber", 4.0)?,
                blade_only,
                detail: Detail { mirror, ..detail(p, 12, 16, 0.5, "")? },
            };
            let rotor = w.rotor()?;
            let v = num(p, "wind_speed", 8.0)?;
            if !(v > 0.0 && v <= 100.0) {
                return Err(Failure::Input("wind_speed", "must be more than 0 and at most 100 m/s".into()));
            }
            let d_m = w.diameter / 1000.0;
            let area = PI * d_m * d_m / 4.0;
            Ok((
                rotor,
                json!({
                    "rotation": if mirror { "cw" } else { "ccw" },
                    "rotation_note": "seen from upwind (+Z); the wind blows towards -Z",
                    "tip_speed_ratio": w.tip_speed_ratio,
                    "design_cl": w.design_cl,
                    "angle_of_attack_deg": w.angle_of_attack,
                    "wind_speed_m_s": v,
                    "design_rpm": w.tip_speed_ratio * v / (PI * d_m) * 60.0,
                    "power_estimate_w": 0.5 * 1.225 * area * v.powi(3) * 0.4,
                    "power_note": "a rough figure: 40% of the wind's power through the rotor disc at wind_speed (Betz's limit is 59%); real small rotors get 25-40%",
                    "design_note": "chord and twist from Schmitz's optimum (Betz with wake rotation) for the design lift coefficient and angle of attack of the airfoil",
                }),
            ))
        }
    }
}

/// Micrometre precision keeps the JSON small.
fn round(x: f64) -> f64 {
    // `+ 0.0` turns -0.0 into 0.0.
    (x * 1e6).round() / 1e6 + 0.0
}

fn pt2(p: [f64; 2]) -> Value {
    json!([round(p[0]), round(p[1])])
}

fn pt3(p: [f64; 3]) -> Value {
    json!([round(p[0]), round(p[1]), round(p[2])])
}

/// Sketch one closed section on a plane given as `{origin, x_dir, y_dir}`.
fn sketch_section(host: &mut dyn Host, name: &str, plane: ([f64; 3], [f64; 3], [f64; 3]), pts: &[[f64; 2]], curve: Curve) -> Result<(), Failure> {
    host.run("sketch.create", json!({"name": name, "plane": {"origin": pt3(plane.0), "x_dir": pt3(plane.1), "y_dir": pt3(plane.2)}}))?;
    let list: Vec<Value> = pts.iter().map(|p| pt2(*p)).collect();
    match curve {
        Curve::Polyline => {
            host.run("sketch.line", json!({"points": list, "closed": true}))?;
        }
        Curve::Spline => {
            let (first, last) = (list[0].clone(), list[list.len() - 1].clone());
            host.run("sketch.spline.fit_point", json!({"points": list}))?;
            host.run("sketch.line", json!({"points": [last, first]}))?;
        }
    }
    host.run("sketch.finish", json!({}))?;
    Ok(())
}

/// SolidCraft allows a plugin run this many host calls (commands, progress and cancel checks).
const MAX_HOST_CALLS: usize = 256;

/// A ring (or disc) on the XY plane at `z`, extruded `length` along +Z as a new body.
fn ring(host: &mut dyn Host, name: &str, body: &str, z: f64, outer: f64, inner: f64, length: f64) -> Result<(), Failure> {
    let mut args = json!({"plane": "XY", "name": name});
    if z != 0.0 {
        args["offset"] = json!(round(z));
    }
    host.run("sketch.create", args)?;
    host.run("sketch.circle.center", json!({"center": [0.0, 0.0], "diameter": round(outer)}))?;
    if inner > 0.0 {
        host.run("sketch.circle.center", json!({"center": [0.0, 0.0], "diameter": round(inner)}))?;
    }
    host.run("sketch.finish", json!({}))?;
    host.run("solid.extrude", json!({"sketch": name, "distance": round(length), "operation": "new", "body_name": body}))?;
    Ok(())
}

/// Build a rotor. Every check runs before the first command.
pub fn build_rotor(host: &mut dyn Host, rotor: &Rotor, curve: Curve) -> Result<Value, Failure> {
    let n = rotor.points;
    let te = rotor.trailing_edge;
    let sections: Vec<Vec<[f64; 2]>> = rotor.stations.iter().map(|s| placed(s, n, te, rotor.suction_up, rotor.mirror)).collect();
    for (s, pts) in rotor.stations.iter().zip(&sections) {
        if self_intersects(pts) {
            return Err(Failure::Input(
                "trailing_edge",
                format!(
                    "the section at r = {:.3} mm folds over itself: its chord ({:.3} mm) is too short for the thickness; lower trailing_edge or the thickness",
                    s.r, s.chord
                ),
            ));
        }
    }
    let rh = rotor.hub_radius;
    // Each blade stands on a pad: a rectangular block round its root section, from inside the
    // hub to just outside its wall, and the blade's loft starts on the pad's flat outer face.
    // Only the pad's flat faces cross the hub's cylinder, which the kernel intersects reliably;
    // a lofted root crossing the cylinder often gives it a near-cusp it refuses.
    // (y0, y1, z0, z1) round the root section in its plane, from radius `r_in` to `r_pad`.
    let mut pad = None;
    let mut hub_z = (0.0, 0.0);
    let mut lofted: Vec<(f64, usize)> = Vec::with_capacity(sections.len());
    if rotor.hub {
        let (y0, y1, z0, z1) = bounds(&sections[0]);
        let m = 0.06 * (z1 - z0).max(y1 - y0);
        let (y0, y1, z0, z1) = (y0 - m, y1 + m, z0 - m, z1 + m);
        let wy = y0.abs().max(y1.abs());
        if wy >= 0.9 * rh {
            return Err(Failure::Input(
                "hub_diameter",
                format!("the blade roots are {:.3} mm across; make the hub diameter more than {:.3} mm or the root chord smaller", 2.0 * wy, 2.0 * wy / 0.9),
            ));
        }
        let r_in = 0.9 * (rh * rh - wy * wy).sqrt();
        let floor = rotor.bore / 2.0 + 0.15 * (rh - rotor.bore / 2.0);
        if r_in < floor {
            return Err(Failure::Input(
                "bore",
                format!(
                    "the blade roots would reach the bore: make the bore less than {:.3} mm, the hub larger or the root chord smaller",
                    2.0 * (r_in - 0.15 * rh) / 0.85
                ),
            ));
        }
        let r_pad = (rh * rh + wy * wy).sqrt() + 0.02 * rh;
        pad = Some((y0, y1, z0, z1, r_in, r_pad));
        // The hub spans the pads, with a margin.
        let span = z1 - z0;
        let len = if rotor.hub_length > 0.0 {
            if rotor.hub_length < span + 0.2 {
                return Err(Failure::Input("hub_length", format!("must be at least {:.3} mm to hold the blade roots (0 = automatic)", span + 0.2)));
            }
            rotor.hub_length
        } else {
            span + 2.0 * (0.06 * span).max(0.5)
        };
        let mid = 0.5 * (z0 + z1);
        hub_z = (mid - len / 2.0, mid + len / 2.0);
        // The loft starts with the root section on the pad and goes on from the first station
        // clear of it.
        lofted.push((r_pad, 0));
        let last = rotor.stations.len() - 1;
        let clear = r_pad + 0.03 * (rotor.radius - rh);
        lofted.extend(rotor.stations.iter().enumerate().skip(1).filter(|(i, s)| *i == last || s.r > clear).map(|(i, s)| (s.r, i)));
        if rotor.stations[last].r <= clear || lofted.len() < 3 {
            return Err(Failure::Input("hub_diameter", "the blade roots are too wide for this hub: make the hub larger or the root chord smaller".into()));
        }
    } else {
        lofted.extend(rotor.stations.iter().enumerate().map(|(i, s)| (s.r, i)));
    }
    let w = pad.map_or(0.0, |(y0, y1, ..): (f64, f64, f64, f64, f64, f64)| y0.abs().max(y1.abs()));
    let blades = if rotor.single_blade { 1 } else { rotor.blades };
    let loft_count = lofted.len();
    if loft_count > 50 {
        return Err(Failure::Input("sections", "a loft takes at most 50 sections".into()));
    }
    // The overall axial extent, for the duct and the reply.
    let (blade_lo, blade_hi) = sections.iter().fold((f64::MAX, f64::MIN), |(lo, hi), pts| {
        let (_, _, a, b) = bounds(pts);
        (lo.min(a), hi.max(b))
    });
    let duct = match &rotor.duct {
        Some(d) => {
            let inner = 2.0 * (rotor.radius + d.clearance);
            let len = if d.length > 0.0 { d.length } else { (blade_hi - blade_lo + 4.0 * d.wall).max(0.3 * rotor.radius * 2.0) };
            let mid = 0.5 * (blade_lo + blade_hi);
            Some((inner, inner + 2.0 * d.wall, mid - len / 2.0, len))
        }
        None => None,
    };

    // Host calls: the hub (4), the pad (4), each section (4), the loft, the pattern, the duct
    // (4), and a few progress and cancel checks.
    let calls = 4 + 4 + 4 * loft_count + 2 + 4 + 8;
    if calls > MAX_HOST_CALLS {
        return Err(Failure::Input("sections", format!("too many sections for one plugin run: use at most {}", (MAX_HOST_CALLS - 22) / 4 - 1)));
    }

    let body = rotor.name.clone();
    let mut bodies = vec![body.clone()];
    if rotor.hub {
        host.progress(0.02, "hub");
        ring(host, &format!("{body} hub"), &body, hub_z.0, 2.0 * rh, rotor.bore, hub_z.1 - hub_z.0)?;
    }
    // One blade, on +X: its pad and its loft. A circular pattern of those two features makes
    // the others and joins them to the same body.
    let blade_name = format!("{body} blade");
    let pad_name = format!("{body} blade pad");
    if let Some((a, b, c, d, r_in, r_pad)) = pad {
        let (o, x, y) = blade::plane(r_in, 0.0);
        host.run("sketch.create", json!({"name": pad_name, "plane": {"origin": pt3(o), "x_dir": pt3(x), "y_dir": pt3(y)}}))?;
        host.run("sketch.line", json!({"points": [pt2([a, c]), pt2([b, c]), pt2([b, d]), pt2([a, d])], "closed": true}))?;
        host.run("sketch.finish", json!({}))?;
        host.run("solid.extrude", json!({"sketch": pad_name, "distance": round(r_pad - r_in), "operation": "join", "targets": [body], "name": pad_name}))?;
    }
    let mut names = Vec::with_capacity(loft_count);
    for (j, &(r, i)) in lofted.iter().enumerate() {
        if j % 8 == 0 {
            host.progress(0.05 + 0.6 * j as f32 / loft_count as f32, "sketching sections");
            if host.cancelled() {
                return Err(Failure::Cancelled);
            }
        }
        let name = format!("{body} section {}", j + 1);
        sketch_section(host, &name, blade::plane(r, 0.0), &sections[i], curve)?;
        names.push(json!({"sketch": name}));
    }
    host.progress(0.7, "lofting the blade");
    let mut loft = json!({"sections": names, "name": blade_name});
    if rotor.hub {
        loft["operation"] = json!("join");
        loft["targets"] = json!([body]);
    } else {
        loft["operation"] = json!("new");
        loft["body_name"] = json!(body);
    }
    host.run("solid.loft", loft)?;
    if blades > 1 {
        host.progress(0.8, "the other blades");
        let mut features = vec![json!(blade_name)];
        if pad.is_some() {
            features.insert(0, json!(pad_name));
        }
        host.run("solid.pattern.circular", json!({"features": features, "axis": "Z", "count": blades, "name": format!("{body} blades")}))?;
    }
    if let Some((inner, outer, z, len)) = duct {
        host.progress(0.97, "duct");
        let name = format!("{body} duct");
        ring(host, &name, &name, z, outer, inner, len)?;
        bodies.push(name);
    }
    host.progress(1.0, "done");

    let r = rotor.radius;
    let span = r - rh;
    // Expanded blade area over the disc, by the trapezoid rule over the stations.
    let st = &rotor.stations;
    let blade_area: f64 = st.windows(2).map(|p| 0.5 * (p[0].chord + p[1].chord) * (p[1].r - p[0].r)).sum();
    let table: Vec<Value> = st
        .iter()
        .map(|s: &Station| {
            json!({
                "r_mm": round(s.r),
                "r_over_R": round(s.r / r),
                "chord_mm": round(s.chord),
                "blade_angle_deg": round(s.beta.to_degrees()),
                "thickness_pct": round(100.0 * s.thickness),
                "round": round(s.round),
            })
        })
        .collect();
    let beta75 = (pitch_at_75(rotor) / (2.0 * PI * 0.75 * r)).atan();
    let mut warnings = Vec::new();
    // Neighbouring roots overlap at the hub when their footprints fill its circumference.
    if rotor.hub && blades > 1 && 2.0 * (w / rh).min(1.0).asin() * f64::from(blades) > 2.0 * PI * 0.95 {
        warnings.push("neighbouring blade roots overlap where they meet the hub; they are joined into one body".to_string());
    }
    let mut out = json!({
        "body": body,
        "bodies": bodies,
        "blades": blades,
        "diameter_mm": 2.0 * r,
        "hub_diameter_mm": 2.0 * rh,
        "bore_mm": rotor.bore,
        "blade_span_mm": span,
        "pitch_at_75_mm": pitch_at_75(rotor),
        "blade_angle_at_75_deg": beta75.to_degrees(),
        "blade_area_ratio": f64::from(rotor.blades) * blade_area / (PI * r * r),
        "axial_extent_mm": [round(blade_lo), round(blade_hi)],
        "sections": table,
        "section_points": 2 * n + 1,
        "axis_note": "the rotor turns about the Z axis through the origin; blade 1 points along +X",
        "warnings": warnings,
    });
    if rotor.hub {
        out["hub_length_mm"] = json!(hub_z.1 - hub_z.0);
        out["hub_z_mm"] = json!([round(hub_z.0), round(hub_z.1)]);
    }
    if let Some((inner, outer, z, len)) = duct {
        out["duct"] = json!({"inner_diameter_mm": inner, "outer_diameter_mm": outer, "z_mm": [round(z), round(z + len)], "length_mm": len});
    }
    Ok(out)
}

/// A toroidal propeller's inputs.
pub fn toroid_design(marine: bool, p: &Value) -> Result<toroid::Toroid, Failure> {
    let mirror = match (marine, rotation(p)?) {
        (false, "default" | "ccw") | (true, "default" | "right") => false,
        (false, "cw") | (true, "left") => true,
        (false, _) => return Err(Failure::Input("rotation", "must be ccw or cw".into())),
        (true, _) => return Err(Failure::Input("rotation", "must be right or left".into())),
    };
    Ok(toroid::Toroid {
        marine,
        loops: int(p, "loops", 3)?,
        diameter: num(p, "diameter", if marine { 250.0 } else { 254.0 })?,
        pitch: num(p, "pitch", if marine { 250.0 } else { 114.3 })?,
        hub_diameter: num(p, "hub_diameter", 0.0)?,
        hub_length: num(p, "hub_length", 0.0)?,
        bore: num(p, "bore", if marine { 0.0 } else { 5.0 })?,
        root_chord: num(p, "root_chord", 0.0)?,
        tip_chord: num(p, "tip_chord", 0.0)?,
        root_thickness: num(p, "root_thickness", 0.12)?,
        tip_thickness: num(p, "tip_thickness", if marine { 0.08 } else { 0.1 })?,
        spread: num(p, "spread", if marine { 0.6 } else { 0.55 })?,
        offset: num(p, "axial_offset", 0.0)?,
        sections: int(p, "sections", 25)?,
        points: int(p, "section_points", 14)?,
        trailing_edge: num(p, "trailing_edge", if marine { 0.5 } else { 0.3 })?,
        mirror,
    })
}

/// Build a toroidal propeller: the hub, one loop (a pad at each root and a loft between them),
/// and a circular pattern for the other loops.
pub fn build_toroid(host: &mut dyn Host, t: &toroid::Toroid, curve: Curve) -> Result<Value, Failure> {
    let l = t.design()?;
    let calls = 4 + 8 + 4 * l.sections.len() + 2 + 8;
    if calls > MAX_HOST_CALLS || l.sections.len() > 50 {
        return Err(Failure::Input("sections", "too many sections for one plugin run".into()));
    }
    let body = l.name.clone();
    let rh = l.hub_radius;
    host.progress(0.02, "hub");
    ring(host, &format!("{body} hub"), &body, l.hub_z.0, 2.0 * rh, t.bore, l.hub_z.1 - l.hub_z.0)?;
    let mut features = Vec::new();
    for (i, (angle, y0, y1, z0, z1)) in l.pads.iter().copied().enumerate() {
        let name = format!("{body} {} pad", if i == 0 { "leading" } else { "trailing" });
        let (o, x, y) = blade::plane(l.r_in, angle);
        host.run("sketch.create", json!({"name": name, "plane": {"origin": pt3(o), "x_dir": pt3(x), "y_dir": pt3(y)}}))?;
        host.run("sketch.line", json!({"points": [pt2([y0, z0]), pt2([y1, z0]), pt2([y1, z1]), pt2([y0, z1])], "closed": true}))?;
        host.run("sketch.finish", json!({}))?;
        host.run("solid.extrude", json!({"sketch": name, "distance": round(l.r_pad - l.r_in), "operation": "join", "targets": [body], "name": name}))?;
        features.push(json!(name));
    }
    let mut names = Vec::with_capacity(l.sections.len());
    for (j, (plane, pts)) in l.sections.iter().enumerate() {
        if j % 8 == 0 {
            host.progress(0.05 + 0.6 * j as f32 / l.sections.len() as f32, "sketching sections");
            if host.cancelled() {
                return Err(Failure::Cancelled);
            }
        }
        let name = format!("{body} section {}", j + 1);
        sketch_section(host, &name, *plane, pts, curve)?;
        names.push(json!({"sketch": name}));
    }
    host.progress(0.7, "lofting the loop");
    let loop_name = format!("{body} loop");
    host.run("solid.loft", json!({"sections": names, "operation": "join", "targets": [body], "name": loop_name}))?;
    features.push(json!(loop_name));
    if t.loops > 1 {
        host.progress(0.85, "the other loops");
        host.run("solid.pattern.circular", json!({"features": features, "axis": "Z", "count": t.loops, "name": format!("{body} loops")}))?;
    }
    host.progress(1.0, "done");
    let table: Vec<Value> = l
        .table
        .iter()
        .map(|[f, r, c, a, z]| json!({"along_loop": round(*f), "r_mm": round(*r), "chord_mm": round(*c), "chord_angle_deg": round(*a), "z_mm": round(*z)}))
        .collect();
    Ok(json!({
        "body": body,
        "bodies": [body],
        "loops": t.loops,
        "diameter_mm": t.diameter,
        "pitch_mm": t.pitch,
        "pitch_ratio": t.pitch / t.diameter,
        "pitch_inches": t.pitch / 25.4,
        "diameter_inches": t.diameter / 25.4,
        "hub_diameter_mm": 2.0 * rh,
        "bore_mm": t.bore,
        "hub_length_mm": l.hub_z.1 - l.hub_z.0,
        "hub_z_mm": [round(l.hub_z.0), round(l.hub_z.1)],
        "root_chord_mm": t.auto_root_chord(),
        "tip_chord_mm": t.auto_tip_chord(),
        "spread_deg": t.spread * 360.0 / f64::from(t.loops),
        "axial_extent_mm": [round(l.axial_extent.0), round(l.axial_extent.1)],
        "rotation": match (t.marine, t.mirror) { (false, false) => "ccw", (false, true) => "cw", (true, false) => "right", (true, true) => "left" },
        "rotation_note": if t.marine {
            "right hand turns clockwise seen from astern (from -Z) when going ahead; ahead is +Z"
        } else {
            "seen from the front (+Z, the way the thrust points); the propeller pushes air towards -Z"
        },
        "loop_note": "each loop leaves the hub forward and ahead (its leading root) and returns aft and behind (its trailing root); the sections are symmetric airfoils, their chords following the pitch helix and turning through axial at the tip",
        "sections": table,
        "section_points": 2 * t.points + 1,
        "axis_note": "the rotor turns about the Z axis through the origin; loop 1 is centred on +X",
        "warnings": l.warnings,
    }))
}

/// Build an entry. `params` is the entry's JSON input.
pub fn build(host: &mut dyn Host, kind: Kind, params: &Value) -> Result<Value, Failure> {
    let curve = match choice(params, "section_curve", &["spline", "polyline"])? {
        0 => Curve::Spline,
        _ => Curve::Polyline,
    };
    if matches!(kind, Kind::Toroidal | Kind::ToroidalMarine) {
        let t = toroid_design(kind == Kind::ToroidalMarine, params)?;
        return build_toroid(host, &t, curve);
    }
    let (rotor, facts) = design(kind, params)?;
    let mut out = build_rotor(host, &rotor, curve)?;
    if let (Some(o), Value::Object(f)) = (out.as_object_mut(), facts) {
        o.extend(f);
    }
    Ok(out)
}

#[cfg(target_arch = "wasm32")]
mod component {
    wit_bindgen::generate!({ world: "command-plugin", path: "wit" });

    use super::{Failure, Host};
    use craft::plugin::types::{CommandFailure, FieldFailure, PluginError};
    use craft::plugin::{commands, control, log};
    use serde_json::Value;

    struct Wasm;
    impl Host for Wasm {
        fn run(&mut self, command: &str, args: Value) -> Result<Value, Failure> {
            match commands::run(command, &args.to_string()) {
                Ok(text) => Ok(serde_json::from_str(&text).unwrap_or(Value::Null)),
                Err(e) => Err(Failure::Command(command.to_string(), format!("{e:?}"))),
            }
        }
        fn progress(&mut self, fraction: f32, message: &str) {
            control::progress(fraction, message);
        }
        fn cancelled(&mut self) -> bool {
            control::cancelled()
        }
    }

    struct Plugin;
    impl exports::craft::plugin::entry::Guest for Plugin {
        fn execute(entry: String, params: String) -> Result<String, PluginError> {
            let Some(kind) = super::Kind::from_entry(&entry) else {
                return Err(PluginError::InvalidInput(FieldFailure { field: "entry".into(), reason: format!("unknown entry {entry}") }));
            };
            let params: Value =
                serde_json::from_str(&params).map_err(|e| PluginError::InvalidInput(FieldFailure { field: "params".into(), reason: e.to_string() }))?;
            match super::build(&mut Wasm, kind, &params) {
                Ok(v) => Ok(v.to_string()),
                Err(Failure::Input(field, reason)) => Err(PluginError::InvalidInput(FieldFailure { field: field.into(), reason })),
                Err(Failure::Command(command, reason)) => {
                    log::log(log::Level::Error, &format!("{command} failed: {reason}"));
                    Err(PluginError::CommandFailed(CommandFailure { command, reason }))
                }
                Err(Failure::Cancelled) => Err(PluginError::Cancelled),
            }
        }
    }

    export!(Plugin);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct Fake(Vec<(String, Value)>);
    impl Host for Fake {
        fn run(&mut self, command: &str, args: Value) -> Result<Value, Failure> {
            self.0.push((command.into(), args));
            Ok(json!({}))
        }
        fn progress(&mut self, _: f32, _: &str) {}
        fn cancelled(&mut self) -> bool {
            false
        }
    }

    fn run(kind: Kind, p: Value) -> (Fake, Result<Value, Failure>) {
        let mut h = Fake::default();
        let r = build(&mut h, kind, &p);
        (h, r)
    }

    fn count(h: &Fake, c: &str) -> usize {
        h.0.iter().filter(|(n, _)| n == c).count()
    }

    #[test]
    fn every_entry_builds_with_its_defaults() {
        // The hub, one blade's pad and loft, a pattern for the others, and perhaps a duct.
        for (kind, blades, extrudes) in [(Kind::Propeller, 2, 2), (Kind::Marine, 3, 2), (Kind::WindTurbine, 3, 2), (Kind::DuctedFan, 7, 3)] {
            let (h, r) = run(kind, json!({}));
            let out = r.unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
            assert_eq!(count(&h, "solid.loft"), 1, "{kind:?}");
            assert_eq!(count(&h, "solid.extrude"), extrudes, "{kind:?}");
            assert_eq!(count(&h, "sketch.create"), count(&h, "sketch.finish"));
            let pattern = &h.0.iter().find(|(c, _)| c == "solid.pattern.circular").unwrap_or_else(|| panic!("{kind:?}")).1;
            assert_eq!(pattern["count"], json!(blades), "{kind:?}");
            assert_eq!(pattern["axis"], json!("Z"));
            assert!(out["warnings"].as_array().is_some(), "{kind:?}");
            assert!(h.0.len() + 8 < MAX_HOST_CALLS, "{kind:?}: {} calls", h.0.len());
        }
    }

    #[test]
    fn toroidal_entries_build_one_loop_and_pattern_it() {
        for (kind, extrudes) in [(Kind::Toroidal, 3), (Kind::ToroidalMarine, 3)] {
            let (h, r) = run(kind, json!({}));
            let out = r.unwrap_or_else(|e| panic!("{kind:?}: {e:?}"));
            assert_eq!(count(&h, "solid.loft"), 1, "{kind:?}");
            assert_eq!(count(&h, "solid.extrude"), extrudes, "{kind:?}");
            assert_eq!(count(&h, "sketch.create"), count(&h, "sketch.finish"));
            let pattern = &h.0.iter().find(|(c, _)| c == "solid.pattern.circular").unwrap_or_else(|| panic!("{kind:?}")).1;
            assert_eq!(pattern["count"], json!(3));
            // Both pads and the loop are patterned.
            assert_eq!(pattern["features"].as_array().map(Vec::len), Some(3));
            let loft = &h.0.iter().find(|(c, _)| c == "solid.loft").unwrap_or_else(|| panic!("{kind:?}")).1;
            assert_eq!(loft["sections"].as_array().map(Vec::len), Some(25));
            assert_eq!(loft["operation"], json!("join"));
            assert_eq!(out["loops"], json!(3));
            assert!(h.0.len() + 8 < MAX_HOST_CALLS, "{kind:?}: {} calls", h.0.len());
        }
        let (_, r) = run(Kind::Toroidal, json!({"rotation": "right"}));
        assert!(matches!(r, Err(Failure::Input("rotation", _))));
        let (_, r) = run(Kind::ToroidalMarine, json!({"rotation": "cw"}));
        assert!(matches!(r, Err(Failure::Input("rotation", _))));
    }

    #[test]
    fn propeller_calls_in_order() {
        let (h, r) = run(Kind::Propeller, json!({"sections": 5}));
        r.unwrap();
        let cmds: Vec<&str> = h.0.iter().map(|(c, _)| c.as_str()).collect();
        assert_eq!(&cmds[..5], ["sketch.create", "sketch.circle.center", "sketch.circle.center", "sketch.finish", "solid.extrude"]);
        // The pad, sketched and extruded into the hub, then the sections, the loft, the pattern.
        assert_eq!(&cmds[5..9], ["sketch.create", "sketch.line", "sketch.finish", "solid.extrude"]);
        assert_eq!(h.0[8].1["operation"], json!("join"));
        let one = ["sketch.create", "sketch.spline.fit_point", "sketch.line", "sketch.finish"];
        assert_eq!(&cmds[9..13], one);
        let loft = &h.0[cmds.len() - 2].1;
        let sections = loft["sections"].as_array().map_or(0, Vec::len);
        assert!((4..=5).contains(&sections), "{sections}");
        assert_eq!(cmds.len(), 9 + sections * 4 + 2);
        assert_eq!(loft["operation"], json!("join"));
        let pattern = &h.0[cmds.len() - 1].1;
        assert_eq!(pattern["features"], json!([h.0[8].1["name"], loft["name"]]));
    }

    #[test]
    fn sections_lie_on_radial_planes() {
        let (h, r) = run(Kind::Propeller, json!({"blades": 4, "sections": 4}));
        r.unwrap();
        for (_, a) in h.0.iter().filter(|(c, a)| c == "sketch.create" && a["plane"].is_object()) {
            let p = &a["plane"];
            assert!(p["origin"][0].as_f64().unwrap() > 0.0);
            assert_eq!(p["origin"][1], json!(0.0));
            assert_eq!(p["x_dir"], json!([0.0, 1.0, 0.0]));
            assert_eq!(p["y_dir"], json!([0.0, 0.0, 1.0]));
        }
    }

    #[test]
    fn polyline_option_and_spline_closure() {
        let (h, _) = run(Kind::Propeller, json!({"section_curve": "polyline", "sections": 4}));
        assert_eq!(count(&h, "sketch.spline.fit_point"), 0);
        let (h, _) = run(Kind::Propeller, json!({"sections": 4}));
        let (spline, line) = (&h.0[10].1["points"], &h.0[11].1["points"]);
        let n = spline.as_array().unwrap().len();
        assert_eq!(line[0], spline[n - 1]);
        assert_eq!(line[1], spline[0]);
    }

    #[test]
    fn rotation_mirrors() {
        let (a, _) = run(Kind::Propeller, json!({"sections": 4}));
        let (b, _) = run(Kind::Propeller, json!({"sections": 4, "rotation": "cw"}));
        let (pa, pb) = (&a.0[10].1["points"][16], &b.0[10].1["points"][16]);
        assert!((pa[0].as_f64().unwrap() + pb[0].as_f64().unwrap()).abs() < 1e-9);
        assert_eq!(pa[1], pb[1]);
    }

    #[test]
    fn the_most_sections_stay_within_the_host_call_limit() {
        for kind in [Kind::Propeller, Kind::Marine, Kind::DuctedFan, Kind::WindTurbine, Kind::Toroidal, Kind::ToroidalMarine] {
            let sections = if matches!(kind, Kind::Toroidal | Kind::ToroidalMarine) { 45 } else { 40 };
            let (h, r) = run(kind, json!({"sections": sections, "section_points": 60}));
            match r {
                Ok(_) => assert!(h.0.len() + 8 <= MAX_HOST_CALLS, "{kind:?}: {}", h.0.len()),
                Err(Failure::Input("sections", _)) => assert!(h.0.is_empty()),
                Err(e) => panic!("{kind:?}: {e:?}"),
            }
        }
    }

    #[test]
    fn bad_inputs_are_refused_before_any_command() {
        for (kind, p, field) in [
            (Kind::Propeller, json!({"blades": 1}), "blades"),
            (Kind::Propeller, json!({"rotation": "left"}), "rotation"),
            (Kind::Propeller, json!({"hub_diameter": 5.5}), "hub_diameter"),
            (Kind::Propeller, json!({"bore": 14, "hub_diameter": 16}), "bore"),
            (Kind::Propeller, json!({"hub_length": 1}), "hub_length"),
            (Kind::Propeller, json!({"sections": 60}), "sections"),
            (Kind::Marine, json!({"area_ratio": 2}), "area_ratio"),
            (Kind::Marine, json!({"rotation": "cw"}), "rotation"),
            (Kind::WindTurbine, json!({"tip_speed_ratio": 30}), "tip_speed_ratio"),
            (Kind::WindTurbine, json!({"build": "nacelle"}), "build"),
            (Kind::WindTurbine, json!({"root_transition": 0.05}), "root_transition"),
            (Kind::DuctedFan, json!({"duct": "yes"}), "duct"),
            (Kind::DuctedFan, json!({"diameter": "big"}), "diameter"),
        ] {
            let (h, r) = run(kind, p.clone());
            match r {
                Err(Failure::Input(f, _)) => assert_eq!(f, field, "{kind:?} {p}"),
                other => panic!("{kind:?} {p}: {other:?}"),
            }
            assert!(h.0.is_empty(), "{kind:?} {p}");
        }
    }

    #[test]
    fn wind_turbine_blade_alone_is_a_new_body() {
        let (h, r) = run(Kind::WindTurbine, json!({"build": "blade"}));
        let out = r.unwrap();
        assert_eq!(count(&h, "solid.extrude"), 0);
        assert_eq!(count(&h, "solid.loft"), 1);
        assert_eq!(
            out["sections"].as_array().map(Vec::len),
            h.0.iter().find(|(c, _)| c == "solid.loft").and_then(|(_, a)| a["sections"].as_array().map(Vec::len))
        );
        assert_eq!(h.0.iter().find(|(c, _)| c == "solid.loft").unwrap().1["operation"], json!("new"));
        assert_eq!(out["blades"], json!(1));
    }

    #[test]
    fn ducted_fan_duct_clears_the_tips() {
        let (_, r) = run(Kind::DuctedFan, json!({"tip_clearance": 0.5, "duct_wall": 2}));
        let out = r.unwrap();
        assert_eq!(out["duct"]["inner_diameter_mm"], json!(65.0));
        assert_eq!(out["duct"]["outer_diameter_mm"], json!(69.0));
        let (h, _) = run(Kind::DuctedFan, json!({"duct": false}));
        assert_eq!(count(&h, "solid.extrude"), 2);
    }

    #[test]
    fn hostile_numbers_never_panic() {
        let vals = [json!(-1), json!(0), json!(1e308), json!(-1e308), json!(0.5), json!(3), json!(1000), json!(null), json!("x"), json!(true)];
        let keys = [
            "blades",
            "diameter",
            "pitch",
            "hub_diameter",
            "hub_length",
            "bore",
            "root_chord",
            "max_chord",
            "max_chord_at",
            "tip_chord",
            "root_thickness",
            "tip_thickness",
            "camber",
            "camber_position",
            "sections",
            "section_points",
            "trailing_edge",
            "area_ratio",
            "skew",
            "rake",
            "thickness_scale",
            "tip_speed_ratio",
            "design_cl",
            "angle_of_attack",
            "root_diameter",
            "root_transition",
            "tip_clearance",
            "duct_wall",
            "duct_length",
            "rpm",
            "wind_speed",
        ];
        for kind in [Kind::Propeller, Kind::Marine, Kind::WindTurbine, Kind::DuctedFan] {
            for k in keys {
                for v in &vals {
                    let mut p = json!({});
                    p[k] = v.clone();
                    let _ = run(kind, p);
                }
            }
        }
    }
}
