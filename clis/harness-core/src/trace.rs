//! `wf browser draw-strokes-on`: the strokes a pad shows, read from its published SVG guide or off its pixels (Otsu mask, Zhang-Suen
//! thinning, skeleton walk), ported from the TypeScript daemon.
use std::collections::{HashMap, HashSet};

pub type Pt = (f64, f64);

const CURVE_STEPS: usize = 12;
const MIN_SPUR_PX: usize = 6;
const MIN_COMPONENT_PX: usize = 30;
const FRAME_PX: i64 = 3;
const DAB_PX: f64 = 1.5;

fn bezier(controls: &[Pt]) -> Vec<Pt> {
    (1..=CURVE_STEPS)
        .map(|step| {
            let t = step as f64 / CURVE_STEPS as f64;
            let mut level = controls.to_vec();
            while level.len() > 1 {
                level = level.windows(2).map(|w| (w[0].0 + (w[1].0 - w[0].0) * t, w[0].1 + (w[1].1 - w[0].1) * t)).collect();
            }
            level[0]
        })
        .collect()
}

fn tokens(d: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = d.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if "MmLlHhVvQqCcZz".contains(c) {
            out.push(c.to_string());
            i += 1;
        } else if c == '-' || c == '.' || c.is_ascii_digit() {
            let start = i;
            i += 1;
            let mut seen_dot = c == '.';
            while i < chars.len() {
                let n = chars[i];
                if n.is_ascii_digit() {
                    i += 1;
                } else if n == '.' && !seen_dot {
                    seen_dot = true;
                    i += 1;
                } else if (n == 'e' || n == 'E') && i + 1 < chars.len() && (chars[i + 1].is_ascii_digit() || chars[i + 1] == '-' || chars[i + 1] == '+') {
                    i += 2;
                } else {
                    break;
                }
            }
            out.push(chars[start..i].iter().collect());
        } else {
            i += 1;
        }
    }
    out
}

/// The strokes an SVG path draws, one per subpath, in its own units: M, L, H, V, Q, C and Z, absolute and relative.
pub fn svg_path_strokes(d: &str) -> Result<Vec<Vec<Pt>>, String> {
    let toks = tokens(d);
    let mut strokes: Vec<Vec<Pt>> = Vec::new();
    let mut current: Vec<Pt> = Vec::new();
    let (mut at, mut start) = ((0.0, 0.0), (0.0, 0.0));
    let mut command = String::new();
    let mut i = 0;
    let is_number = |t: &str| !(t.len() == 1 && t.chars().all(|c| c.is_ascii_alphabetic()));
    let finish = |current: &mut Vec<Pt>, strokes: &mut Vec<Vec<Pt>>| {
        if current.len() >= 2 {
            strokes.push(std::mem::take(current));
        } else {
            current.clear();
        }
    };
    while i < toks.len() {
        if !is_number(&toks[i]) {
            command = toks[i].clone();
            i += 1;
            if command == "Z" || command == "z" {
                current.push(start);
                at = start;
                finish(&mut current, &mut strokes);
                continue;
            }
        }
        let relative = command == command.to_lowercase();
        let base = if relative { at } else { (0.0, 0.0) };
        let number = |i: &mut usize| -> f64 {
            let v = toks.get(*i).and_then(|t| t.parse::<f64>().ok()).unwrap_or(f64::NAN);
            *i += 1;
            v
        };
        match command.to_uppercase().as_str() {
            "M" => {
                finish(&mut current, &mut strokes);
                at = (base.0 + number(&mut i), base.1 + number(&mut i));
                start = at;
                current = vec![at];
                command = if relative { "l".into() } else { "L".into() };
            }
            "L" => {
                at = (base.0 + number(&mut i), base.1 + number(&mut i));
                current.push(at);
            }
            "H" => {
                at = ((if relative { at.0 } else { 0.0 }) + number(&mut i), at.1);
                current.push(at);
            }
            "V" => {
                at = (at.0, (if relative { at.1 } else { 0.0 }) + number(&mut i));
                current.push(at);
            }
            "Q" => {
                let control = (base.0 + number(&mut i), base.1 + number(&mut i));
                let end = (base.0 + number(&mut i), base.1 + number(&mut i));
                current.extend(bezier(&[at, control, end]));
                at = end;
            }
            "C" => {
                let first = (base.0 + number(&mut i), base.1 + number(&mut i));
                let second = (base.0 + number(&mut i), base.1 + number(&mut i));
                let end = (base.0 + number(&mut i), base.1 + number(&mut i));
                current.extend(bezier(&[at, first, second, end]));
                at = end;
            }
            _ => return Err(format!("trace: unsupported path command \"{command}\" in the guide")),
        }
    }
    finish(&mut current, &mut strokes);
    Ok(strokes)
}

fn otsu(histogram: &[usize; 256], total: usize) -> usize {
    let sum: f64 = (0..256).map(|v| v as f64 * histogram[v] as f64).sum();
    let (mut below, mut below_sum, mut best, mut best_variance) = (0usize, 0.0f64, 0usize, -1.0f64);
    for (v, &count) in histogram.iter().enumerate() {
        below += count;
        if below == 0 {
            continue;
        }
        let above = total - below;
        if above == 0 {
            break;
        }
        below_sum += v as f64 * count as f64;
        let (mean_below, mean_above) = (below_sum / below as f64, (sum - below_sum) / above as f64);
        let variance = below as f64 * above as f64 * (mean_below - mean_above).powi(2);
        if variance > best_variance {
            best_variance = variance;
            best = v;
        }
    }
    best
}

