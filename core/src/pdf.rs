//! The lesson PDF: its words with their boxes (to highlight and click), and page images.
//! One engine (pdfium) does both, so a word's box always sits exactly on the rendered page.

use crate::align::{norm, PdfWord, Rect};
use anyhow::{anyhow, Context, Result};
use once_cell::sync::OnceCell;
use pdfium_render::prelude::*;
use serde::Serialize;
use std::path::Path;

static PDFIUM: OnceCell<Pdfium> = OnceCell::new();

/// Load the pdfium library from `dir` (the app's resources), else from next to the executable or the system.
pub fn init(dir: Option<&Path>) -> Result<()> {
    if PDFIUM.get().is_some() {
        return Ok(());
    }
    let mut tried = vec![];
    let mut candidates: Vec<std::path::PathBuf> = vec![];
    if let Some(d) = dir {
        candidates.push(d.to_path_buf());
    }
    if let Ok(d) = std::env::var("RAM_PDFIUM_DIR") {
        candidates.push(d.into());
    }
    if let Some(d) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
        candidates.push(d);
    }
    for d in candidates {
        let lib = Pdfium::pdfium_platform_library_name_at_path(&d);
        match Pdfium::bind_to_library(&lib) {
            Ok(b) => {
                let _ = PDFIUM.set(Pdfium::new(b));
                return Ok(());
            }
            Err(e) => tried.push(format!("{}: {e}", lib.display())),
        }
    }
    let b = Pdfium::bind_to_system_library().map_err(|e| anyhow!("pdfium not found ({}; system: {e})", tried.join("; ")))?;
    let _ = PDFIUM.set(Pdfium::new(b));
    Ok(())
}

fn pdfium() -> Result<&'static Pdfium> {
    PDFIUM.get().ok_or_else(|| anyhow!("pdfium is not loaded"))
}

#[derive(Clone, Debug, Serialize)]
pub struct PageSize {
    /// points
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Serialize)]
pub struct PdfText {
    pub pages: Vec<PageSize>,
    pub words: Vec<PdfWord>,
}

fn is_dot(c: char) -> bool {
    matches!(c, '·' | '•' | '|')
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || is_dot(c) || c == '\u{2}' || c == '\u{fffe}'
}

struct Found {
    text: String,
    rect: Rect,
    dotted: bool,
}

/// Every word on every page, with "chunks": what the page prints as one item.
pub fn read_words(path: &Path) -> Result<PdfText> {
    let doc = pdfium()?.load_pdf_from_file(path, None).with_context(|| format!("opening {}", path.display()))?;
    let mut pages = vec![];
    let mut words = vec![];
    let mut chunk = 0u32;
    for (pi, page) in doc.pages().iter().enumerate() {
        let (pw, ph) = (page.width().value, page.height().value);
        pages.push(PageSize { w: pw, h: ph });
        let Ok(text) = page.text() else { continue };
        let mut found: Vec<Found> = vec![];
        let mut cur = String::new();
        let mut bounds: Option<(f32, f32, f32, f32)> = None; // left, bottom, right, top
        let mut dotted = false;
        let mut flush = |cur: &mut String, bounds: &mut Option<(f32, f32, f32, f32)>, dotted: &mut bool| {
            if let Some((l, b, r, t)) = bounds.take() {
                if !cur.is_empty() && !norm(cur).is_empty() && r > l && t > b {
                    found.push(Found {
                        text: std::mem::take(cur),
                        rect: Rect { x: l / pw, y: (ph - t) / ph, w: (r - l) / pw, h: (t - b) / ph },
                        dotted: *dotted,
                    });
                    *dotted = false;
                }
            }
            cur.clear();
        };
        for ch in text.chars().iter() {
            let Some(c) = ch.unicode_char() else { continue };
            if is_sep(c) {
                flush(&mut cur, &mut bounds, &mut dotted);
                if is_dot(c) {
                    dotted = true;
                }
                continue;
            }
            cur.push(c);
            if let Ok(r) = ch.loose_bounds() {
                let (l, b, rr, t) = (r.left().value, r.bottom().value, r.right().value, r.top().value);
                if rr > l && t > b {
                    bounds = Some(match bounds {
                        None => (l, b, rr, t),
                        Some((l0, b0, r0, t0)) => (l0.min(l), b0.min(b), r0.max(rr), t0.max(t)),
                    });
                }
            }
        }
        flush(&mut cur, &mut bounds, &mut dotted);

        // A new chunk after a · or |, after a wide gap on the same line (the next column of a grid),
        // and on a new line, unless the line before ran to the right edge (a sentence that wraps).
        let left = found.iter().map(|f| f.rect.x).fold(f32::INFINITY, f32::min);
        let right = found.iter().map(|f| f.rect.max_x()).fold(0.0, f32::max);
        chunk += 1;
        let mut prev: Option<Rect> = None;
        for f in found {
            if let Some(p) = prev {
                let h = p.h.min(f.rect.h) * ph; // in points, so it compares with widths
                let same_line = (p.mid_y() - f.rect.mid_y()).abs() * ph < h * 0.5;
                let wrapped = p.max_x() > right - 0.12 * (right - left);
                if f.dotted || (same_line && (f.rect.x - p.max_x()) * pw > h * 2.5) || (!same_line && !wrapped) {
                    chunk += 1;
                }
            }
            prev = Some(f.rect);
            let n = norm(&f.text);
            words.push(PdfWord { text: f.text, n, page: pi, rect: f.rect, tw: -1, chunk });
        }
    }
    Ok(PdfText { pages, words })
}

/// Page `index` as a PNG, `width` pixels wide.
pub fn render_page(path: &Path, index: usize, width: u32) -> Result<Vec<u8>> {
    let doc = pdfium()?.load_pdf_from_file(path, None)?;
    let page = doc.pages().get(index as PdfPageIndex)?;
    let cfg = PdfRenderConfig::new().set_target_width(width.clamp(200, 4000) as Pixels).render_form_data(true);
    let img = page.render_with_config(&cfg)?.as_image()?.to_rgb8();
    let mut out = Vec::with_capacity(img.len() / 4);
    let enc = image::codecs::png::PngEncoder::new_with_quality(
        &mut out,
        image::codecs::png::CompressionType::Fast,
        image::codecs::png::FilterType::Sub,
    );
    image::ImageEncoder::write_image(enc, img.as_raw(), img.width(), img.height(), image::ExtendedColorType::Rgb8)?;
    Ok(out)
}
