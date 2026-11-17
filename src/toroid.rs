//! Toroidal propellers: each blade is a closed loop. A leading arm leaves the hub forward and
//! ahead in the direction of rotation, runs out to a tip bridge, and comes back to the hub as a
//! trailing arm, aft and behind. The loop is one loft through airfoil sections square to its
//! centreline, each turned to the blade angle of the propeller's pitch.
//!
//! A section's chord turns from the blade angle on the leading arm,
//! through axial at the tip, to the blade angle on the trailing arm, so both arms meet the air
//! leading edge first. The section's two faces swap sides on the way round (the face on top of
//! the leading arm is underneath on the trailing arm), so the sections are symmetric: a cambered
//! airfoil would be cambered the wrong way on one arm.
//!
//! Conventions as for the other rotors: the rotor turns about +Z, counter-clockwise seen from
//! +Z unless mirrored, and pushes air towards -Z.

use crate::blade::{bounds, placed, self_intersects, Station};
use crate::design::{fmt, Check};
use std::f64::consts::PI;

/// A toroidal propeller.
#[derive(Clone, Debug)]
pub struct Toroid {
    pub marine: bool,
    pub loops: u32,
    pub diameter: f64,
    pub pitch: f64,
    pub hub_diameter: f64,
    pub hub_length: f64,
    pub bore: f64,
    /// Chord where the arms meet the hub, and at the tip bridge, mm.
    pub root_chord: f64,
    pub tip_chord: f64,
    /// Thickness as a fraction of the chord, at the roots and at the tip.
    pub root_thickness: f64,
    pub tip_thickness: f64,
    /// The angle between a loop's two roots, as a fraction of the spacing between loops.
    pub spread: f64,
    /// Axial distance between a loop's two roots, mm.
    pub offset: f64,
    pub sections: u32,
    pub points: u32,
    pub trailing_edge: f64,
    pub mirror: bool,
}

/// A point and its sketch plane: (origin, x_dir, y_dir).
pub type Plane = ([f64; 3], [f64; 3], [f64; 3]);

/// The loop worked out, ready to build.
#[derive(Clone, Debug)]
pub struct Loop {
    pub name: String,
    pub hub_radius: f64,
    /// Each section's plane and its outline in that plane, from the leading arm's root to the
    /// trailing arm's.
    pub sections: Vec<(Plane, Vec<[f64; 2]>)>,
    /// Section facts for the reply: (distance along the loop as a fraction, radius, chord,
    /// chord angle from the plane of rotation in degrees, axial position).
    pub table: Vec<[f64; 5]>,
    /// The pads at the two roots, in the plane of the root's radial direction (x along the
    /// direction of rotation, y along +Z): (angle, y0, y1, z0, z1), from `r_in` to `r_pad`.
    pub pads: [(f64, f64, f64, f64, f64); 2],
    pub r_in: f64,
    pub r_pad: f64,
    pub hub_z: (f64, f64),
    pub axial_extent: (f64, f64),
    pub warnings: Vec<String>,
}

fn within(field: &'static str, v: f64, lo: f64, hi: f64, unit: &str) -> Check<f64> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(v)
    } else {
        Err((field, format!("must be {lo}…{hi}{unit}")))
    }
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let n = dot(a, a).sqrt();
    if n > 0.0 {
        scale(a, 1.0 / n)
    } else {
        a
    }
}

/// Radial and tangential unit vectors at `angle`.
fn radial(angle: f64) -> [f64; 3] {
    [angle.cos(), angle.sin(), 0.0]
}

fn tangential(angle: f64) -> [f64; 3] {
    [-angle.sin(), angle.cos(), 0.0]
}

const Z: [f64; 3] = [0.0, 0.0, 1.0];