/// 8-connected components of set pixels, as pixel indices.
fn components(mask: &[u8], width: usize, height: usize) -> Vec<Vec<usize>> {
    let mut seen = vec![false; mask.len()];
    let mut found = Vec::new();
    for p in 0..mask.len() {
        if mask[p] == 0 || seen[p] {
            continue;
        }
        let (mut component, mut stack) = (Vec::new(), vec![p]);
        seen[p] = true;
        while let Some(q) = stack.pop() {
            component.push(q);
            let (x, y) = ((q % width) as i64, (q / width) as i64);
            for dy in -1..=1i64 {
                for dx in -1..=1i64 {
                    let (nx, ny) = (x + dx, y + dy);
                    if nx < 0 || ny < 0 || nx >= width as i64 || ny >= height as i64 {
                        continue;
                    }
                    let n = ny as usize * width + nx as usize;
                    if mask[n] != 0 && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        found.push(component);
    }
    found
}

/// 1 where the pixel is glyph, 0 where it is pad.
pub fn glyph_mask(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let count = width * height;
    let mut luminance = vec![0u8; count];
    let mut histogram = [0usize; 256];
    for p in 0..count {
        let v = (0.299 * f64::from(rgba[p * 4]) + 0.587 * f64::from(rgba[p * 4 + 1]) + 0.114 * f64::from(rgba[p * 4 + 2])).round() as usize;
        luminance[p] = v as u8;
        histogram[v] += 1;
    }
    // The pad is the commonest shade (the first, on a tie); the glyph is whatever stands off it.
    let pad = histogram.iter().enumerate().fold((0usize, 0usize), |best, (v, c)| if *c > best.1 { (v, *c) } else { best }).0 as i64;
    let mut offset = vec![0u8; count];
    let mut offsets = [0usize; 256];
    for p in 0..count {
        let v = (i64::from(luminance[p]) - pad).unsigned_abs() as usize;
        offset[p] = v as u8;
        offsets[v] += 1;
    }
    let threshold = otsu(&offsets, count).max(6);
    let mut mask: Vec<u8> = offset.iter().map(|v| u8::from(usize::from(*v) > threshold)).collect();
    for component in components(&mask, width, height) {
        if component.len() < MIN_COMPONENT_PX {
            for p in component {
                mask[p] = 0;
            }
        }
    }
    let near_edge = |p: usize| {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        x < FRAME_PX || y < FRAME_PX || x >= width as i64 - FRAME_PX || y >= height as i64 - FRAME_PX
    };
    for component in components(&mask, width, height) {
        if component.iter().any(|p| near_edge(*p)) {
            for p in component {
                mask[p] = 0;
            }
        }
    }
    mask
}

/// Zhang-Suen thinning: peels the mask down to one-pixel centre lines.
pub fn thin(mask: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut image = mask.to_vec();
    let mut changed = true;
    while changed {
        changed = false;
        for pass in 0..2 {
            let at = |image: &[u8], x: i64, y: i64| -> u8 {
                if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
                    0
                } else {
                    image[y as usize * width + x as usize]
                }
            };
            let mut peel = Vec::new();
            for y in 0..height as i64 {
                for x in 0..width as i64 {
                    if at(&image, x, y) == 0 {
                        continue;
                    }
                    let ring = [at(&image, x, y - 1), at(&image, x + 1, y - 1), at(&image, x + 1, y), at(&image, x + 1, y + 1), at(&image, x, y + 1), at(&image, x - 1, y + 1), at(&image, x - 1, y), at(&image, x - 1, y - 1)];
                    let (p2, p4, p6, p8) = (ring[0], ring[2], ring[4], ring[6]);
                    let neighbours: u32 = ring.iter().map(|v| u32::from(*v)).sum();
                    let transitions = (0..8).filter(|k| ring[*k] == 0 && ring[(k + 1) % 8] == 1).count();
                    let keep = if pass == 0 { u32::from(p2) * u32::from(p4) * u32::from(p6) != 0 || u32::from(p4) * u32::from(p6) * u32::from(p8) != 0 } else { u32::from(p2) * u32::from(p4) * u32::from(p8) != 0 || u32::from(p2) * u32::from(p6) * u32::from(p8) != 0 };
                    if (2..=6).contains(&neighbours) && transitions == 1 && !keep {
                        peel.push(y as usize * width + x as usize);
                    }
                }
            }
            for p in &peel {
                image[*p] = 0;
            }
            if !peel.is_empty() {
                changed = true;
            }
        }
    }
    image
}

fn skeleton_strokes(skeleton: &[u8], width: usize, height: usize) -> Vec<Vec<Pt>> {
    let set = |x: i64, y: i64| x >= 0 && y >= 0 && x < width as i64 && y < height as i64 && skeleton[y as usize * width + x as usize] != 0;
    let neighbours = |p: usize| -> Vec<usize> {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        let idx = |nx: i64, ny: i64| ny as usize * width + nx as usize;
        let mut found = Vec::new();
        for (dx, dy) in [(0, -1), (1, 0), (0, 1), (-1, 0)] {
            if set(x + dx, y + dy) {
                found.push(idx(x + dx, y + dy));
            }
        }
        for (dx, dy) in [(1, -1), (1, 1), (-1, 1), (-1, -1)] {
            if set(x + dx, y + dy) && !set(x + dx, y) && !set(x, y + dy) {
                found.push(idx(x + dx, y + dy));
            }
        }
        found
    };
    let point = |p: usize| ((p % width) as f64, (p / width) as f64);
    let pixels: Vec<usize> = (0..skeleton.len()).filter(|p| skeleton[*p] != 0).collect();
    let degree: HashMap<usize, usize> = pixels.iter().map(|p| (*p, neighbours(*p).len())).collect();
    let is_node = |p: usize| degree.get(&p).copied() != Some(2);
    let mut walked: HashSet<(usize, usize)> = HashSet::new();
    let edge = |a: usize, b: usize| if a < b { (a, b) } else { (b, a) };
    let mut strokes: Vec<(Vec<Pt>, bool)> = Vec::new();
    let walk = |from: usize, next: usize, walked: &mut HashSet<(usize, usize)>| -> Vec<usize> {
        let mut path = vec![from, next];
        walked.insert(edge(from, next));
        let (mut previous, mut current) = (from, next);
        while !is_node(current) && current != from {
            let Some(onward) = neighbours(current).into_iter().find(|n| *n != previous && !walked.contains(&edge(current, *n))) else { break };
            walked.insert(edge(current, onward));
            path.push(onward);
            previous = current;
            current = onward;
        }
        path
    };
    for p in &pixels {
        if !is_node(*p) {
            continue;
        }
        for n in neighbours(*p) {
            if walked.contains(&edge(*p, n)) {
                continue;
            }
            let path = walk(*p, n, &mut walked);
            let (first, last) = (path[0], path[path.len() - 1]);
            let junctions = [first, last].iter().filter(|q| degree.get(q).copied().unwrap_or(0) >= 3).count();
            let free = [first, last].iter().any(|q| degree.get(q).copied() == Some(1));
            let noise = path.len() < MIN_SPUR_PX && ((free && junctions == 1) || junctions == 2);
            strokes.push((path.iter().map(|q| point(*q)).collect(), noise));
        }
    }
    for p in &pixels {
        let Some(open) = neighbours(*p).into_iter().find(|n| !walked.contains(&edge(*p, *n))) else { continue };
        let path = walk(*p, open, &mut walked);
        strokes.push((path.iter().map(|q| point(*q)).collect(), false));
    }
    strokes.into_iter().filter(|(points, spur)| !spur && points.len() >= 2).map(|(points, _)| points).collect()
}

fn key(p: Pt) -> (i64, i64) {
    (p.0 as i64, p.1 as i64)
}

/// Joins strokes wherever exactly two of them meet: a line that noise split.
fn join_at_passes(strokes: Vec<Vec<Pt>>) -> Vec<Vec<Pt>> {
    let mut joined = strokes;
    loop {
        // Insertion-ordered ends, as a JavaScript Map keeps them.
        let mut order: Vec<(i64, i64)> = Vec::new();
        let mut ends: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (index, stroke) in joined.iter().enumerate() {
            for end in [stroke[0], stroke[stroke.len() - 1]] {
                let entry = ends.entry(key(end)).or_insert_with(|| {
                    order.push(key(end));
                    Vec::new()
                });
                entry.push(index);
            }
        }
        let mut did = false;
        for at in &order {
            let incident = &ends[at];
            if incident.len() != 2 || incident[0] == incident[1] {
                continue;
            }
            let (i, j) = (incident[0], incident[1]);
            let (a, b) = (joined[i].clone(), joined[j].clone());
            let into: Vec<Pt> = if key(a[a.len() - 1]) == *at { a } else { a.into_iter().rev().collect() };
            let onward: Vec<Pt> = if key(b[0]) == *at { b } else { b.into_iter().rev().collect() };
            joined[i] = into.into_iter().chain(onward.into_iter().skip(1)).collect();
            joined.remove(j);
            did = true;
            break;
        }
        if !did {
            return joined;
        }
    }
}

fn distance_to_segment(p: Pt, a: Pt, b: Pt) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length = dx * dx + dy * dy;
    let t = if length == 0.0 { 0.0 } else { (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length).clamp(0.0, 1.0) };
    (p.0 - (a.0 + t * dx)).hypot(p.1 - (a.1 + t * dy))
}

/// Douglas-Peucker: drops points within `epsilon` of the line through their neighbours.
pub fn simplify_path(points: &[Pt], epsilon: f64) -> Vec<Pt> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let (first, last) = (points[0], points[points.len() - 1]);
    let (mut farthest, mut index) = (0.0, 0);
    for (k, p) in points.iter().enumerate().take(points.len() - 1).skip(1) {
        let d = distance_to_segment(*p, first, last);
        if d > farthest {
            farthest = d;
            index = k;
        }
    }
    if farthest <= epsilon {
        return vec![first, last];
    }
    let mut left = simplify_path(&points[..=index], epsilon);
    left.pop();
    left.extend(simplify_path(&points[index..], epsilon));
    left
}

