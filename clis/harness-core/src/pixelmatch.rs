//! A port of pixelmatch (YIQ colour distance with anti-aliasing detection), the pixel counter the visual gate uses.

fn colour_delta(a: &[u8], b: &[u8], k: usize) -> f64 {
    let (r1, g1, b1, a1) = (a[k] as f64, a[k + 1] as f64, a[k + 2] as f64, a[k + 3] as f64);
    let (r2, g2, b2, a2) = (b[k] as f64, b[k + 1] as f64, b[k + 2] as f64, b[k + 3] as f64);
    let (mut dr, mut dg, mut db) = (r1 - r2, g1 - g2, b1 - b2);
    let da = a1 - a2;
    if a1 < 255.0 || a2 < 255.0 {
        let rb = 48.0 + 159.0 * (k % 2) as f64;
        let gb = 48.0 + 159.0 * (((k as f64 / 1.618033988749895) as usize) % 2) as f64;
        let bb = 48.0 + 159.0 * (((k as f64 / 2.618033988749895) as usize) % 2) as f64;
        dr = (r1 * a1 - r2 * a2 - rb * da) / 255.0;
        dg = (g1 * a1 - g2 * a2 - gb * da) / 255.0;
        db = (b1 * a1 - b2 * a2 - bb * da) / 255.0;
    }
    let y = dr * 0.29889531 + dg * 0.58662247 + db * 0.11448223;
    let i = dr * 0.59597799 - dg * 0.27417610 - db * 0.32180189;
    let q = dr * 0.21147017 - dg * 0.52261711 + db * 0.31114694;
    let delta = 0.5053 * y * y + 0.299 * i * i + 0.1957 * q * q;
    if y > 0.0 {
        -delta
    } else {
        delta
    }
}

fn brightness_delta(img: &[u8], k: usize, m: usize) -> f64 {
    let (r1, g1, b1, a1) = (img[k] as f64, img[k + 1] as f64, img[k + 2] as f64, img[k + 3] as f64);
    let (r2, g2, b2, a2) = (img[m] as f64, img[m + 1] as f64, img[m + 2] as f64, img[m + 3] as f64);
    let (mut dr, mut dg, mut db) = (r1 - r2, g1 - g2, b1 - b2);
    let da = a1 - a2;
    if dr == 0.0 && dg == 0.0 && db == 0.0 && da == 0.0 {
        return 0.0;
    }
    if a1 < 255.0 || a2 < 255.0 {
        let rb = 48.0 + 159.0 * (k % 2) as f64;
        let gb = 48.0 + 159.0 * (((k as f64 / 1.618033988749895) as usize) % 2) as f64;
        let bb = 48.0 + 159.0 * (((k as f64 / 2.618033988749895) as usize) % 2) as f64;
        dr = (r1 * a1 - r2 * a2 - rb * da) / 255.0;
        dg = (g1 * a1 - g2 * a2 - gb * da) / 255.0;
        db = (b1 * a1 - b2 * a2 - bb * da) / 255.0;
    }
    dr * 0.29889531 + dg * 0.58662247 + db * 0.11448223
}

fn px(img: &[u8], i: usize) -> u32 {
    u32::from_le_bytes([img[i * 4], img[i * 4 + 1], img[i * 4 + 2], img[i * 4 + 3]])
}

fn has_many_siblings(img: &[u8], x1: usize, y1: usize, width: usize, height: usize) -> bool {
    let (x0, y0) = (x1.saturating_sub(1), y1.saturating_sub(1));
    let (x2, y2) = ((x1 + 1).min(width - 1), (y1 + 1).min(height - 1));
    let val = px(img, y1 * width + x1);
    let mut zeroes = usize::from(x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2);
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            zeroes += usize::from(val == px(img, y * width + x));
            if zeroes > 2 {
                return true;
            }
        }
    }
    false
}

fn antialiased(img: &[u8], x1: usize, y1: usize, width: usize, height: usize, a: &[u8], b: &[u8]) -> bool {
    let (x0, y0) = (x1.saturating_sub(1), y1.saturating_sub(1));
    let (x2, y2) = ((x1 + 1).min(width - 1), (y1 + 1).min(height - 1));
    let pos = (y1 * width + x1) * 4;
    let mut zeroes = usize::from(x1 == x0 || x1 == x2 || y1 == y0 || y1 == y2);
    let (mut min, mut max) = (0.0f64, 0.0f64);
    let (mut min_x, mut min_y, mut max_x, mut max_y) = (0, 0, 0, 0);
    for x in x0..=x2 {
        for y in y0..=y2 {
            if x == x1 && y == y1 {
                continue;
            }
            let delta = brightness_delta(img, pos, (y * width + x) * 4);
            if delta == 0.0 {
                zeroes += 1;
                if zeroes > 2 {
                    return false;
                }
            } else if delta < min {
                min = delta;
                min_x = x;
                min_y = y;
            } else if delta > max {
                max = delta;
                max_x = x;
                max_y = y;
            }
        }
    }
    if min == 0.0 || max == 0.0 {
        return false;
    }
    (has_many_siblings(a, min_x, min_y, width, height) && has_many_siblings(b, min_x, min_y, width, height)) || (has_many_siblings(a, max_x, max_y, width, height) && has_many_siblings(b, max_x, max_y, width, height))
}

/// The number of pixels that differ between two equally sized RGBA images (anti-aliasing excluded).
pub fn pixelmatch(img1: &[u8], img2: &[u8], width: usize, height: usize, threshold: f64) -> usize {
    diff_mask(img1, img2, width, height, threshold).iter().filter(|d| **d).count()
}

/// Per pixel, whether it differs (the pixels `pixelmatch` counts).
pub fn diff_mask(img1: &[u8], img2: &[u8], width: usize, height: usize, threshold: f64) -> Vec<bool> {
    let len = width * height;
    let mut mask = vec![false; len];
    if len == 0 || img1[..len * 4] == img2[..len * 4] {
        return mask;
    }
    let max_delta = 35215.0 * threshold * threshold;
    for (i, m) in mask.iter_mut().enumerate() {
        let pos = i * 4;
        let delta = if px(img1, i) == px(img2, i) { 0.0 } else { colour_delta(img1, img2, pos) };
        if delta.abs() > max_delta {
            let (x, y) = (i % width, i / width);
            *m = !(antialiased(img1, x, y, width, height, img1, img2) || antialiased(img2, x, y, width, height, img2, img1));
        }
    }
    mask
}
