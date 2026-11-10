//! A capture that came back as one horizontal band repeated down the image: headless Chromium sometimes answers
//! `Page.captureScreenshot` with the device frame's bottom band (rounded corners, home indicator) tiled over the whole
//! viewport while the page loads or a route changes (`docs/gaps/browser-harness-screenshot-tiled-chrome.md`). Such a
//! frame hides what the run did (a tap dot went missing from a G1 recording), so the recorder drops it and a screenshot
//! is taken again.

/// True when the grayscale image is a short band repeated vertically. A blank page is not tiled (nothing varies), and a
/// real page is not (rows a band apart differ).
pub fn is_tiled(gray: &[u8], width: usize, height: usize) -> bool {
    if width < 16 || height < 64 || gray.len() < width * height {
        return false;
    }
    let (y0, y1) = (height / 10, height * 85 / 100);
    let ys: Vec<usize> = (y0..y1).step_by(3).collect();
    let xs: Vec<usize> = (0..width).step_by((width / 64).max(1)).collect();
    let at = |x: usize, y: usize| f64::from(gray[y * width + x]);
    let n = (ys.len() * xs.len()) as f64;
    let mean = ys.iter().flat_map(|&y| xs.iter().map(move |&x| (x, y))).map(|(x, y)| at(x, y)).sum::<f64>() / n;
    let spread = (ys.iter().flat_map(|&y| xs.iter().map(move |&x| (x, y))).map(|(x, y)| (at(x, y) - mean).powi(2)).sum::<f64>() / n).sqrt();
    // a faint band (the bezel's soft gradient, spread about 2) repeats exactly; nothing real repeats that exactly
    if spread < 1.0 {
        return false;
    }
    // the band's height is rarely a whole number of pixels at the decoded scale, so each row is matched against the rows
    // one band and one band plus a pixel further down, whichever is closer
    let (lo, hi) = ((height * 3 / 100).max(2), (height * 7 / 100).max(3));
    (lo..=hi).any(|p| {
        let rows: Vec<usize> = ys.iter().copied().filter(|&y| y + p + 1 < height).collect();
        let n = (rows.len() * xs.len()) as f64;
        let cells = || rows.iter().flat_map(|&y| xs.iter().map(move |&x| (x, y)));
        let same = cells().map(|(x, y)| (at(x, y) - at(x, y + p)).abs().min((at(x, y) - at(x, y + p + 1)).abs())).sum::<f64>() / n;
        // half a band down the rows differ: a blank page, alike at every offset, does not pass this
        let half = cells().map(|(x, y)| (at(x, y) - at(x, y + p / 2)).abs()).sum::<f64>() / n;
        (spread >= 8.0 && same <= 3.0 && half >= 6.0 && half >= 3.0 * same) || (same <= 0.3 && half >= 0.5 && half >= 5.0 * same)
    })
}

/// [`is_tiled`] for a JPEG (decoded at an eighth of its size, which is enough and fast).
pub fn jpeg_is_tiled(bytes: &[u8]) -> bool {
    let mut decoder = jpeg_decoder::Decoder::new(bytes);
    if decoder.read_info().is_err() {
        return false;
    }
    let Some(info) = decoder.info() else { return false };
    let Ok((w, h)) = decoder.scale(info.width / 8, info.height / 8) else { return false };
    let Ok(pixels) = decoder.decode() else { return false };
    let Some(info) = decoder.info() else { return false };
    let (w, h) = (usize::from(w), usize::from(h));
    let gray: Vec<u8> = match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => pixels,
        jpeg_decoder::PixelFormat::RGB24 => pixels.chunks_exact(3).map(|c| ((u16::from(c[0]) * 3 + u16::from(c[1]) * 6 + u16::from(c[2])) / 10) as u8).collect(),
        _ => return false,
    };
    is_tiled(&gray, w, h)
}

/// [`is_tiled`] for a PNG.
pub fn png_is_tiled(bytes: &[u8]) -> bool {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let Ok(mut reader) = decoder.read_info() else { return false };
    let mut buf = vec![0; reader.output_buffer_size()];
    let Ok(frame) = reader.next_frame(&mut buf) else { return false };
    let (w, h) = (frame.width as usize, frame.height as usize);
    let channels = frame.color_type.samples();
    let gray: Vec<u8> = buf[..w * h * channels]
        .chunks_exact(channels)
        .map(|c| if channels >= 3 { ((u16::from(c[0]) * 3 + u16::from(c[1]) * 6 + u16::from(c[2])) / 10) as u8 } else { c[0] })
        .collect();
    is_tiled(&gray, w, h)
}
