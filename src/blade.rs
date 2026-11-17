//! Blade geometry: airfoil sections, the stations along a blade, and where each section lies in
//! the world.
//!
//! Conventions (before any mirroring for the other rotation direction):
//! - The rotor turns about +Z. Blade 0 points along +X and moves towards +Y, so it turns
//!   counter-clockwise seen from +Z.
//! - A section at radius `r` lies in the plane square to the blade's radial direction, at `r`
//!   from the axis. Its 2D coordinates are (tangential, axial): +Y and +Z for blade 0.
//! - The leading edge faces the direction of travel (+tangential) and the chord climbs towards
//!   it at the blade angle `beta`, measured from the plane of rotation.

use std::f64::consts::PI;

/// One section along the blade.
#[derive(Clone, Debug, PartialEq)]
pub struct Station {
    /// Distance from the rotor axis, mm.
    pub r: f64,
    /// Chord, mm.
    pub chord: f64,
    /// Blade angle from the plane of rotation, radians.
    pub beta: f64,
    /// Maximum thickness as a fraction of the chord.
    pub thickness: f64,
    /// Maximum camber as a fraction of the chord.
    pub camber: f64,
    /// Where the maximum camber is, as a fraction of the chord from the leading edge.
    pub camber_pos: f64,
    /// The point of the chord that lies on the blade's stacking line, as a fraction from the
    /// leading edge.
    pub pivot: f64,
    /// Tangential offset of the stacking point, mm, positive in the direction of travel.
    pub skew: f64,
    /// Axial offset of the stacking point, mm, positive along +Z.
    pub rake: f64,
    /// 0 for an airfoil, 1 for a round (oval) section, between for a blend.
    pub round: f64,
    /// Width of the round section along the chord, mm.
    pub round_diameter: f64,
    /// Its thickness across the chord, mm; 0 makes it a circle.
    pub round_thickness: f64,
}

impl Station {
    pub fn foil(r: f64, chord: f64, beta: f64, thickness: f64) -> Station {
        Station {
            r,
            chord,
            beta,
            thickness,
            camber: 0.0,
            camber_pos: 0.4,
            pivot: 0.25,
            skew: 0.0,
            rake: 0.0,
            round: 0.0,
            round_diameter: 0.0,
            round_thickness: 0.0,
        }
    }
}

/// Points along the chord, from the leading edge (0) to the trailing edge (1), closer together
/// at both ends (cosine spacing).
fn chord_points(n: usize) -> Vec<f64> {
    (0..=n).map(|i| 0.5 * (1.0 - (PI * i as f64 / n as f64).cos())).collect()
}

/// NACA 4-digit half thickness at `x` for thickness `t` (fractions of the chord), with the
/// standard open trailing edge.
pub fn naca_half_thickness(t: f64, x: f64) -> f64 {
    5.0 * t * (0.2969 * x.sqrt() - 0.1260 * x - 0.3516 * x * x + 0.2843 * x.powi(3) - 0.1015 * x.powi(4))
}

/// NACA 4-digit mean line and its slope at `x`, camber `m` at `p`.
fn naca_camber(m: f64, p: f64, x: f64) -> (f64, f64) {
    if m == 0.0 || p <= 0.0 || p >= 1.0 {
        return (0.0, 0.0);
    }
    if x < p {
        (m / (p * p) * (2.0 * p * x - x * x), 2.0 * m / (p * p) * (p - x))
    } else {
        let q = (1.0 - p) * (1.0 - p);
        (m / q * ((1.0 - 2.0 * p) + 2.0 * p * x - x * x), 2.0 * m / q * (p - x))
    }
}