impl Toroid {
    fn hub_radius(&self) -> Check<f64> {
        let d = self.diameter;
        let auto = if self.marine { 0.24 * d } else { 0.12 * d };
        let h = if self.hub_diameter > 0.0 { self.hub_diameter } else { auto.max(self.bore + 4.0) };
        if !(h.is_finite() && h > self.bore + 1.0) {
            return Err(("hub_diameter", format!("must be more than the bore + 1 mm ({:.3} mm)", self.bore + 1.0)));
        }
        if h >= 0.6 * d {
            return Err(("hub_diameter", format!("must be less than 60% of the diameter ({:.3} mm)", 0.6 * d)));
        }
        Ok(h / 2.0)
    }

    pub fn auto_root_chord(&self) -> f64 {
        if self.root_chord > 0.0 {
            self.root_chord
        } else if self.marine {
            0.15 * self.diameter
        } else {
            0.1 * self.diameter
        }
    }

    pub fn auto_tip_chord(&self) -> f64 {
        if self.tip_chord > 0.0 {
            self.tip_chord
        } else {
            0.6 * self.auto_root_chord()
        }
    }

    /// Work out the loop.
    pub fn design(&self) -> Check<Loop> {
        let (lo, hi) = if self.marine { (2, 6) } else { (2, 8) };
        if !(lo..=hi).contains(&self.loops) {
            return Err(("loops", format!("must be {lo}…{hi}")));
        }
        let d = within("diameter", self.diameter, 20.0, 10_000.0, " mm")?;
        let pitch = within("pitch", self.pitch, 0.1 * d, 3.0 * d, " mm (0.1…3 × diameter)")?;
        let bore = within("bore", self.bore, 0.0, 0.5 * d, " mm")?;
        let rh = self.hub_radius()?;
        let c_root = within("root_chord", self.auto_root_chord(), 0.5, 0.5 * d, " mm")?;
        let c_tip = within("tip_chord", self.auto_tip_chord(), 0.5, 0.5 * d, " mm")?;
        let t_root = within("root_thickness", self.root_thickness, 0.04, 0.3, "")?;
        let t_tip = within("tip_thickness", self.tip_thickness, 0.04, 0.3, "")?;
        let spread = within("spread", self.spread, 0.2, 0.9, " (fraction of the spacing between loops)")?;
        let offset = if self.offset > 0.0 { self.offset } else { c_root };
        let offset = within("axial_offset", offset, 0.0, d, " mm")?;
        within("hub_length", self.hub_length, 0.0, 10.0 * d, " mm")?;
        within("trailing_edge", self.trailing_edge, 0.0, 5.0, " mm")?;
        if !(9..=45).contains(&self.sections) {
            return Err(("sections", "must be 9…45".into()));
        }
        if !(6..=60).contains(&self.points) {
            return Err(("section_points", "must be 6…60".into()));
        }
        let n = self.points as usize;
        let te = self.trailing_edge;
        let r = d / 2.0;
        // The tip bridge's chord runs axially, so the loop's outer edge is its centreline plus
        // half the tip's thickness.
        let rt = r - 0.5 * t_tip * c_tip;
        if rt <= rh * 1.3 {
            return Err(("hub_diameter", "the hub leaves no room for the loops: make it smaller or the diameter larger".into()));
        }
        let dtheta = spread * 2.0 * PI / f64::from(self.loops);
        let (ta, tb) = (0.5 * dtheta, -0.5 * dtheta);
        let (za, zb) = (0.5 * offset, -0.5 * offset);
        let m = if self.mirror { -1.0 } else { 1.0 };
        let mirror3 = |p: [f64; 3]| [p[0], m * p[1], p[2]];
        let mirror_plane = |(o, x, y): Plane| (mirror3(o), mirror3(x), mirror3(y));

        // A section of the given chord, thickness and chord angle, centred on its point.
        let section = |c: f64, t: f64, alpha: f64| -> Vec<[f64; 2]> {
            let s = Station { pivot: 0.5, ..Station::foil(0.0, c, alpha, t) };
            placed(&s, n, te, true, false)
        };
        // Chord and thickness by s = sin(π t): the roots at s = 0, the tip at s = 1.
        let chord = |s: f64| c_root + (c_tip - c_root) * s;
        let thick = |s: f64| t_root + (t_tip - t_root) * s;

        // The root sections, square to the radial direction, and their pads.
        let root_a = section(c_root, t_root, (pitch / (2.0 * PI * rh)).atan());
        let (y0, y1, z0, z1) = bounds(&root_a);
        let mg = 0.06 * (z1 - z0).max(y1 - y0);
        let wy = (y0 - mg).abs().max((y1 + mg).abs());
        if wy >= 0.9 * rh {
            return Err((
                "hub_diameter",
                format!("the loop roots are {:.3} mm across; make the hub diameter more than {:.3} mm or the root chord smaller", 2.0 * wy, 2.0 * wy / 0.9),
            ));
        }
        let r_in = 0.9 * (rh * rh - wy * wy).sqrt();
        if r_in < bore / 2.0 + 0.15 * (rh - bore / 2.0) {
            return Err(("bore", "the loop roots would reach the bore: make the bore smaller, the hub larger or the root chord smaller".into()));
        }
        let r_pad = (rh * rh + wy * wy).sqrt() + 0.02 * rh;
        if rt <= r_pad * 1.15 {
            return Err(("hub_diameter", "the root pads leave no room for the loops: make the hub or the root chord smaller".into()));
        }

        // The centreline: r rises as √sin(πt) so both ends leave the hub radially, and the angle
        // and axial position move from root A to root B with zero slope at both ends.
        let q = 0.5;
        let s0 = ((r_pad - rh) / (rt - rh)).powf(1.0 / q).clamp(0.0, 1.0);
        let t0 = s0.asin() / PI;
        let point = |t: f64| -> [f64; 3] {
            let s = (PI * t).sin().max(0.0);
            let g = 0.5 * (1.0 - (PI * t).cos());
            let rr = rh + (rt - rh) * s.powf(q);
            let th = ta + (tb - ta) * g;
            add(scale(radial(th), rr), [0.0, 0.0, za + (zb - za) * g])
        };
        let count = self.sections as usize;
        let mut sections = Vec::with_capacity(count);
        let mut table = Vec::with_capacity(count);
        for k in 0..count {
            let t = t0 + (1.0 - 2.0 * t0) * k as f64 / (count - 1) as f64;
            let s = (PI * t).sin();
            let c = chord(s);
            let (plane, alpha, p) = if k == 0 || k == count - 1 {
                // The ends lie exactly on the pads' faces.
                let (th, z, sign) = if k == 0 { (ta, za, 1.0) } else { (tb, zb, -1.0) };
                let o = add(scale(radial(th), r_pad), [0.0, 0.0, z]);
                let v = scale(tangential(th), sign);
                let alpha = pitch.atan2(sign * 2.0 * PI * r_pad);
                ((o, v, Z), alpha, o)
            } else {
                let p = point(t);
                let h = 1e-5;
                let tan = unit(add(point(t + h), scale(point(t - h), -1.0)));
                let w = unit(add(Z, scale(tan, -dot(Z, tan))));
                let v = cross(w, tan);
                let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
                let th = p[1].atan2(p[0]);
                // How far the section's x axis runs along the direction of rotation: 1 on the
                // leading arm's root, 0 across the tip bridge, -1 on the trailing arm's root.
                // The chord takes the blade angle of a helix of the pitch, scaled by that, so
                // it turns smoothly through axial at the tip. (Projecting the helix itself onto
                // the section fails at the tip, where the bridge runs nearly along it.)
                let e = dot(v, tangential(th));
                let alpha = pitch.atan2(2.0 * PI * rr * e);
                ((p, v, w), alpha, p)
            };
            let pts = section(c, thick(s), alpha);
            if self_intersects(&pts) {
                return Err(("trailing_edge", format!("a section {:.3} mm long folds over itself: lower trailing_edge or the thickness", c)));
            }
            let rr = (p[0] * p[0] + p[1] * p[1]).sqrt();
            table.push([(t - t0) / (1.0 - 2.0 * t0), rr, c, alpha.to_degrees(), p[2]]);
            sections.push((mirror_plane(plane), pts));
        }

        // Pads, in each root's standard radial plane (x along rotation). Root B's section was
        // drawn with x against rotation; flip it into that frame.
        let pad_of = |pts: &[[f64; 2]], z: f64, flip: bool| -> (f64, f64, f64, f64) {
            let f = if flip { -1.0 } else { 1.0 };
            let moved: Vec<[f64; 2]> = pts.iter().map(|p| [f * p[0], p[1] + z]).collect();
            let (a, b, c, d) = bounds(&moved);
            let mg = 0.06 * (d - c).max(b - a);
            (a - mg, b + mg, c - mg, d + mg)
        };
        let pa = pad_of(&sections[0].1, za, false);
        let pb = pad_of(&sections[count - 1].1, zb, true);
        for (y0, y1, ..) in [pa, pb] {
            if y0.abs().max(y1.abs()) >= 0.9 * rh {
                return Err(("hub_diameter", "the loop roots are too wide for this hub: make it larger or the root chord smaller".into()));
            }
        }
        // The pads' footprints on the hub, as (arc along the circumference, axial) boxes, must
        // not overlap: a loop's own two roots, nor one loop's trailing root and the next loop's
        // leading root.
        // (The sections themselves, without the pads' margin: pads may overlap, they all merge
        // into the hub, but two loop ends crossing each other make a loop the kernel refuses.)
        let unpad = |(y0, y1, z0, z1): (f64, f64, f64, f64)| {
            // pad_of added 6% of the larger side to each side: take it off again.
            let m = 0.06 / 1.12 * (z1 - z0).max(y1 - y0);
            (y0 + m, y1 - m, z0 + m, z1 - m)
        };
        let foot = |th: f64, b: (f64, f64, f64, f64)| {
            let (y0, y1, z0, z1) = unpad(b);
            (th * rh + y0, th * rh + y1, z0, z1)
        };
        let overlap = |a: (f64, f64, f64, f64), b: (f64, f64, f64, f64)| a.0 < b.1 && b.0 < a.1 && a.2 < b.3 && b.2 < a.3;
        let next_a = foot(ta - 2.0 * PI / f64::from(self.loops), pa);
        if overlap(foot(ta, pa), foot(tb, pb)) {
            return Err(("spread", "a loop's two roots overlap on the hub: increase spread or axial_offset, or make the root chord smaller".into()));
        }
        if overlap(foot(tb, pb), next_a) {
            return Err(("spread", "neighbouring loops' roots overlap on the hub: decrease spread, use fewer loops, or make the root chord smaller".into()));
        }
        // Mirroring turns the pads' angles and swaps the sides of their rectangles.
        let pad = |th: f64, (y0, y1, z0, z1): (f64, f64, f64, f64)| if self.mirror { (-th, -y1, -y0, z0, z1) } else { (th, y0, y1, z0, z1) };
        let pads = [pad(ta, pa), pad(tb, pb)];

        let (lo, hi) = (pa.2.min(pb.2), pa.3.max(pb.3));
        let span = hi - lo;
        let len = if self.hub_length > 0.0 {
            if self.hub_length < span + 0.2 {
                return Err(("hub_length", format!("must be at least {:.3} mm to hold the loop roots (0 = automatic)", span + 0.2)));
            }
            self.hub_length
        } else {
            span + 2.0 * (0.06 * span).max(0.5)
        };
        let mid = 0.5 * (lo + hi);
        let hub_z = (mid - len / 2.0, mid + len / 2.0);

        // The overall axial extent: each section's points lifted into 3D.
        let mut ext = (f64::MAX, f64::MIN);
        for ((o, x, y), pts) in &sections {
            for p in pts {
                let z = o[2] + p[0] * x[2] + p[1] * y[2];
                ext = (ext.0.min(z), ext.1.max(z));
            }
        }
        let mut warnings = Vec::new();
        // Neighbouring loops: root B of one and root A of the next are this far apart.
        let gap = 2.0 * PI / f64::from(self.loops) - dtheta;
        if gap * rh < 0.5 * c_root {
            warnings.push("neighbouring loops' roots are close together on the hub; consider a smaller spread or fewer loops".to_string());
        }
        let kind = if self.marine { "Toroidal marine propeller" } else { "Toroidal propeller" };
        Ok(Loop {
            name: format!("{kind} {}x{} {}-loop", fmt(d), fmt(pitch), self.loops),
            hub_radius: rh,
            sections,
            table,
            pads,
            r_in,
            r_pad,
            hub_z,
            axial_extent: ext,
            warnings,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toroid() -> Toroid {
        Toroid {
            marine: false,
            loops: 3,
            diameter: 254.0,
            pitch: 114.3,
            hub_diameter: 0.0,
            hub_length: 0.0,
            bore: 5.0,
            root_chord: 0.0,
            tip_chord: 0.0,
            root_thickness: 0.12,
            tip_thickness: 0.1,
            spread: 0.55,
            offset: 0.0,
            sections: 25,
            points: 14,
            trailing_edge: 0.3,
            mirror: false,
        }
    }

    #[test]
    fn ends_sit_on_the_pads_and_the_tip_reaches_the_diameter() {
        let l = toroid().design().unwrap();
        let (o, x, y) = l.sections[0].0;
        assert!(((o[0] * o[0] + o[1] * o[1]).sqrt() - l.r_pad).abs() < 1e-9);
        assert_eq!(y, Z);
        // Root A's x runs along rotation; root B's against it.
        assert!(dot(x, tangential(o[1].atan2(o[0]))) > 0.999);
        let (ob, xb, _) = l.sections[l.sections.len() - 1].0;
        assert!(dot(xb, tangential(ob[1].atan2(ob[0]))) < -0.999);
        // Root A is forward (+Z), root B aft.
        assert!(o[2] > 0.0 && ob[2] < 0.0);
        let rmax = l.table.iter().map(|s| s[1]).fold(0.0, f64::max);
        assert!(rmax > 0.97 * 127.0 && rmax < 127.0, "{rmax}");
    }

    #[test]
    fn chord_turns_through_axial_at_the_tip() {
        let l = toroid().design().unwrap();
        let first = l.table[0][3];
        let mid = l.table[l.table.len() / 2][3];
        let last = l.table[l.table.len() - 1][3];
        assert!(first > 0.0 && first < 90.0, "{first}");
        assert!((mid - 90.0).abs() < 15.0, "{mid}");
        assert!((last - (180.0 - first)).abs() < 1e-9, "{last}");
        // Continuous on the way round: always between flat and reversed, and no flips.
        for w in l.table.windows(2) {
            assert!(w[1][3] > 0.0 && w[1][3] < 180.0);
            assert!((w[1][3] - w[0][3]).abs() < 40.0, "{} -> {}", w[0][3], w[1][3]);
        }
    }

    #[test]
    fn mirror_swaps_sides() {
        let a = toroid().design().unwrap();
        let b = Toroid { mirror: true, ..toroid() }.design().unwrap();
        let (oa, ob) = (a.sections[5].0 .0, b.sections[5].0 .0);
        assert!((oa[1] + ob[1]).abs() < 1e-9 && (oa[0] - ob[0]).abs() < 1e-9);
        assert!((a.pads[0].0 + b.pads[0].0).abs() < 1e-12);
    }

    #[test]
    fn bad_inputs() {
        assert_eq!(Toroid { loops: 1, ..toroid() }.design().err().map(|e| e.0), Some("loops"));
        assert_eq!(Toroid { spread: 1.5, ..toroid() }.design().err().map(|e| e.0), Some("spread"));
        assert_eq!(Toroid { hub_diameter: 200.0, ..toroid() }.design().err().map(|e| e.0), Some("hub_diameter"));
        assert_eq!(Toroid { sections: 5, ..toroid() }.design().err().map(|e| e.0), Some("sections"));
        // Wide roots, close together and level: the loop's own roots collide.
        let wide = Toroid { marine: true, loops: 5, diameter: 643.0, pitch: 553.0, spread: 0.47, offset: 51.4, ..toroid() };
        assert_eq!(wide.design().err().map(|e| e.0), Some("spread"));
        assert!(Toroid { offset: 200.0, ..wide }.design().is_ok());
    }
}
