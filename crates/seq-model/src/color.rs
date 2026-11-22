//! Pattern colors: any RGB, picked by hue at a fixed saturation and brightness that reads
//! well on the dark grounds of both themes.

/// An sRGB color.
pub type Rgb = [u8; 3];

/// Saturation and brightness (HSV) of the colors given to patterns.
pub const PATTERN_SAT: f32 = 0.62;
pub const PATTERN_VAL: f32 = 0.96;

/// HSV to RGB. `h` in turns (0.0..1.0, wraps), `s` and `v` in 0.0..=1.0.
pub fn hsv_rgb(h: f32, s: f32, v: f32) -> Rgb {
    let h = h.rem_euclid(1.0) * 6.0;
    let (s, v) = (s.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    let q = |f: f32| ((f + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    [q(r), q(g), q(b)]
}

/// Hue of a color, in turns (0.0..1.0). Greys have hue 0.
pub fn rgb_hue(c: Rgb) -> f32 {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    if d <= 0.0 {
        return 0.0;
    }
    let h = if max == r {
        ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    h / 6.0
}

/// Pattern color of hue `h` (turns).
pub fn pattern_color(h: f32) -> Rgb {
    hsv_rgb(h, PATTERN_SAT, PATTERN_VAL)
}

/// Distance between two hues on the color wheel (0.0..=0.5 turn).
pub fn hue_distance(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(1.0);
    d.min(1.0 - d)
}

/// A random-looking pattern color for `seed`: among a few random hues, the one farthest
/// from the colors already `taken`, so a new pattern does not look like an existing one.
pub fn pick_pattern_color(seed: u64, taken: &[Rgb]) -> Rgb {
    let mut state = seed;
    let mut best = (f32::MIN, 0.0);
    for _ in 0..12 {
        let h = (splitmix(&mut state) >> 40) as f32 / (1u64 << 24) as f32;
        let gap = taken.iter().map(|&c| hue_distance(h, rgb_hue(c))).fold(1.0, f32::min);
        if gap > best.0 {
            best = (gap, h);
        }
    }
    pattern_color(best.1)
}

fn splitmix(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_primaries_and_hue_round_trip() {
        assert_eq!(hsv_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_rgb(1.0 / 3.0, 1.0, 1.0), [0, 255, 0]);
        assert_eq!(hsv_rgb(2.0 / 3.0, 1.0, 1.0), [0, 0, 255]);
        assert_eq!(hsv_rgb(1.25, 0.0, 0.5), [128, 128, 128]);
        for i in 0..36 {
            let h = i as f32 / 36.0;
            assert!(hue_distance(rgb_hue(pattern_color(h)), h) < 0.005, "hue {h}");
        }
        assert_eq!(rgb_hue([90, 90, 90]), 0.0);
    }

    #[test]
    fn hue_distance_wraps() {
        assert!((hue_distance(0.95, 0.05) - 0.1).abs() < 1e-6);
        assert!((hue_distance(0.2, 0.7) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn picked_colors_avoid_taken_hues() {
        // Same seed, same color; a different seed usually gives another one.
        assert_eq!(pick_pattern_color(7, &[]), pick_pattern_color(7, &[]));
        let distinct: std::collections::HashSet<Rgb> = (0..20).map(|s| pick_pattern_color(s, &[])).collect();
        assert!(distinct.len() > 15);
        // With many colors around, the new one keeps some distance from all of them.
        let mut taken = Vec::new();
        for seed in 0..8 {
            let c = pick_pattern_color(seed * 31 + 5, &taken);
            let gap = taken.iter().map(|&t| hue_distance(rgb_hue(t), rgb_hue(c))).fold(1.0, f32::min);
            assert!(gap > 0.03, "seed {seed}: {gap}");
            taken.push(c);
        }
        // Always the pattern saturation and brightness.
        let [r, g, b] = pick_pattern_color(3, &taken);
        assert_eq!(r.max(g).max(b), (PATTERN_VAL * 255.0).round() as u8);
    }
}