/// The section's outline in chord coordinates (u from the leading edge along the chord, v
/// towards the suction side), mm: from the upper trailing edge round the leading edge to the
/// lower trailing edge, `2n + 1` points. The trailing edge is left open, at least `te` mm thick:
/// the closing line across it is the sketch's second curve.
pub fn outline(s: &Station, n: usize, te: f64) -> Vec<[f64; 2]> {
    let xs = chord_points(n);
    let c = s.chord;
    // NACA's own trailing edge is 0.021 t thick; thicken linearly towards the trailing edge to
    // reach `te`.
    let extra = (te / c - 0.021 * s.thickness).max(0.0);
    let foil = |x: f64, upper: bool| -> [f64; 2] {
        let yt = naca_half_thickness(s.thickness, x) + 0.5 * extra * x;
        let (yc, dyc) = naca_camber(s.camber, s.camber_pos, x);
        let th = dyc.atan();
        let k = if upper { 1.0 } else { -1.0 };
        [c * (x - k * yt * th.sin()), c * (yc + k * yt * th.cos())]
    };
    // The round section, an oval centred on the stacking point, indexed like the airfoil:
    // angle 0 is the trailing edge side, π the leading edge.
    let ra = 0.5 * s.round_diameter;
    let rb = if s.round_thickness > 0.0 { 0.5 * s.round_thickness } else { ra };
    let phi0 = if rb > 0.0 { (0.5 * te / rb).clamp(0.0, 0.5).asin() } else { 0.0 };
    let round = |x: f64, upper: bool| -> [f64; 2] {
        let phi = phi0 + (PI - phi0) * (1.0 - x);
        let k = if upper { 1.0 } else { -1.0 };
        [s.pivot * c + ra * phi.cos(), k * rb * phi.sin()]
    };
    let point = |x: f64, upper: bool| -> [f64; 2] {
        if s.round <= 0.0 {
            foil(x, upper)
        } else if s.round >= 1.0 {
            round(x, upper)
        } else {
            let (a, b) = (foil(x, upper), round(x, upper));
            [a[0] + s.round * (b[0] - a[0]), a[1] + s.round * (b[1] - a[1])]
        }
    };
    let mut pts = Vec::with_capacity(2 * n + 1);
    for x in xs.iter().rev() {
        pts.push(point(*x, true));
    }
    for x in xs.iter().skip(1) {
        pts.push(point(*x, false));
    }
    pts
}

/// The section's outline in its sketch plane: (tangential, axial) mm, the stacking point at
/// (`skew`, `rake`). `suction_up` puts the suction side towards +Z, as on a propeller, which
/// pushes air towards -Z; a wind turbine, driven by wind blowing towards -Z, has it towards -Z.
/// `mirror` turns the blade the other way round (tangential becomes -tangential).
pub fn placed(s: &Station, n: usize, te: f64, suction_up: bool, mirror: bool) -> Vec<[f64; 2]> {
    let (sb, cb) = s.beta.sin_cos();
    // Leading edge to trailing edge: back against the direction of travel and down the slope.
    let ec = [-cb, -sb];
    let k = if suction_up { 1.0 } else { -1.0 };
    let en = [-sb * k, cb * k];
    let m = if mirror { -1.0 } else { 1.0 };
    outline(s, n, te)
        .into_iter()
        .map(|[u, v]| {
            let a = u - s.pivot * s.chord;
            [m * (a * ec[0] + v * en[0] + s.skew), a * ec[1] + v * en[1] + s.rake]
        })
        .collect()
}

/// The sketch plane for the section at radius `r` of the blade at `angle` (radians from +X):
/// `{origin, x_dir, y_dir}`, x tangential, y along +Z, normal pointing outwards.
pub fn plane(r: f64, angle: f64) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let (s, c) = angle.sin_cos();
    ([r * c, r * s, 0.0], [-s, c, 0.0], [0.0, 0.0, 1.0])
}

/// The blade angle of a constant geometric pitch `p` mm at radius `r`.
pub fn pitch_angle(p: f64, r: f64) -> f64 {
    (p / (2.0 * PI * r)).atan()
}

/// The geometric pitch, mm, of blade angle `beta` at radius `r`.
pub fn pitch_of(beta: f64, r: f64) -> f64 {
    2.0 * PI * r * beta.tan()
}

/// `n` radii from `a` to `b`, closer together towards `b` (the tip), where the blade changes
/// fastest.
pub fn radii(a: f64, b: f64, n: usize) -> Vec<f64> {
    (0..n).map(|k| a + (b - a) * (0.5 * PI * k as f64 / (n - 1) as f64).sin()).collect()
}