fn dabs(mask: &[u8], width: usize, strokes: &[Vec<Pt>]) -> Vec<Vec<Pt>> {
    let height = mask.len() / width;
    let touched: HashSet<i64> = strokes.iter().flatten().map(|p| p.1.round() as i64 * width as i64 + p.0.round() as i64).collect();
    components(mask, width, height)
        .into_iter()
        .filter(|pixels| !pixels.iter().any(|p| touched.contains(&(*p as i64))))
        .map(|pixels| {
            let n = pixels.len() as f64;
            let x = pixels.iter().map(|p| (p % width) as f64).sum::<f64>() / n;
            let y = pixels.iter().map(|p| (p / width) as f64).sum::<f64>() / n;
            vec![(x - DAB_PX, y), (x + DAB_PX, y)]
        })
        .collect()
}

/// Letter by letter, left to right; within a letter, top to bottom, then left to right; each stroke from its upper end.
fn order_strokes(strokes: Vec<Vec<Pt>>, mask: &[u8], width: usize) -> Vec<Vec<Pt>> {
    let height = mask.len() / width;
    let mut letter_of = vec![-1i64; mask.len()];
    let mut letters: Vec<(Vec<usize>, usize)> = components(mask, width, height).into_iter().map(|pixels| {
        let left = pixels.iter().map(|p| p % width).min().unwrap_or(width);
        (pixels, left)
    }).collect();
    letters.sort_by_key(|(_, left)| *left);
    for (index, (pixels, _)) in letters.iter().enumerate() {
        for p in pixels {
            letter_of[*p] = index as i64;
        }
    }
    let letter_for = |stroke: &Vec<Pt>| -> i64 {
        let at = stroke[0];
        letter_of.get(at.1.round() as usize * width + at.0.round() as usize).copied().unwrap_or(-1)
    };
    let top = |s: &Vec<Pt>| s.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let left = |s: &Vec<Pt>| s.iter().map(|p| p.0).fold(f64::INFINITY, f64::min);
    let mut oriented: Vec<Vec<Pt>> = strokes
        .into_iter()
        .map(|stroke| {
            let (first, last) = (stroke[0], stroke[stroke.len() - 1]);
            let level = (first.1 - last.1).abs() < 4.0;
            let reverse = if level { last.0 < first.0 } else { last.1 < first.1 };
            if reverse { stroke.into_iter().rev().collect() } else { stroke }
        })
        .collect();
    oriented.sort_by(|a, b| {
        letter_for(a).cmp(&letter_for(b)).then(top(a).partial_cmp(&top(b)).unwrap_or(std::cmp::Ordering::Equal)).then(left(a).partial_cmp(&left(b)).unwrap_or(std::cmp::Ordering::Equal))
    });
    oriented
}

