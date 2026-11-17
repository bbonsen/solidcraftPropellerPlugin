//! The four kinds of rotor, each turned into blade stations by its own design rules.

use crate::blade::{pitch_angle, pitch_of, radii, smooth, Station};
use std::f64::consts::PI;

/// A rotor ready to build: a hub, `blades` copies of one blade, and perhaps a duct.
#[derive(Clone, Debug)]
pub struct Rotor {
    pub name: String,
    pub blades: u32,
    /// Tip radius, mm.
    pub radius: f64,
    pub hub_radius: f64,
    /// 0 means long enough for the blade roots.
    pub hub_length: f64,
    pub bore: f64,
    /// Stations from the hub (first, at `hub_radius`) to the tip (last).
    pub stations: Vec<Station>,
    pub suction_up: bool,
    pub mirror: bool,
    /// Points per side of each section.
    pub points: usize,
    /// Minimum trailing edge thickness, mm.
    pub trailing_edge: f64,
    /// Build the hub; without it the first blade is a new body and the others join it.
    pub hub: bool,
    /// Only one blade (for a wind turbine blade made on its own).
    pub single_blade: bool,
    pub duct: Option<Duct>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Duct {
    /// Gap between the blade tips and the duct wall, mm.
    pub clearance: f64,
    pub wall: f64,
    /// 0 means automatic.
    pub length: f64,
}

/// Input checks report the field and why.
pub type Check<T> = Result<T, (&'static str, String)>;

fn within(field: &'static str, v: f64, lo: f64, hi: f64, unit: &str) -> Check<f64> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(v)
    } else {
        Err((field, format!("must be {lo}…{hi}{unit}")))
    }
}

fn whole(field: &'static str, v: u32, lo: u32, hi: u32) -> Check<u32> {
    if (lo..=hi).contains(&v) {
        Ok(v)
    } else {
        Err((field, format!("must be {lo}…{hi}")))
    }
}

/// Section detail shared by every kind.
#[derive(Clone, Debug)]
pub struct Detail {
    pub sections: u32,
    pub points: u32,
    pub trailing_edge: f64,
    pub mirror: bool,
}

impl Detail {
    pub fn check(&self) -> Check<()> {
        whole("sections", self.sections, 4, 40)?;
        whole("section_points", self.points, 6, 60)?;
        within("trailing_edge", self.trailing_edge, 0.0, 5.0, " mm")?;
        Ok(())
    }
}

/// Thickness from root to tip as fractions of the chord, checked.
fn thickness(root: f64, tip: f64) -> Check<(f64, f64)> {
    Ok((within("root_thickness", root, 0.04, 0.4, "")?, within("tip_thickness", tip, 0.03, 0.3, "")?))
}

/// Give the blade a rounded shank at the root: the first station becomes an oval `width` ×
/// `thick` mm, and the stations out to `blend` mm beyond the hub blend from it into the
/// airfoil. Real blades are built this way, and it keeps thin leading and trailing edges out
/// of the hub, where the kernel has to intersect the blade with the hub's wall.
pub fn root_shank(stations: &mut [Station], blend: f64, width: f64, thick: f64) {
    let r0 = stations[0].r;
    for s in stations.iter_mut() {
        let u = ((s.r - r0) / blend).clamp(0.0, 1.0);
        if u < 1.0 {
            s.round = 1.0 - u * u * (3.0 - 2.0 * u);
            s.round_diameter = width;
            s.round_thickness = thick;
        }
    }
}

/// The usual shank: an oval `width` × the root chord wide, at least 35% as thick as it is wide,
/// blending into the airfoil over 12% of the blade's span.
fn standard_shank(stations: &mut [Station], span: f64, width: f64) {
    let s0 = stations[0].clone();
    let width = width * s0.chord;
    let thick = (1.5 * s0.thickness * s0.chord).max(0.35 * width);
    root_shank(stations, (0.12 * span).max(1.5 * thick), width, thick);
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn hub_radius(diameter: f64, hub: f64, auto: f64, bore: f64) -> Check<f64> {
    let h = if hub > 0.0 { hub } else { auto.max(bore + 4.0) };
    if !(h.is_finite() && h > bore + 1.0) {
        return Err(("hub_diameter", format!("must be more than the bore + 1 mm ({:.3} mm)", bore + 1.0)));
    }
    if h >= 0.8 * diameter {
        return Err(("hub_diameter", format!("must be less than 80% of the diameter ({:.3} mm)", 0.8 * diameter)));
    }
    Ok(h / 2.0)
}

/// An air propeller: drone, RC model or light aircraft. Constant geometric pitch, airfoil
/// sections of tapering thickness, a chord that swells to `max_chord` and narrows to the tip.
#[derive(Clone, Debug)]
pub struct Propeller {
    pub blades: u32,
    pub diameter: f64,
    pub pitch: f64,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub bore: f64,
    pub root_chord: f64,
    pub max_chord: f64,
    pub max_chord_at: f64,
    pub tip_chord: f64,
    pub root_thickness: f64,
    pub tip_thickness: f64,
    pub camber: f64,
    pub camber_pos: f64,
    pub detail: Detail,
}

impl Propeller {
    pub fn rotor(&self) -> Check<Rotor> {
        whole("blades", self.blades, 2, 8)?;
        let d = within("diameter", self.diameter, 10.0, 10_000.0, " mm")?;
        let pitch = within("pitch", self.pitch, 0.05 * d, 3.0 * d, " mm (0.05…3 × diameter)")?;
        let bore = within("bore", self.bore, 0.0, 0.5 * d, " mm")?;
        let rh = hub_radius(d, self.hub_diameter, 0.06 * d, bore)?;
        let auto = |v: f64, f: f64| if v > 0.0 { v } else { f * d };
        let (c_root, c_max, c_tip) = (auto(self.root_chord, 0.06), auto(self.max_chord, 0.09), auto(self.tip_chord, 0.035));
        for (f, c) in [("root_chord", c_root), ("max_chord", c_max), ("tip_chord", c_tip)] {
            within(f, c, 0.1, 0.5 * d, " mm")?;
        }
        let r = d / 2.0;
        let xh = rh / r;
        let xm = within("max_chord_at", self.max_chord_at, 0.0, 0.95, "")?.max(xh + 0.02);
        if xm >= 0.97 {
            return Err(("max_chord_at", "must leave room between the hub and the tip".into()));
        }
        let (t_root, t_tip) = thickness(self.root_thickness, self.tip_thickness)?;
        let camber = within("camber", self.camber, 0.0, 9.0, " %")? / 100.0;
        let camber_pos = within("camber_position", self.camber_pos, 0.1, 0.9, "")?;
        self.detail.check()?;
        let knots = [(xh, c_root), (xm, c_max), (1.0, c_tip)];
        let mut stations: Vec<Station> = radii(rh, r, self.detail.sections as usize)
            .into_iter()
            .map(|ri| {
                let x = ri / r;
                let t = (x - xh) / (1.0 - xh);
                Station { camber, camber_pos, ..Station::foil(ri, smooth(&knots, x), pitch_angle(pitch, ri), lerp(t_root, t_tip, t)) }
            })
            .collect();
        standard_shank(&mut stations, r - rh, 0.7);
        Ok(Rotor {
            name: format!("Propeller {}x{} {}-blade", fmt(d), fmt(pitch), self.blades),
            blades: self.blades,
            radius: r,
            hub_radius: rh,
            hub_length: within("hub_length", self.hub_length, 0.0, 10.0 * d, " mm")?,
            bore,
            stations,
            suction_up: true,
            mirror: self.detail.mirror,
            points: self.detail.points as usize,
            trailing_edge: self.detail.trailing_edge,
            hub: true,
            single_blade: false,
            duct: None,
        })
    }
}

/// A ducted fan: many blades of near-constant chord in a close-fitting duct (EDF units, cooling
/// and blower fans).
#[derive(Clone, Debug)]
pub struct DuctedFan {
    pub blades: u32,
    pub diameter: f64,
    pub pitch: f64,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub bore: f64,
    pub root_chord: f64,
    pub tip_chord: f64,
    pub root_thickness: f64,
    pub tip_thickness: f64,
    pub camber: f64,
    pub duct: Option<Duct>,
    pub detail: Detail,
}

impl DuctedFan {
    pub fn rotor(&self) -> Check<Rotor> {
        whole("blades", self.blades, 3, 12)?;
        let d = within("diameter", self.diameter, 10.0, 5_000.0, " mm")?;
        let pitch = within("pitch", self.pitch, 0.05 * d, 3.0 * d, " mm (0.05…3 × diameter)")?;
        let bore = within("bore", self.bore, 0.0, 0.5 * d, " mm")?;
        let rh = hub_radius(d, self.hub_diameter, 0.45 * d, bore)?;
        // By default, the roots fill 60% of the hub's circumference seen along the axis.
        let beta_h = pitch_angle(pitch, rh);
        let auto_root = 0.6 * 2.0 * PI * rh / (f64::from(self.blades) * beta_h.cos());
        let c_root = if self.root_chord > 0.0 { self.root_chord } else { auto_root };
        let c_tip = if self.tip_chord > 0.0 { self.tip_chord } else { c_root };
        within("root_chord", c_root, 0.1, d, " mm")?;
        within("tip_chord", c_tip, 0.1, d, " mm")?;
        let (t_root, t_tip) = thickness(self.root_thickness, self.tip_thickness)?;
        let camber = within("camber", self.camber, 0.0, 9.0, " %")? / 100.0;
        if let Some(duct) = &self.duct {
            within("tip_clearance", duct.clearance, 0.0, 0.1 * d, " mm")?;
            within("duct_wall", duct.wall, 0.2, d, " mm")?;
            within("duct_length", duct.length, 0.0, 10.0 * d, " mm")?;
        }
        self.detail.check()?;
        let r = d / 2.0;
        let mut stations: Vec<Station> = radii(rh, r, self.detail.sections as usize)
            .into_iter()
            .map(|ri| {
                let t = (ri - rh) / (r - rh);
                Station { camber, ..Station::foil(ri, lerp(c_root, c_tip, t), pitch_angle(pitch, ri), lerp(t_root, t_tip, t)) }
            })
            .collect();
        standard_shank(&mut stations, r - rh, 0.7);
        Ok(Rotor {
            name: format!("Ducted fan {} {}-blade", fmt(d), self.blades),
            blades: self.blades,
            radius: r,
            hub_radius: rh,
            hub_length: within("hub_length", self.hub_length, 0.0, 10.0 * d, " mm")?,
            bore,
            stations,
            suction_up: true,
            mirror: self.detail.mirror,
            points: self.detail.points as usize,
            trailing_edge: self.detail.trailing_edge,
            hub: true,
            single_blade: false,
            duct: self.duct.clone(),
        })
    }
}

/// A marine propeller (outboard, inboard, boat and ROV thrusters): the blade outline and
/// thickness of the Wageningen B-series, set by the expanded area ratio, with skew and rake.
#[derive(Clone, Debug)]
pub struct Marine {
    pub blades: u32,
    pub diameter: f64,
    pub pitch: f64,
    pub area_ratio: f64,
    pub skew: f64,
    pub rake: f64,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub bore: f64,
    pub camber: f64,
    pub thickness_scale: f64,
    pub detail: Detail,
}

/// B-series chord as c·Z / (D·EAR), divided by √(1 − r/R) so it is nearly linear, at r/R =
/// 0.2 … 0.9. Multiplying back by √(1 − r/R) gives the rounded tip.
const B_SERIES_CHORD: [(f64, f64); 8] = [(0.2, 1.858), (0.3, 2.249), (0.4, 2.646), (0.5, 3.043), (0.6, 3.458), (0.7, 3.914), (0.8, 4.405), (0.9, 5.003)];

/// The B-series tip is thinner than this; a leading edge that sharp is more than the kernel's
/// checks (and a printer) will take.
const MARINE_MIN_THICKNESS: f64 = 0.05;

/// The blade's outline stops here, short of the true tip where the chord is 0.
const MARINE_TIP: f64 = 0.99;

impl Marine {
    /// Chord at r/R = `x`, mm.
    pub fn chord(&self, x: f64) -> f64 {
        let k = &B_SERIES_CHORD;
        let g = if x <= k[0].0 {
            k[0].1 + (x - k[0].0) * (k[1].1 - k[0].1) / (k[1].0 - k[0].0)
        } else if x >= k[7].0 {
            k[7].1 + (x - k[7].0) * (k[7].1 - k[6].1) / (k[7].0 - k[6].0)
        } else {
            smooth(k, x)
        };
        g * (1.0 - x.min(MARINE_TIP)).sqrt() * self.diameter * self.area_ratio / f64::from(self.blades)
    }

    /// Maximum thickness at r/R = `x`, mm: B-series, 0.0366 D at 0.2 R to 0.003 D at the tip.
    pub fn max_thickness(&self, x: f64) -> f64 {
        self.thickness_scale * self.diameter * (0.0366 - 0.042 * (x - 0.2)).max(0.003)
    }

    pub fn rotor(&self) -> Check<Rotor> {
        whole("blades", self.blades, 2, 7)?;
        let d = within("diameter", self.diameter, 10.0, 10_000.0, " mm")?;
        let pitch = within("pitch", self.pitch, 0.3 * d, 2.5 * d, " mm (0.3…2.5 × diameter)")?;
        within("area_ratio", self.area_ratio, 0.25, 1.2, "")?;
        let skew = within("skew", self.skew, 0.0, 60.0, " degrees")?.to_radians();
        let rake = within("rake", self.rake, -10.0, 25.0, " degrees")?.to_radians();
        within("thickness_scale", self.thickness_scale, 0.3, 3.0, "")?;
        let camber = within("camber", self.camber, 0.0, 9.0, " %")? / 100.0;
        let bore = within("bore", self.bore, 0.0, 0.5 * d, " mm")?;
        let rh = hub_radius(d, self.hub_diameter, 0.2 * d, bore)?;
        self.detail.check()?;
        let r = d / 2.0;
        let xh = rh / r;
        let mut stations: Vec<Station> = radii(rh, MARINE_TIP * r, self.detail.sections as usize)
            .into_iter()
            .map(|ri| {
                let x = ri / r;
                let c = self.chord(x);
                let u = (x - xh) / (1.0 - xh);
                Station {
                    camber,
                    camber_pos: 0.5,
                    pivot: 0.5,
                    // Skew back (against the direction of travel), growing as the square of the
                    // distance from the hub; rake aft (towards -Z, downstream).
                    skew: -ri * skew * u * u,
                    rake: -(ri - rh) * rake.tan(),
                    ..Station::foil(ri, c, pitch_angle(pitch, ri), (self.max_thickness(x) / c).clamp(MARINE_MIN_THICKNESS, 0.4))
                }
            })
            .collect();
        standard_shank(&mut stations, MARINE_TIP * r - rh, 0.45);
        Ok(Rotor {
            name: format!("Marine propeller {}x{} {}-blade", fmt(d), fmt(pitch), self.blades),
            blades: self.blades,
            radius: r,
            hub_radius: rh,
            hub_length: within("hub_length", self.hub_length, 0.0, 10.0 * d, " mm")?,
            bore,
            stations,
            suction_up: true,
            mirror: self.detail.mirror,
            points: self.detail.points as usize,
            trailing_edge: self.detail.trailing_edge,
            hub: true,
            single_blade: false,
            duct: None,
        })
    }
}

/// A horizontal-axis wind turbine rotor, its blades shaped for the design tip speed ratio by
/// Schmitz's optimum (Betz with wake rotation), with a round root that blends into the airfoil.
#[derive(Clone, Debug)]
pub struct WindTurbine {
    pub blades: u32,
    pub diameter: f64,
    pub tip_speed_ratio: f64,
    pub design_cl: f64,
    pub angle_of_attack: f64,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub bore: f64,
    pub root_diameter: f64,
    pub root_transition: f64,
    pub root_thickness: f64,
    pub tip_thickness: f64,
    pub camber: f64,
    pub blade_only: bool,
    pub detail: Detail,
}

impl WindTurbine {
    /// Schmitz's optimum at radius `r`: (chord mm, blade angle rad, inflow angle rad).
    pub fn optimum(&self, r: f64) -> (f64, f64, f64) {
        let big_r = self.diameter / 2.0;
        let phi1 = (big_r / (self.tip_speed_ratio * r)).atan();
        let phi = 2.0 / 3.0 * phi1;
        let chord = 16.0 * PI * r / (f64::from(self.blades) * self.design_cl) * (phi1 / 3.0).sin().powi(2);
        (chord, phi - self.angle_of_attack.to_radians(), phi)
    }

    pub fn rotor(&self) -> Check<Rotor> {
        whole("blades", self.blades, 1, 6)?;
        let d = within("diameter", self.diameter, 50.0, 20_000.0, " mm")?;
        within("tip_speed_ratio", self.tip_speed_ratio, 1.0, 15.0, "")?;
        within("design_cl", self.design_cl, 0.2, 1.8, "")?;
        within("angle_of_attack", self.angle_of_attack, -5.0, 15.0, " degrees")?;
        let bore = within("bore", self.bore, 0.0, 0.5 * d, " mm")?;
        let rh = hub_radius(d, self.hub_diameter, 0.1 * d, bore)?;
        let r = d / 2.0;
        let xt = within("root_transition", self.root_transition, 0.05, 0.6, "")?;
        let rt = xt * r;
        if rt < rh * 1.15 {
            return Err(("root_transition", format!("must start the airfoil further out than the hub: at least {:.3}", rh * 1.15 / r)));
        }
        let (t_root, t_tip) = thickness(self.root_thickness, self.tip_thickness)?;
        let camber = within("camber", self.camber, 0.0, 9.0, " %")? / 100.0;
        self.detail.check()?;
        let (c_t, beta_t, _) = self.optimum(rt);
        // The round root: by default as wide as the airfoil it turns into is thick, with room,
        // and no wider than fits the hub.
        let auto_root = (1.15 * rh).min(0.45 * c_t).max(t_root * c_t * 1.4);
        let rd = if self.root_diameter > 0.0 { self.root_diameter } else { auto_root };
        within("root_diameter", rd, 0.5, 1.8 * rh, " mm (up to 0.9 × the hub diameter)")?;
        // Stations: round from the hub to a short way out, then blending into the airfoil until
        // the transition, then the optimum to the tip.
        let n = self.detail.sections as usize;
        let n_blend = (n / 4).max(2);
        let rc = rh + 0.25 * (rt - rh);
        let mut stations = Vec::with_capacity(n + n_blend + 1);
        let root = |ri: f64, w: f64| Station { camber, pivot: 0.3, round: 1.0 - w, round_diameter: rd, ..Station::foil(ri, c_t, beta_t, t_root) };
        stations.push(root(rh, 0.0));
        for k in 0..n_blend {
            let w = k as f64 / n_blend as f64;
            stations.push(root(rc + (rt - rc) * w, w * w * (3.0 - 2.0 * w)));
        }
        for ri in radii(rt, r, n) {
            let (c, beta, _) = self.optimum(ri);
            let u = (ri - rt) / (r - rt);
            stations.push(Station { camber, pivot: 0.3, ..Station::foil(ri, c, beta, lerp(t_root, t_tip, u)) });
        }
        Ok(Rotor {
            name: format!("Wind turbine {} TSR{} {}-blade", fmt(d), fmt(self.tip_speed_ratio), self.blades),
            blades: self.blades,
            radius: r,
            hub_radius: rh,
            hub_length: within("hub_length", self.hub_length, 0.0, 10.0 * d, " mm")?,
            bore,
            stations,
            suction_up: false,
            mirror: self.detail.mirror,
            points: self.detail.points as usize,
            trailing_edge: self.detail.trailing_edge,
            hub: !self.blade_only,
            single_blade: self.blade_only,
            duct: None,
        })
    }
}

/// A number for a name: no trailing zeros.
pub fn fmt(x: f64) -> String {
    let s = format!("{:.2}", x);
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The geometric pitch at 0.75 R, the usual way to quote a propeller's pitch.
pub fn pitch_at_75(rotor: &Rotor) -> f64 {
    let r = 0.75 * rotor.radius;
    let s = &rotor.stations;
    let i = s.iter().position(|st| st.r >= r).unwrap_or(s.len() - 1).max(1);
    let (a, b) = (&s[i - 1], &s[i]);
    let t = ((r - a.r) / (b.r - a.r)).clamp(0.0, 1.0);
    pitch_of(lerp(a.beta, b.beta, t), r)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detail() -> Detail {
        Detail { sections: 12, points: 16, trailing_edge: 0.3, mirror: false }
    }

    #[test]
    fn marine_chord_follows_the_b_series() {
        let m = Marine {
            blades: 4,
            diameter: 1000.0,
            pitch: 1000.0,
            area_ratio: 0.55,
            skew: 0.0,
            rake: 0.0,
            hub_diameter: 0.0,
            hub_length: 0.0,
            bore: 0.0,
            camber: 0.0,
            thickness_scale: 1.0,
            detail: detail(),
        };
        // Published c·Z/(D·EAR) at 0.6 R is 2.187.
        assert!((m.chord(0.6) * 4.0 / 550.0 - 2.187).abs() < 0.005);
        assert!((m.chord(0.9) * 4.0 / 550.0 - 1.582).abs() < 0.005);
        assert!((m.max_thickness(0.2) - 36.6).abs() < 1e-9);
    }

    #[test]
    fn schmitz_tip_angles() {
        let w = WindTurbine {
            blades: 3,
            diameter: 1000.0,
            tip_speed_ratio: 7.0,
            design_cl: 1.0,
            angle_of_attack: 6.0,
            hub_diameter: 0.0,
            hub_length: 0.0,
            bore: 0.0,
            root_diameter: 0.0,
            root_transition: 0.25,
            root_thickness: 0.21,
            tip_thickness: 0.12,
            camber: 4.0,
            blade_only: false,
            detail: detail(),
        };
        let (c, beta, phi) = w.optimum(500.0);
        assert!((phi.to_degrees() - 2.0 / 3.0 * (1.0f64 / 7.0).atan().to_degrees()).abs() < 1e-9);
        assert!((beta.to_degrees() - (phi.to_degrees() - 6.0)).abs() < 1e-9);
        assert!(c > 15.0 && c < 25.0, "{c}");
        let rotor = w.rotor().unwrap();
        assert_eq!(rotor.stations[0].round, 1.0);
        assert_eq!(rotor.stations.last().unwrap().round, 0.0);
        assert!((rotor.stations.last().unwrap().r - 500.0).abs() < 1e-9);
    }

    #[test]
    fn propeller_pitch_at_75_is_the_input() {
        let p = Propeller {
            blades: 2,
            diameter: 254.0,
            pitch: 114.3,
            hub_diameter: 0.0,
            hub_length: 0.0,
            bore: 5.0,
            root_chord: 0.0,
            max_chord: 0.0,
            max_chord_at: 0.35,
            tip_chord: 0.0,
            root_thickness: 0.16,
            tip_thickness: 0.08,
            camber: 4.0,
            camber_pos: 0.4,
            detail: detail(),
        };
        let r = p.rotor().unwrap();
        assert!((pitch_at_75(&r) - 114.3).abs() < 0.5, "{}", pitch_at_75(&r));
        let bad = Propeller { pitch: 2000.0, ..p.clone() };
        assert_eq!(bad.rotor().err().map(|e| e.0), Some("pitch"));
    }
}