/// Smooth interpolation through `(x, y)` knots sorted by x (monotone cubic Hermite, so no
/// overshoot); constant beyond the ends.
pub fn smooth(knots: &[(f64, f64)], x: f64) -> f64 {
    let n = knots.len();
    if n == 0 {
        return 0.0;
    }
    if n == 1 || x <= knots[0].0 {
        return knots[0].1;
    }
    if x >= knots[n - 1].0 {
        return knots[n - 1].1;
    }
    let slope = |i: usize| (knots[i + 1].1 - knots[i].1) / (knots[i + 1].0 - knots[i].0);
    let tangent = |i: usize| -> f64 {
        if i == 0 {
            slope(0)
        } else if i == n - 1 {
            slope(n - 2)
        } else {
            let (a, b) = (slope(i - 1), slope(i));
            if a * b <= 0.0 {
                0.0
            } else {
                // Harmonic mean (Fritsch–Butland) keeps it monotone.
                2.0 * a * b / (a + b)
            }
        }
    };
    let i = (0..n - 1).find(|&i| x < knots[i + 1].0).unwrap_or(n - 2);
    let (x0, y0, x1, y1) = (knots[i].0, knots[i].1, knots[i + 1].0, knots[i + 1].1);
    let h = x1 - x0;
    let t = (x - x0) / h;
    let (m0, m1) = (tangent(i) * h, tangent(i + 1) * h);
    let (t2, t3) = (t * t, t * t * t);
    (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * m0 + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * m1
}

/// The bounds of a set of 2D points: (min x, max x, min y, max y).
pub fn bounds(pts: &[[f64; 2]]) -> (f64, f64, f64, f64) {
    pts.iter().fold((f64::MAX, f64::MIN, f64::MAX, f64::MIN), |(a, b, c, d), p| (a.min(p[0]), b.max(p[0]), c.min(p[1]), d.max(p[1])))
}

/// Twice the area enclosed by a closed polygon, signed (positive counter-clockwise).
pub fn area2(pts: &[[f64; 2]]) -> f64 {
    let n = pts.len();
    (0..n).map(|i| pts[i][0] * pts[(i + 1) % n][1] - pts[(i + 1) % n][0] * pts[i][1]).sum()
}

/// Whether a polygon's edges cross each other (other than neighbours meeting at a vertex).
pub fn self_intersects(pts: &[[f64; 2]]) -> bool {
    let n = pts.len();
    let cross = |o: [f64; 2], a: [f64; 2], b: [f64; 2]| (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0]);
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % n]);
        for j in i + 2..n {
            if (j + 1) % n == i {
                continue;
            }
            let (c, d) = (pts[j], pts[(j + 1) % n]);
            let (d1, d2, d3, d4) = (cross(c, d, a), cross(c, d, b), cross(a, b, c), cross(a, b, d));
            if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn naca_0012_is_twelve_percent_thick() {
        let s = Station::foil(100.0, 100.0, 0.0, 0.12);
        let pts = outline(&s, 40, 0.0);
        let (_, _, lo, hi) = bounds(&pts);
        assert!((hi - lo - 12.0).abs() < 0.05, "{}", hi - lo);
        // Symmetric, open trailing edge 0.021 t thick.
        let te = pts[0][1] - pts[pts.len() - 1][1];
        assert!((te - 0.021 * 0.12 * 100.0).abs() < 1e-6, "{te}");
        assert!(!self_intersects(&pts));
    }

    #[test]
    fn trailing_edge_reaches_the_minimum() {
        let s = Station { camber: 0.04, ..Station::foil(100.0, 30.0, 0.3, 0.1) };
        let pts = outline(&s, 16, 0.5);
        let te = (pts[0][0] - pts[32][0]).hypot(pts[0][1] - pts[32][1]);
        assert!((te - 0.5).abs() < 0.02, "{te}");
        assert!(!self_intersects(&pts));
    }

    #[test]
    fn camber_lifts_the_mean_line() {
        let s = Station { camber: 0.04, camber_pos: 0.4, ..Station::foil(100.0, 100.0, 0.0, 0.12) };
        let pts = outline(&s, 40, 0.0);
        let (_, _, lo, hi) = bounds(&pts);
        assert!(hi > 9.0 && lo > -5.0, "{lo} {hi}");
    }

    #[test]
    fn placed_section_climbs_towards_the_leading_edge() {
        let s = Station { pivot: 0.5, ..Station::foil(50.0, 20.0, 30f64.to_radians(), 0.1) };
        let pts = placed(&s, 16, 0.2, true, false);
        // The leading edge is the middle point: forward (+y) and up (+z).
        let le = pts[16];
        assert!((le[0] - 10.0 * 30f64.to_radians().cos()).abs() < 1e-9);
        assert!((le[1] - 10.0 * 30f64.to_radians().sin()).abs() < 1e-9);
        let m = placed(&s, 16, 0.2, true, true);
        assert!((m[16][0] + le[0]).abs() < 1e-12 && (m[16][1] - le[1]).abs() < 1e-12);
    }

    #[test]
    fn round_section_is_a_circle_with_a_gap() {
        let s = Station { round: 1.0, round_diameter: 20.0, pivot: 0.3, ..Station::foil(10.0, 30.0, 0.0, 0.2) };
        let pts = outline(&s, 16, 0.5);
        for p in &pts {
            assert!(((p[0] - 9.0).hypot(p[1]) - 10.0).abs() < 1e-9);
        }
        assert!((pts[0][1] - pts[32][1] - 0.5).abs() < 1e-3);
    }

    #[test]
    fn smooth_hits_knots_and_stays_monotone() {
        let k = [(0.0, 1.0), (0.4, 3.0), (1.0, 0.5)];
        assert!((smooth(&k, 0.4) - 3.0).abs() < 1e-12);
        assert!(smooth(&k, 0.2) <= 3.0 && smooth(&k, 0.7) <= 3.0);
        assert_eq!(smooth(&k, -1.0), 1.0);
    }

    #[test]
    fn pitch_round_trips() {
        let b = pitch_angle(114.3, 95.25);
        assert!((pitch_of(b, 95.25) - 114.3).abs() < 1e-9);
    }
}