/// What stands out of the pad as ink: the faint template (a shade off the pad's commonest, by Otsu) or the colourful guide
/// (the only saturated pixels, as in the lesson pad's green guide beside its grey outline).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Select {
    Template,
    Guide,
}

/// RGBA pixels, width and height of a PNG.
pub fn decode_rgba(png_bytes: &[u8]) -> Result<(Vec<u8>, usize, usize), String> {
    let mut decoder = png::Decoder::new(png_bytes);
    decoder.set_transformations(png::Transformations::EXPAND);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let (width, height) = (info.width as usize, info.height as usize);
    let rgba: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => buf[..info.buffer_size()].to_vec(),
        png::ColorType::Rgb => buf[..info.buffer_size()].chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        png::ColorType::Grayscale => buf[..info.buffer_size()].iter().flat_map(|g| [*g, *g, *g, 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf[..info.buffer_size()].chunks(2).flat_map(|c| [c[0], c[0], c[0], c[1]]).collect(),
        png::ColorType::Indexed => return Err("trace: indexed screenshot".into()),
    };
    Ok((rgba, width, height))
}

/// 1 where the pixel is a saturated colour (channel spread of 24 or more); specks are dropped.
fn saturated_mask(rgba: &[u8], width: usize, height: usize) -> Vec<u8> {
    let mut mask: Vec<u8> = (0..width * height)
        .map(|p| {
            let (r, g, b) = (rgba[p * 4], rgba[p * 4 + 1], rgba[p * 4 + 2]);
            u8::from(r.max(g).max(b) - r.min(g).min(b) >= 24 && g >= b)
        })
        .collect();
    for component in components(&mask, width, height) {
        if component.len() < MIN_COMPONENT_PX {
            for p in component {
                mask[p] = 0;
            }
        }
    }
    mask
}

/// The template strokes inside the CSS-pixel box `[x, y, w, h]` of a whole-viewport PNG that is `viewport_css_w` CSS pixels
/// wide; the strokes come back in CSS pixels relative to the box.
pub fn template_strokes_in_viewport(png_bytes: &[u8], viewport_css_w: f64, [bx, by, bw, bh]: [f64; 4]) -> Result<Vec<Vec<Pt>>, String> {
    let (rgba, width, height) = decode_rgba(png_bytes)?;
    let scale = if viewport_css_w > 0.0 { width as f64 / viewport_css_w } else { 1.0 };
    // Read the template at twice the CSS resolution when the capture has it: at 1x a letter's small inner loop (the ல of
    // இலை at phone width) thins away and the word is traced without it (run g2-browser-1792028944, NOT YET 65 three times).
    let k = if scale >= 2.0 { 2.0 } else { 1.0 };
    let (w, h) = ((bw * k).round().max(1.0) as usize, (bh * k).round().max(1.0) as usize);
    let step = scale / k;
    let mut small = vec![255u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = (((bx * k + x as f64) * step) as usize, ((by * k + y as f64) * step) as usize);
            let (x1, y1) = ((((bx * k + x as f64 + 1.0) * step) as usize).max(x0 + 1), (((by * k + y as f64 + 1.0) * step) as usize).max(y0 + 1));
            let mut sum = [0u32; 4];
            let mut n = 0u32;
            for sy in y0..y1.min(height) {
                for sx in x0..x1.min(width) {
                    let i = (sy * width + sx) * 4;
                    for c in 0..4 {
                        sum[c] += u32::from(rgba[i + c]);
                    }
                    n += 1;
                }
            }
            let o = (y * w + x) * 4;
            for c in 0..4 {
                if let Some(v) = sum[c].checked_div(n) {
                    small[o + c] = v as u8;
                }
            }
        }
    }
    Ok(strokes_from_rgba(&small, w, h, Select::Template).into_iter().map(|s| s.into_iter().map(|(x, y)| (x / k, y / k)).collect()).collect())
}

