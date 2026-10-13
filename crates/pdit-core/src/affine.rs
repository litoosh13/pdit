//! PDF matrices [a, b, c, d, e, f] (Crop image, D-073).

/// `p` then `q`, for PDF matrices [a, b, c, d, e, f] (x' = a·x + c·y + e, y' = b·x + d·y + f).
pub fn then(p: [f32; 6], q: [f32; 6]) -> [f32; 6] {
    [
        p[0] * q[0] + p[1] * q[2],
        p[0] * q[1] + p[1] * q[3],
        p[2] * q[0] + p[3] * q[2],
        p[2] * q[1] + p[3] * q[3],
        p[4] * q[0] + p[5] * q[2] + q[4],
        p[4] * q[1] + p[5] * q[3] + q[5],
    ]
}

/// Where `m` puts the point (`x`, `y`).
pub fn at(m: [f32; 6], x: f32, y: f32) -> (f32, f32) {
    (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

pub fn invert([a, b, c, d, e, f]: [f32; 6]) -> [f32; 6] {
    let det = a * d - b * c;
    [
        d / det,
        -b / det,
        -c / det,
        a / det,
        (c * f - d * e) / det,
        (b * e - a * f) / det,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cropped_part_stays_in_place() {
        // A picture 200 × 100 pt at (50, 300), and the same turned 90°.
        for m in [
            [200.0, 0.0, 0.0, 100.0, 50.0, 300.0],
            [0.0, 200.0, -100.0, 0.0, 400.0, 100.0],
        ] {
            // Keep the right half's lower half (seen from the top-left): x 0.5..1, y (top-down) 0.5..1.
            let keep_part = [0.5, 0.0, 0.0, 0.5, 0.5, 0.0];
            let new = then(then(invert(m), keep_part), m);
            // The new picture's unit square = applying the old placement to the kept part.
            let after = then(keep_part, m);
            for (x, y) in [(0.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
                let (a, b) = at(then(m, new), x, y);
                let (c, d) = at(after, x, y);
                assert!(
                    (a - c).abs() < 1e-3 && (b - d).abs() < 1e-3,
                    "{m:?} ({x}, {y})"
                );
            }
            let (x, y) = at(after, 0.0, 0.0);
            let (ex, ey) = at(m, 0.5, 0.0);
            assert!((x - ex).abs() < 1e-3 && (y - ey).abs() < 1e-3);
        }
    }
}