/// The strokes of what `select` picks out of an RGBA crop of the pad.
pub fn strokes_from_rgba(rgba: &[u8], width: usize, height: usize, select: Select) -> Vec<Vec<Pt>> {
    let mask = match select {
        Select::Template => glyph_mask(rgba, width, height),
        Select::Guide => saturated_mask(rgba, width, height),
    };
    let skeleton = thin(&mask, width, height);
    let strokes: Vec<Vec<Pt>> = join_at_passes(skeleton_strokes(&skeleton, width, height)).into_iter().map(|s| simplify_path(&s, 0.75)).collect();
    let mut all = strokes.clone();
    all.extend(dabs(&mask, width, &strokes));
    order_strokes(all, &mask, width)
}

fn centroid(points: &[usize], width: usize) -> Pt {
    let n = points.len() as f64;
    (points.iter().map(|p| (p % width) as f64).sum::<f64>() / n, points.iter().map(|p| (p / width) as f64).sum::<f64>() / n)
}

struct Edge {
    from: usize,
    to: usize,
    pixels: Vec<usize>,
}

fn turn(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (na, nb) = (a.0.hypot(a.1).max(1e-9), b.0.hypot(b.1).max(1e-9));
    ((a.0 * b.0 + a.1 * b.1) / (na * nb)).clamp(-1.0, 1.0).acos()
}

/// The single trail along a guide's thinned line that covers every segment of it, from the node at `dot` to the free end
/// nearest `arrow`, turning as little as possible where lines meet (a stroke that crosses itself goes straight through the
/// crossing, and loops hanging off the start are drawn first). `None` when the line has no such trail.
fn guide_trail(skeleton: &[u8], width: usize, height: usize, dot: Pt, arrow: Pt) -> Option<Vec<Pt>> {
    let at = |p: usize| ((p % width) as f64, (p / width) as f64);
    let on = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && skeleton[y as usize * width + x as usize] == 1;
    let ring: [(i64, i64); 8] = [(-1, -1), (0, -1), (1, -1), (1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0)];
    let neighbours = |p: usize| -> Vec<usize> {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        ring.iter().filter(|(dx, dy)| on(x + dx, y + dy)).map(|(dx, dy)| (y + dy) as usize * width + (x + dx) as usize).collect()
    };
    // A crossing is where the 8-neighbourhood of a line pixel has three or more separate runs of line.
    let crossing = |p: usize| -> bool {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        let around: Vec<bool> = ring.iter().map(|(dx, dy)| on(x + dx, y + dy)).collect();
        (0..8).filter(|i| !around[*i] && around[(i + 1) % 8]).count() >= 3 || around.iter().filter(|b| **b).count() >= 4 && (0..8).filter(|i| !around[*i] && around[(i + 1) % 8]).count() >= 3
    };
    let line: Vec<usize> = (0..width * height).filter(|p| skeleton[*p] == 1).collect();
    let mut junction = vec![false; width * height];
    for p in &line {
        if crossing(*p) {
            for dy in -2i64..=2 {
                for dx in -2i64..=2 {
                    let (x, y) = ((p % width) as i64 + dx, (p / width) as i64 + dy);
                    if on(x, y) {
                        junction[y as usize * width + x as usize] = true;
                    }
                }
            }
        }
    }
    let mut nodes: Vec<Vec<usize>> = Vec::new();
    let mut node_of: HashMap<usize, usize> = HashMap::new();
    for p in line.iter().filter(|p| junction[**p]) {
        if node_of.contains_key(p) {
            continue;
        }
        let id = nodes.len();
        let mut members = vec![*p];
        node_of.insert(*p, id);
        let mut i = 0;
        while i < members.len() {
            for n in neighbours(members[i]) {
                if junction[n] && !node_of.contains_key(&n) {
                    node_of.insert(n, id);
                    members.push(n);
                }
            }
            i += 1;
        }
        nodes.push(members);
    }
    // Crossings a few pixels apart are one crossing drawn thick.
    let centre = |ids: &[usize]| -> Pt {
        let n = ids.len() as f64;
        (ids.iter().map(|p| at(*p).0).sum::<f64>() / n, ids.iter().map(|p| at(*p).1).sum::<f64>() / n)
    };
    let mut merged: Vec<usize> = (0..nodes.len()).collect();
    for i in 0..nodes.len() {
        for j in 0..i {
            let (a, b) = (centre(&nodes[i]), centre(&nodes[j]));
            if (a.0 - b.0).hypot(a.1 - b.1) < 25.0 {
                let keep = merged[j];
                let drop = merged[i];
                for m in merged.iter_mut() {
                    if *m == drop {
                        *m = keep;
                    }
                }
            }
        }
    }
    for p in line.iter().filter(|p| junction[**p]) {
        let id = merged[node_of[p]];
        node_of.insert(*p, id);
    }
    let mut node_ids: Vec<usize> = merged.clone();
    node_ids.sort_unstable();
    node_ids.dedup();
    let mut members_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (p, id) in &node_of {
        members_of.entry(*id).or_default().push(*p);
    }
    // Fragments: what is left of the line between crossings.
    let free: Vec<usize> = line.iter().copied().filter(|p| !junction[*p]).collect();
    let mut seen: HashSet<usize> = HashSet::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut terminals: Vec<(usize, usize)> = Vec::new(); // (node id, pixel) of free line ends
    let mut next_id = nodes.len();
    for start in &free {
        if seen.contains(start) {
            continue;
        }
        let mut comp = vec![*start];
        seen.insert(*start);
        let mut i = 0;
        while i < comp.len() {
            for n in neighbours(comp[i]) {
                if !junction[n] && seen.insert(n) {
                    comp.push(n);
                }
            }
            i += 1;
        }
        if comp.len() < 4 {
            continue;
        }
        let in_comp: HashSet<usize> = comp.iter().copied().collect();
        let ends: Vec<usize> = comp.iter().copied().filter(|p| neighbours(*p).iter().filter(|n| in_comp.contains(n)).count() <= 1).collect();
        if ends.len() != 2 {
            return None;
        }
        // Walk the fragment from one end to the other.
        let mut pixels = vec![ends[0]];
        let mut visited: HashSet<usize> = HashSet::from([ends[0]]);
        while *pixels.last()? != ends[1] {
            let cur = *pixels.last()?;
            let nxt = neighbours(cur).into_iter().filter(|n| in_comp.contains(n) && !visited.contains(n)).min_by_key(|n| {
                let (a, b) = (at(*n), at(cur));
                ((a.0 - b.0).abs() + (a.1 - b.1).abs()) as i64
            })?;
            visited.insert(nxt);
            pixels.push(nxt);
        }
        let attach = |end: usize, terminals: &mut Vec<(usize, usize)>, next_id: &mut usize| -> usize {
            let touching = neighbours(end).into_iter().find_map(|n| node_of.get(&n).copied());
            touching.unwrap_or_else(|| {
                *next_id += 1;
                terminals.push((*next_id - 1, end));
                *next_id - 1
            })
        };
        let from = attach(ends[0], &mut terminals, &mut next_id);
        let to = attach(ends[1], &mut terminals, &mut next_id);
        edges.push(Edge { from, to, pixels });
    }
    if edges.is_empty() || edges.len() > 14 {
        return None;
    }
    let node_centre = |id: usize| -> Pt {
        members_of.get(&id).map_or_else(|| terminals.iter().find(|(n, _)| *n == id).map_or((0.0, 0.0), |(_, p)| at(*p)), |m| centre(m))
    };
    let nearest = |target: Pt, ids: &[usize]| -> Option<usize> {
        ids.iter().copied().min_by(|a, b| {
            let (da, db) = ((node_centre(*a).0 - target.0).hypot(node_centre(*a).1 - target.1), (node_centre(*b).0 - target.0).hypot(node_centre(*b).1 - target.1));
            da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
        })
    };
    let all_nodes: Vec<usize> = node_ids.iter().copied().chain(terminals.iter().map(|(n, _)| *n)).collect();
    let start = nearest(dot, &all_nodes)?;
    let end = nearest(arrow, &terminals.iter().map(|(n, _)| *n).collect::<Vec<_>>())?;
    // Heading of an edge when it is left from its `from` end (forward) or its `to` end (reversed), over its first dozen pixels.
    let heading = |e: &Edge, forward: bool| -> (f64, f64) {
        let px: Vec<Pt> = if forward { e.pixels.iter().map(|p| at(*p)).collect() } else { e.pixels.iter().rev().map(|p| at(*p)).collect() };
        let k = px.len().min(12) - 1;
        (px[k].0 - px[0].0, px[k].1 - px[0].1)
    };
    let tail = |e: &Edge, forward: bool| -> (f64, f64) {
        let px: Vec<Pt> = if forward { e.pixels.iter().map(|p| at(*p)).collect() } else { e.pixels.iter().rev().map(|p| at(*p)).collect() };
        let k = px.len().min(12) - 1;
        (px[px.len() - 1].0 - px[px.len() - 1 - k].0, px[px.len() - 1].1 - px[px.len() - 1 - k].1)
    };
    struct Search<'a> {
        edges: &'a [Edge],
        end: usize,
        best: Option<(f64, Vec<(usize, bool)>)>,
        heading: &'a dyn Fn(&Edge, bool) -> (f64, f64),
        tail: &'a dyn Fn(&Edge, bool) -> (f64, f64),
    }
    fn dfs(s: &mut Search, node: usize, arriving: Option<(f64, f64)>, used: &mut Vec<bool>, trail: &mut Vec<(usize, bool)>, cost: f64) {
        if s.best.as_ref().is_some_and(|(c, _)| cost >= *c) {
            return;
        }
        if used.iter().all(|u| *u) {
            if node == s.end {
                s.best = Some((cost, trail.clone()));
            }
            return;
        }
        for i in 0..s.edges.len() {
            if used[i] {
                continue;
            }
            for forward in [true, false] {
                let (from, to) = if forward { (s.edges[i].from, s.edges[i].to) } else { (s.edges[i].to, s.edges[i].from) };
                if from != node || (from == to && !forward && false) {
                    continue;
                }
                let leaving = (s.heading)(&s.edges[i], forward);
                let add = arriving.map_or(0.0, |a| turn(a, leaving));
                used[i] = true;
                trail.push((i, forward));
                dfs(s, to, Some((s.tail)(&s.edges[i], forward)), used, trail, cost + add);
                trail.pop();
                used[i] = false;
            }
        }
    }
    let mut search = Search { edges: &edges, end, best: None, heading: &heading, tail: &tail };
    dfs(&mut search, start, None, &mut vec![false; edges.len()], &mut Vec::new(), 0.0);
    let (_, trail) = search.best?;
    let mut path: Vec<Pt> = vec![node_centre(start)];
    for (i, forward) in trail {
        let px: Vec<Pt> = if forward { edges[i].pixels.iter().map(|p| at(*p)).collect() } else { edges[i].pixels.iter().rev().map(|p| at(*p)).collect() };
        path.extend(px);
        let to = if forward { edges[i].to } else { edges[i].from };
        path.push(node_centre(to));
    }
    Some(simplify_path(&path, 0.75))
}

/// A walk over the whole guide line from `start` that ends at the pixel nearest `arrow`: the line's spanning tree is toured
/// branch by branch, the branch leading to the arrow last. For guides whose crossings `guide_trail` cannot untangle; the
/// walk retraces the side branches, which the checker accepts (it measures the distance to the line and the end points).
fn cover_tour(skeleton: &[u8], width: usize, height: usize, start: usize, arrow: Pt) -> Option<Vec<Pt>> {
    let on = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && skeleton[y as usize * width + x as usize] == 1;
    let neighbours = |p: usize| -> Vec<usize> {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        let mut out = Vec::new();
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if (dx, dy) != (0, 0) && on(x + dx, y + dy) {
                    out.push((y + dy) as usize * width + (x + dx) as usize);
                }
            }
        }
        out
    };
    let at = |p: usize| ((p % width) as f64, (p / width) as f64);
    let mut parent: HashMap<usize, usize> = HashMap::from([(start, start)]);
    let mut children: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut stack = vec![start];
    while let Some(cur) = stack.pop() {
        for n in neighbours(cur) {
            if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(n) {
                e.insert(cur);
                children.entry(cur).or_default().push(n);
                stack.push(n);
            }
        }
    }
    let target = parent.keys().copied().min_by(|a, b| {
        let (da, db) = ((at(*a).0 - arrow.0).hypot(at(*a).1 - arrow.1), (at(*b).0 - arrow.0).hypot(at(*b).1 - arrow.1));
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    let mut to_arrow: HashSet<usize> = HashSet::from([target]);
    let mut cur = target;
    while cur != start {
        cur = parent[&cur];
        to_arrow.insert(cur);
    }
    // Iterative tour: frames are (node, next child index, children ordered with the arrow's branch last).
    let ordered = |n: usize| -> Vec<usize> {
        let mut c = children.get(&n).cloned().unwrap_or_default();
        c.sort_by_key(|k| to_arrow.contains(k));
        c
    };
    let mut path = vec![start];
    let mut frames: Vec<(usize, Vec<usize>, usize)> = vec![(start, ordered(start), 0)];
    while let Some((node, kids, i)) = frames.last().cloned() {
        if i < kids.len() {
            frames.last_mut()?.2 += 1;
            let kid = kids[i];
            path.push(kid);
            frames.push((kid, ordered(kid), 0));
        } else {
            frames.pop();
            if let Some((up, _, _)) = frames.last() {
                if to_arrow.contains(&node) {
                    break;
                }
                path.push(*up);
            }
        }
    }
    Some(simplify_path(&path.iter().map(|p| at(*p)).collect::<Vec<Pt>>(), 0.75))
}

/// The path a lesson pad's guide marks: from its dark start dot along the skeleton of the guide line, straight on through
/// every junction, to the end nearest the dark arrow. `None` when the pad shows no guide line or no dot.
pub fn guide_walk(rgba: &[u8], width: usize, height: usize) -> Option<Vec<Pt>> {
    let mask = saturated_mask(rgba, width, height);
    let skeleton = thin(&mask, width, height);
    let dark: Vec<u8> = (0..width * height).map(|p| u8::from(mask[p] == 1 && 0.299 * f64::from(rgba[p * 4]) + 0.587 * f64::from(rgba[p * 4 + 1]) + 0.114 * f64::from(rgba[p * 4 + 2]) < 150.0)).collect();
    let mut blobs = components(&dark, width, height);
    blobs.sort_by_key(|b| std::cmp::Reverse(b.len()));
    let dot = centroid(blobs.first()?, width);
    let arrow = blobs.get(1).filter(|b| b.len() >= 12).map(|b| centroid(b, width));
    let on = |x: i64, y: i64| x >= 0 && y >= 0 && (x as usize) < width && (y as usize) < height && skeleton[y as usize * width + x as usize] == 1;
    let neighbours = |p: usize| -> Vec<usize> {
        let (x, y) = ((p % width) as i64, (p / width) as i64);
        let mut out = Vec::new();
        for dy in -1..=1i64 {
            for dx in -1..=1i64 {
                if (dx, dy) != (0, 0) && on(x + dx, y + dy) {
                    out.push((y + dy) as usize * width + (x + dx) as usize);
                }
            }
        }
        out
    };
    let at = |p: usize| ((p % width) as f64, (p / width) as f64);
    let start = (0..width * height).filter(|p| skeleton[*p] == 1).min_by(|a, b| {
        let (da, db) = (((at(*a).0 - dot.0).hypot(at(*a).1 - dot.1)), ((at(*b).0 - dot.0).hypot(at(*b).1 - dot.1)));
        da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
    })?;
    if let Some(a) = arrow {
        if let Some(path) = guide_trail(&skeleton, width, height, dot, a) {
            return Some(path);
        }
        // The arrow marks where the stroke ends: the shortest way along the line from the dot to it.
        let target = (0..width * height).filter(|p| skeleton[*p] == 1).min_by(|x, y| {
            let (dx, dy) = ((at(*x).0 - a.0).hypot(at(*x).1 - a.1), (at(*y).0 - a.0).hypot(at(*y).1 - a.1));
            dx.partial_cmp(&dy).unwrap_or(std::cmp::Ordering::Equal)
        })?;
        let mut parent: HashMap<usize, usize> = HashMap::new();
        let mut queue = std::collections::VecDeque::from([start]);
        parent.insert(start, start);
        while let Some(cur) = queue.pop_front() {
            if cur == target {
                break;
            }
            for n in neighbours(cur) {
                if let std::collections::hash_map::Entry::Vacant(e) = parent.entry(n) {
                    e.insert(cur);
                    queue.push_back(n);
                }
            }
        }
        if parent.contains_key(&target) {
            let mut path = vec![target];
            while *path.last()? != start {
                path.push(parent[path.last()?]);
            }
            path.reverse();
            // A shortest way that skips much of the line is not the stroke the checker measures against; tour the whole line.
            let total = skeleton.iter().filter(|v| **v == 1).count();
            if (path.len() as f64) < 0.6 * total as f64 {
                if let Some(tour) = cover_tour(&skeleton, width, height, start, a) {
                    return Some(tour);
                }
            }
            return Some(simplify_path(&path.iter().map(|p| at(*p)).collect::<Vec<Pt>>(), 0.75));
        }
    }
    let mut best: Option<(f64, Vec<usize>)> = None;
    for first in neighbours(start) {
        let mut path = vec![start, first];
        let mut seen: HashSet<usize> = path.iter().copied().collect();
        seen.extend(neighbours(start));
        loop {
            let cur = *path.last()?;
            let back = path[path.len().saturating_sub(7)];
            let (hx, hy) = (at(cur).0 - at(back).0, at(cur).1 - at(back).1);
            let norm = hx.hypot(hy).max(1e-9);
            let next = neighbours(cur).into_iter().filter(|n| !seen.contains(n)).max_by(|a, b| {
                let score = |n: &usize| ((at(*n).0 - at(cur).0) * hx + (at(*n).1 - at(cur).1) * hy) / norm;
                score(a).partial_cmp(&score(b)).unwrap_or(std::cmp::Ordering::Equal)
            });
            let Some(next) = next else { break };
            seen.extend(neighbours(cur));
            seen.insert(next);
            path.push(next);
        }
        let end = at(*path.last()?);
        let cost = arrow.map_or(-(path.len() as f64), |a| (end.0 - a.0).hypot(end.1 - a.1) - path.len() as f64 * 0.01);
        if best.as_ref().is_none_or(|(c, _)| cost < *c) {
            best = Some((cost, path));
        }
    }
    let (_, path) = best?;
    Some(simplify_path(&path.iter().map(|p| at(*p)).collect::<Vec<Pt>>(), 0.75))
}
