//! Inline visual previews for image files and Mermaid diagrams.
//!
//! The renderer deliberately has a no-network surface: it reads local files
//! the caller already selected, and Mermaid diagrams are rendered only by a
//! local `mmdc` executable when one is on PATH. Pixels are always painted with
//! the Unicode half-block fallback, so the same output survives native
//! terminals, ttyd/xterm.js, and `nebula ssh`.
//!
//! On a host with an image protocol the real image then goes over those cells
//! ([`ImagePlacement`], encoded off the loop since it takes a few hundred
//! milliseconds, written by [`sync_image`] once a frame is out). The half-blocks
//! underneath are what keep that safe: they show until the image lands, ratatui's
//! diff leaves them alone while it is up, a host that ignored the sequence still
//! shows a picture, and anything else drawn there rewrites them and so erases
//! the image.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use image::codecs::png::{CompressionType, PngEncoder};
use image::{DynamicImage, ExtendedColorType, ImageBuffer, ImageEncoder, Rgba, RgbaImage};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::theme::Theme;
use crate::view_jobs::{Answer, Cancel, Jobs};

pub const INLINE_GRAPHICS_MODES: &[&str] =
    &["auto", "kitty", "iterm", "sixel", "halfblocks", "off"];
pub const DEFAULT_INLINE_GRAPHICS: &str = "auto";

const MAX_MARKDOWN_DIAGRAM_ROWS: u16 = 40;

/// A cell's size in pixels on a host that reports none: the half-block
/// renderer's one-by-two cell, at about a Retina terminal's resolution.
const FALLBACK_CELL: (u32, u32) = (16, 32);

/// A half-block cell with nothing to show. Not a plain space: every cell under
/// an image has to differ from a blank one, or ratatui's diff would leave a
/// piece of the image behind when a blank is drawn there.
const EMPTY_PIXELS: &str = "\u{a0}";

/// The id nebula's one image goes by on a kitty-protocol host.
const KITTY_IMAGE_ID: u32 = 7878;
/// The protocol's cap on one escape sequence's payload.
const KITTY_CHUNK_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphicsMode {
    Off,
    Halfblocks,
    Iterm,
    Kitty,
}

impl GraphicsMode {
    fn has_protocol(&self) -> bool {
        matches!(self, Self::Iterm | Self::Kitty)
    }
}

pub fn graphics_mode(value: &str) -> GraphicsMode {
    match value.trim().to_ascii_lowercase().as_str() {
        "off" => GraphicsMode::Off,
        "iterm" => GraphicsMode::Iterm,
        "kitty" => GraphicsMode::Kitty,
        "auto" => detected_mode(
            &std::env::var("TERM_PROGRAM").unwrap_or_default(),
            &std::env::var("TERM").unwrap_or_default(),
        ),
        _ => GraphicsMode::Halfblocks,
    }
}

/// What `auto` picks: the protocol the host speaks out of the box, by what it
/// says it is. WezTerm has kitty's too, but only behind its
/// `enable_kitty_graphics`. A multiplexer in between hides the host
/// (`TERM_PROGRAM=tmux`) and keeps the half-blocks.
fn detected_mode(term_program: &str, term: &str) -> GraphicsMode {
    match (term_program, term) {
        ("WezTerm" | "iTerm.app", _) => GraphicsMode::Iterm,
        ("ghostty", _) | (_, "xterm-kitty" | "xterm-ghostty") => GraphicsMode::Kitty,
        _ => GraphicsMode::Halfblocks,
    }
}

pub fn mode_label(value: &str) -> &'static str {
    INLINE_GRAPHICS_MODES
        .iter()
        .copied()
        .find(|mode| mode.eq_ignore_ascii_case(value.trim()))
        .unwrap_or(DEFAULT_INLINE_GRAPHICS)
}

pub fn is_image_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg")
    )
}

pub fn is_mermaid_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("mmd"))
}

#[derive(Debug, Clone)]
pub struct RasterImage {
    width: u32,
    height: u32,
    /// Shared: the views are cloned every frame, and the encode runs off the loop.
    pixels: Arc<Vec<[u8; 4]>>,
}

impl RasterImage {
    fn from_dynamic(image: DynamicImage) -> Self {
        let rgba = image.to_rgba8();
        let (width, height) = rgba.dimensions();
        Self {
            width,
            height,
            pixels: Arc::new(rgba.pixels().map(|p| p.0).collect()),
        }
    }

    fn from_png_bytes(bytes: &[u8]) -> Result<Self, String> {
        image::load_from_memory(bytes)
            .map(Self::from_dynamic)
            .map_err(|err| format!("couldn't decode rendered diagram: {err}"))
    }
}

#[derive(Debug, Clone)]
pub struct VisualRender {
    area: Rect,
    mode: GraphicsMode,
    canvas: Color,
    lines: Vec<Line<'static>>,
    /// The encode running for this render, by ticket, and its way to stop.
    encode: Option<(u64, Cancel)>,
    image: Option<ImagePlacement>,
}

/// A preview's real image: the cells it covers and the escape sequence that
/// draws it. Two placements are the same image only when they share that
/// sequence, which is what tells [`sync_image`] nothing needs writing.
#[derive(Debug, Clone)]
pub struct ImagePlacement {
    rect: Rect,
    mode: GraphicsMode,
    sequence: Arc<Vec<u8>>,
}

impl PartialEq for ImagePlacement {
    fn eq(&self, other: &Self) -> bool {
        self.rect == other.rect && Arc::ptr_eq(&self.sequence, &other.sequence)
    }
}

impl ImagePlacement {
    /// Whether the half-blocks under the image are all still on top: anything
    /// drawn over them later in the frame (the floating editor, a toast)
    /// shows up here as another symbol.
    pub fn is_uncovered(&self, buffer: &Buffer) -> bool {
        self.rect.positions().all(|position| {
            buffer
                .cell(position)
                .is_some_and(|cell| matches!(cell.symbol(), "▀" | "▄" | EMPTY_PIXELS))
        })
    }
}

/// Bring the host up to date with the frame just drawn, `shown` being what it
/// has now: a new image goes out once, and stays until the wanted one changes.
/// The cursor is put back because the sequence moves it, and the frame parked
/// it where a CJK input method composes (`draw_frame`).
pub fn sync_image(
    wanted: &Option<ImagePlacement>,
    shown: &mut Option<ImagePlacement>,
    w: &mut impl Write,
) -> std::io::Result<()> {
    if wanted == shown {
        return Ok(());
    }
    // A kitty image floats above the cells, so redrawing them doesn't erase it.
    if shown
        .take()
        .is_some_and(|old| old.mode == GraphicsMode::Kitty)
    {
        write!(w, "\x1b_Ga=d,d=I,i={KITTY_IMAGE_ID},q=2\x1b\\")?;
    }
    if let Some(image) = wanted {
        write!(w, "\x1b7\x1b[{};{}H", image.rect.y + 1, image.rect.x + 1)?;
        w.write_all(&image.sequence)?;
        w.write_all(b"\x1b8")?;
    }
    *shown = wanted.clone();
    w.flush()
}

#[derive(Debug, Clone)]
pub struct VisualPreview {
    pub title: String,
    pub raster: Option<RasterImage>,
    pub message: Option<String>,
    pub source: Option<String>,
    pub rendered: Option<VisualRender>,
}

impl VisualPreview {
    pub fn image(path: &Path) -> Self {
        match load_image(path) {
            Ok(raster) => Self {
                title: image_title(path),
                raster: Some(raster),
                message: None,
                source: None,
                rendered: None,
            },
            Err(message) => Self {
                title: image_title(path),
                raster: None,
                message: Some(message),
                source: None,
                rendered: None,
            },
        }
    }

    pub fn mermaid(source: String) -> Self {
        let rendered = render_mermaid(&source);
        let (raster, message) = match rendered {
            MermaidOutput::Image(raster) => (Some(raster), None),
            MermaidOutput::Hint(hint) => (None, Some(hint)),
        };
        Self {
            title: "Mermaid diagram".into(),
            raster,
            message,
            source: Some(source),
            rendered: None,
        }
    }

    pub fn render(
        &mut self,
        area: Rect,
        mode: GraphicsMode,
        th: Theme,
        jobs: Option<&Jobs>,
    ) -> Vec<Line<'static>> {
        if area.width == 0 || area.height == 0 {
            return Vec::new();
        }
        if mode == GraphicsMode::Off {
            self.rendered = None;
            return self.fallback_lines(
                th,
                "inline graphics are off (set `inline_graphics` to `auto` or `halfblocks`)",
            );
        }
        let Some(raster) = &self.raster else {
            return self.fallback_lines(
                th,
                self.message
                    .as_deref()
                    .unwrap_or("couldn't render this visual preview"),
            );
        };
        if let Some(cached) = &self.rendered {
            if cached.area == area && cached.mode == mode && cached.canvas == th.canvas {
                return cached.lines.clone();
            }
        }
        if let Some((_, cancel)) = self.rendered.as_ref().and_then(|r| r.encode.as_ref()) {
            cancel.cancel();
        }
        let cell = cell_pixels();
        let placed = fit(raster, area, cell);
        let lines = raster_to_lines(raster, placed, placed.x - area.x, th);
        let mut render = VisualRender {
            area,
            mode: mode.clone(),
            canvas: th.canvas,
            lines: lines.clone(),
            encode: None,
            image: None,
        };
        let canvas = th.canvas;
        match jobs {
            Some(jobs) if mode.has_protocol() => {
                let (ticket, cancel) = (crate::view_jobs::ticket(), Cancel::default());
                render.encode = Some((ticket, cancel.clone()));
                let raster = raster.clone();
                jobs.run(move || {
                    if cancel.is_cancelled() {
                        return None;
                    }
                    let image = encode_image(&raster, placed, cell, &mode, canvas);
                    Some(Answer::Image { ticket, image })
                });
            }
            Some(_) => {}
            None => render.image = encode_image(raster, placed, cell, &mode, canvas),
        }
        self.rendered = Some(render);
        lines
    }

    /// The real image for the area last rendered, once it is encoded, on a
    /// host with a protocol.
    pub fn placement(&self) -> Option<ImagePlacement> {
        self.rendered.as_ref()?.image.clone()
    }

    /// An encode came back: kept when it is the last render's, dropped when
    /// the area has moved on since.
    pub fn land_image(&mut self, ticket: u64, image: Option<ImagePlacement>) {
        if let Some(render) = &mut self.rendered {
            if render.encode.as_ref().is_some_and(|(id, _)| *id == ticket) {
                render.encode = None;
                render.image = image;
            }
        }
    }

    fn fallback_lines(&self, th: Theme, hint: &str) -> Vec<Line<'static>> {
        let mut lines = vec![Line::from(Span::styled(
            hint.to_string(),
            Style::default().fg(th.warn),
        ))];
        if let Some(source) = &self.source {
            lines.push(Line::default());
            lines.extend(source.lines().map(|line| Line::from(line.to_string())));
        }
        lines
    }
}

#[derive(Debug, Clone)]
pub struct MermaidDiagram {
    pub source: String,
    pub visual: VisualPreview,
}

pub fn extract_mermaid_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut lines = markdown.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let fence = if trimmed.starts_with("```") {
            '`'
        } else if trimmed.starts_with("~~~") {
            '~'
        } else {
            continue;
        };
        let fence_len = trimmed.chars().take_while(|c| *c == fence).count();
        let info = trimmed[fence_len..].trim();
        if !info
            .split_whitespace()
            .next()
            .is_some_and(|lang| lang.eq_ignore_ascii_case("mermaid"))
        {
            continue;
        }
        let mut source = String::new();
        for body in lines.by_ref() {
            let close = body.trim_start();
            if close.chars().take_while(|c| *c == fence).count() >= fence_len {
                break;
            }
            if !source.is_empty() {
                source.push('\n');
            }
            source.push_str(body);
        }
        blocks.push(source);
    }
    blocks
}

pub fn render_mermaid_blocks(markdown: &str) -> Vec<MermaidDiagram> {
    extract_mermaid_blocks(markdown)
        .into_iter()
        .map(|source| MermaidDiagram {
            visual: VisualPreview::mermaid(source.clone()),
            source,
        })
        .collect()
}

pub fn markdown_diagram_lines(
    source: &str,
    diagrams: &mut [MermaidDiagram],
    width: u16,
    mode: GraphicsMode,
    th: Theme,
) -> Option<Vec<Line<'static>>> {
    let diagram = diagrams
        .iter_mut()
        .find(|diagram| diagram.source == source)?;
    let rows = diagram_rows(diagram.visual.raster.as_ref(), width);
    let area = Rect::new(0, 0, width, rows);
    // A diagram scrolls with its page, which a placed image can't follow.
    let mode = match mode {
        GraphicsMode::Off => GraphicsMode::Off,
        _ => GraphicsMode::Halfblocks,
    };
    Some(diagram.visual.render(area, mode, th, None))
}

fn diagram_rows(raster: Option<&RasterImage>, width: u16) -> u16 {
    let Some(raster) = raster else {
        return 8;
    };
    if raster.width == 0 || raster.height == 0 || width == 0 {
        return 1;
    }
    let pixel_h = (raster.height as f32 * width as f32 / raster.width as f32).ceil() as u16;
    pixel_h.div_ceil(2).clamp(1, MAX_MARKDOWN_DIAGRAM_ROWS)
}

pub fn load_image(path: &Path) -> Result<RasterImage, String> {
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"))
    {
        return load_svg(path);
    }
    image::ImageReader::open(path)
        .map_err(|err| format!("couldn't read image: {err}"))?
        .with_guessed_format()
        .map_err(|err| format!("couldn't identify image format: {err}"))?
        .decode()
        .map(RasterImage::from_dynamic)
        .map_err(|err| format!("couldn't decode image: {err}"))
}

fn load_svg(path: &Path) -> Result<RasterImage, String> {
    let try_rsvg = std::process::Command::new("rsvg-convert")
        .arg("-f")
        .arg("png")
        .arg(path)
        .output();
    if let Ok(out) = try_rsvg {
        if out.status.success() {
            return RasterImage::from_png_bytes(&out.stdout);
        }
    }
    let try_magick = std::process::Command::new("magick")
        .arg(path)
        .arg("png:-")
        .output();
    if let Ok(out) = try_magick {
        if out.status.success() {
            return RasterImage::from_png_bytes(&out.stdout);
        }
    }
    Err("SVG preview needs `rsvg-convert` or ImageMagick's `magick` on PATH".into())
}

fn image_title(path: &Path) -> String {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string());
    format!("Image preview — {name}")
}

/// A cell's size in pixels, as the host reports it.
fn cell_pixels() -> (u32, u32) {
    crossterm::terminal::window_size()
        .ok()
        .filter(|size| size.width > 0 && size.height > 0 && size.columns > 0 && size.rows > 0)
        .map_or(FALLBACK_CELL, |size| {
            (
                (size.width / size.columns).max(1) as u32,
                (size.height / size.rows).max(1) as u32,
            )
        })
}

/// The cells `raster` fills in `area`, its proportions kept for cells of
/// `cell` pixels: centred across, flush with the top.
fn fit(raster: &RasterImage, area: Rect, cell: (u32, u32)) -> Rect {
    let (cell_w, cell_h) = (cell.0 as f32, cell.1 as f32);
    let scale = (area.width as f32 * cell_w / raster.width as f32)
        .min(area.height as f32 * cell_h / raster.height as f32);
    let width = ((raster.width as f32 * scale / cell_w).round() as u16).clamp(1, area.width);
    let height = ((raster.height as f32 * scale / cell_h).round() as u16).clamp(1, area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y,
        width,
        height,
    }
}

fn resized(
    raster: &RasterImage,
    width: u32,
    height: u32,
    filter: image::imageops::FilterType,
) -> Option<RgbaImage> {
    if raster.width == 0 || raster.height == 0 {
        return None;
    }
    let src = raster.pixels.iter().flatten().copied().collect();
    let buf = ImageBuffer::<Rgba<u8>, Vec<u8>>::from_raw(raster.width, raster.height, src)?;
    Some(image::imageops::resize(&buf, width, height, filter))
}

fn raster_to_lines(
    raster: &RasterImage,
    placed: Rect,
    left_pad: u16,
    th: Theme,
) -> Vec<Line<'static>> {
    let (target_w, target_h) = (placed.width as u32, placed.height as u32 * 2);
    let filter = image::imageops::FilterType::Triangle;
    let Some(pixels) = resized(raster, target_w, target_h, filter) else {
        return vec![Line::from("couldn't prepare image pixels")];
    };
    (0..placed.height as u32)
        .map(|row| {
            let mut spans = Vec::with_capacity(target_w as usize + 1);
            if left_pad > 0 {
                spans.push(Span::raw(" ".repeat(left_pad as usize)));
            }
            for x in 0..target_w {
                let top = pixels.get_pixel(x, row * 2).0;
                let bottom = pixels.get_pixel(x, row * 2 + 1).0;
                spans.push(pixel_pair_span(top, bottom, th.canvas));
            }
            Line::from(spans)
        })
        .collect()
}

/// The image for `placed`, pixel for pixel at the host's cell size so the
/// terminal has no scaling of its own to do, flattened on the canvas so no
/// half-block shows through. None on a host with no protocol.
fn encode_image(
    raster: &RasterImage,
    placed: Rect,
    cell: (u32, u32),
    mode: &GraphicsMode,
    canvas: Color,
) -> Option<ImagePlacement> {
    let sequence = match mode {
        GraphicsMode::Iterm => iterm_sequence,
        GraphicsMode::Kitty => kitty_sequence,
        GraphicsMode::Off | GraphicsMode::Halfblocks => return None,
    };
    let (width, height) = (placed.width as u32 * cell.0, placed.height as u32 * cell.1);
    let filter = image::imageops::FilterType::CatmullRom;
    let pixels = resized(raster, width, height, filter)?;
    let opaque: Vec<u8> = pixels
        .pixels()
        .flat_map(|p| over_canvas(p.0, canvas))
        .collect();
    let mut png = Vec::new();
    PngEncoder::new_with_quality(
        &mut png,
        CompressionType::Fast,
        image::codecs::png::FilterType::Adaptive,
    )
    .write_image(&opaque, width, height, ExtendedColorType::Rgb8)
    .ok()?;
    Some(ImagePlacement {
        rect: placed,
        mode: mode.clone(),
        sequence: Arc::new(sequence(&png, placed)),
    })
}

fn iterm_sequence(png: &[u8], rect: Rect) -> Vec<u8> {
    format!(
        "\x1b]1337;File=inline=1;size={};width={};height={};preserveAspectRatio=0:{}\x07",
        png.len(),
        rect.width,
        rect.height,
        base64::engine::general_purpose::STANDARD.encode(png),
    )
    .into_bytes()
}

fn kitty_sequence(png: &[u8], rect: Rect) -> Vec<u8> {
    let data = base64::engine::general_purpose::STANDARD.encode(png);
    let chunks = data.as_bytes().chunks(KITTY_CHUNK_BYTES);
    let last = chunks.len().saturating_sub(1);
    let mut out = Vec::with_capacity(data.len() + data.len() / 64);
    for (i, chunk) in chunks.enumerate() {
        let more = u8::from(i < last);
        if i == 0 {
            let (cols, rows) = (rect.width, rect.height);
            let keys = format!("a=T,f=100,t=d,i={KITTY_IMAGE_ID},c={cols},r={rows},C=1");
            out.extend_from_slice(format!("\x1b_G{keys},q=2,m={more};").as_bytes());
        } else {
            out.extend_from_slice(format!("\x1b_Gq=2,m={more};").as_bytes());
        }
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn pixel_pair_span(top: [u8; 4], bottom: [u8; 4], canvas: Color) -> Span<'static> {
    let top = rgba_color(top, canvas);
    let bottom = rgba_color(bottom, canvas);
    match (top, bottom) {
        (None, None) => Span::raw(EMPTY_PIXELS),
        (Some(fg), Some(bg)) => Span::styled("▀", Style::default().fg(fg).bg(bg)),
        (Some(fg), None) => Span::styled("▀", Style::default().fg(fg)),
        (None, Some(fg)) => Span::styled("▄", Style::default().fg(fg)),
    }
}

/// A pixel over the theme's `canvas`: its alpha blends it into the
/// background it is drawn on, so an antialiased edge fades to white on the
/// light preset instead of to black.
fn rgba_color(px: [u8; 4], canvas: Color) -> Option<Color> {
    if px[3] == 0 {
        return None;
    }
    let [r, g, b] = over_canvas(px, canvas);
    Some(Color::Rgb(r, g, b))
}

fn over_canvas([r, g, b, a]: [u8; 4], canvas: Color) -> [u8; 3] {
    let (br, bg, bb) = match canvas {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    let a = a as u16;
    let blend =
        |fg: u8, bg: u8| -> u8 { (((fg as u16 * a) + (bg as u16 * (255 - a))) / 255) as u8 };
    [blend(r, br), blend(g, bg), blend(b, bb)]
}

#[derive(Debug, Clone)]
enum MermaidOutput {
    Image(RasterImage),
    Hint(String),
}

fn mermaid_cache() -> &'static Mutex<HashMap<u64, MermaidOutput>> {
    static CACHE: OnceLock<Mutex<HashMap<u64, MermaidOutput>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn render_mermaid(source: &str) -> MermaidOutput {
    let key = content_hash(source);
    if let Some(cached) = mermaid_cache().lock().unwrap().get(&key).cloned() {
        return cached;
    }
    let rendered = render_mermaid_uncached(source);
    mermaid_cache()
        .lock()
        .unwrap()
        .insert(key, rendered.clone());
    rendered
}

fn render_mermaid_uncached(source: &str) -> MermaidOutput {
    let paths = MermaidTempPaths::new();
    if let Err(err) = std::fs::write(&paths.input, source) {
        return MermaidOutput::Hint(format!("couldn't prepare Mermaid render: {err}"));
    }
    let output = std::process::Command::new("mmdc")
        .arg("-i")
        .arg(&paths.input)
        .arg("-o")
        .arg(&paths.output)
        .arg("-b")
        .arg("transparent")
        .output();
    let output = match output {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            paths.cleanup();
            return MermaidOutput::Hint(
                "install Mermaid CLI (`npm install -g @mermaid-js/mermaid-cli`) to render diagrams"
                    .into(),
            );
        }
        Err(err) => {
            paths.cleanup();
            return MermaidOutput::Hint(format!("couldn't run Mermaid CLI: {err}"));
        }
    };
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let one_line = err
            .lines()
            .rev()
            .find(|line| !line.trim().is_empty())
            .unwrap_or("Mermaid CLI failed");
        paths.cleanup();
        return MermaidOutput::Hint(format!("Mermaid CLI failed: {one_line}"));
    }
    let bytes = match std::fs::read(&paths.output) {
        Ok(bytes) => bytes,
        Err(err) => {
            paths.cleanup();
            return MermaidOutput::Hint(format!("couldn't read Mermaid output: {err}"));
        }
    };
    paths.cleanup();
    RasterImage::from_png_bytes(&bytes).map_or_else(MermaidOutput::Hint, MermaidOutput::Image)
}

#[derive(Debug)]
struct MermaidTempPaths {
    input: PathBuf,
    output: PathBuf,
}

impl MermaidTempPaths {
    fn new() -> Self {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let stem = format!("nebula-mermaid-{}-{millis}", std::process::id());
        let dir = std::env::temp_dir();
        Self {
            input: dir.join(format!("{stem}.mmd")),
            output: dir.join(format!("{stem}.png")),
        }
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_file(&self.input);
        let _ = std::fs::remove_file(&self.output);
    }
}

fn content_hash(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_and_mermaid_paths_are_detected_by_extension() {
        assert!(is_image_path(Path::new("mock.PNG")));
        assert!(is_image_path(Path::new("photo.jpeg")));
        assert!(is_image_path(Path::new("anim.gif")));
        assert!(is_image_path(Path::new("vector.svg")));
        assert!(is_mermaid_path(Path::new("diagram.MMD")));
        assert!(!is_image_path(Path::new("README.md")));
        assert!(!is_mermaid_path(Path::new("README.md")));
    }

    #[test]
    fn mermaid_blocks_are_extracted_from_markdown_fences() {
        let text = "before\n```mermaid\nflowchart LR\n  A-->B\n```\n\
                    ```rust\nfn main() {}\n```\n~~~ Mermaid\nsequenceDiagram\nA->>B: hi\n~~~\n";
        assert_eq!(
            extract_mermaid_blocks(text),
            vec![
                "flowchart LR\n  A-->B".to_string(),
                "sequenceDiagram\nA->>B: hi".to_string()
            ]
        );
    }

    #[test]
    fn a_mode_is_named_outright_and_sixel_stays_on_the_half_blocks() {
        assert_eq!(graphics_mode("off"), GraphicsMode::Off);
        assert_eq!(graphics_mode("iterm"), GraphicsMode::Iterm);
        assert_eq!(graphics_mode("kitty"), GraphicsMode::Kitty);
        assert_eq!(graphics_mode("sixel"), GraphicsMode::Halfblocks);
        assert_eq!(graphics_mode("halfblocks"), GraphicsMode::Halfblocks);
        assert_eq!(mode_label("bogus"), DEFAULT_INLINE_GRAPHICS);
    }

    #[test]
    fn auto_takes_the_protocol_the_host_speaks_out_of_the_box() {
        let detected = |program, term| detected_mode(program, term);
        assert_eq!(detected("WezTerm", "xterm-256color"), GraphicsMode::Iterm);
        assert_eq!(detected("iTerm.app", "xterm-256color"), GraphicsMode::Iterm);
        assert_eq!(detected("ghostty", "xterm-ghostty"), GraphicsMode::Kitty);
        assert_eq!(detected("", "xterm-kitty"), GraphicsMode::Kitty);
        assert_eq!(detected("tmux", "tmux-256color"), GraphicsMode::Halfblocks);
        assert_eq!(
            detected("Apple_Terminal", "xterm-256color"),
            GraphicsMode::Halfblocks
        );
        assert_eq!(detected("", ""), GraphicsMode::Halfblocks);
    }

    fn raster(width: u32, height: u32, pixel: [u8; 4]) -> RasterImage {
        RasterImage {
            width,
            height,
            pixels: Arc::new(vec![pixel; (width * height) as usize]),
        }
    }

    fn placement(mode: GraphicsMode, rect: Rect) -> ImagePlacement {
        let th = Theme::default();
        encode_image(
            &raster(4, 4, [255, 0, 0, 128]),
            rect,
            (2, 4),
            &mode,
            th.canvas,
        )
        .unwrap()
    }

    #[test]
    fn an_image_keeps_its_proportions_on_the_cells_the_host_really_has() {
        let area = Rect::new(10, 5, 120, 50);
        // A square on 1:2 cells is twice as many columns as rows, centred.
        let square = fit(&raster(100, 100, [0; 4]), area, (10, 20));
        assert_eq!(square, Rect::new(20, 5, 100, 50));
        // On 1:4 cells it takes four columns a row, and width runs out first.
        let narrow = fit(&raster(100, 100, [0; 4]), area, (5, 20));
        assert_eq!(narrow, Rect::new(10, 5, 120, 30));
        let wide = fit(&raster(1000, 10, [0; 4]), area, (10, 20));
        assert_eq!((wide.width, wide.height), (120, 1));
    }

    #[test]
    fn each_protocol_draws_the_rect_it_was_placed_on_as_an_opaque_png() {
        let rect = Rect::new(3, 4, 2, 1);
        let image = placement(GraphicsMode::Iterm, rect);
        let text = String::from_utf8(image.sequence.to_vec()).unwrap();
        let (head, data) = text.split_once(':').unwrap();
        assert!(head.contains("inline=1"), "{head}");
        assert!(
            head.contains(";width=2;height=1;"),
            "cells, not pixels: {head}"
        );
        let data = data.strip_suffix('\x07').unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (4, 4), "the host's own pixels");
        assert_eq!(decoded.get_pixel(1, 1).0[3], 255, "flattened on the canvas");
    }

    #[test]
    fn a_kitty_image_is_chunked_and_only_the_last_chunk_says_so() {
        let sequence = kitty_sequence(&vec![7; 10_000], Rect::new(0, 0, 5, 6));
        let text = String::from_utf8(sequence).unwrap();
        let parts: Vec<&str> = text.split_terminator("\x1b\\").collect();
        assert!(parts.len() > 1, "over the cap: {}", parts.len());
        assert!(parts[0].contains("a=T,f=100"), "{}", &parts[0][..40]);
        assert!(parts[0].contains(",c=5,r=6,"), "{}", &parts[0][..40]);
        for (i, part) in parts.iter().enumerate() {
            let (keys, data) = part.split_once(';').unwrap();
            assert!(data.len() <= KITTY_CHUNK_BYTES);
            assert!(keys.contains("q=2"), "every chunk stays quiet: {keys}");
            assert_eq!(keys.ends_with("m=0"), i == parts.len() - 1, "{keys}");
        }
    }

    #[test]
    fn an_image_goes_out_once_and_again_only_when_it_changes() {
        let rect = Rect::new(3, 4, 2, 1);
        let image = Some(placement(GraphicsMode::Iterm, rect));
        let mut shown = None;
        let mut out = Vec::new();
        sync_image(&image, &mut shown, &mut out).unwrap();
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.starts_with("\x1b7\x1b[5;4H"),
            "one-based, at the corner"
        );
        assert!(text.ends_with("\x1b8"), "the frame's cursor comes back");

        out.clear();
        sync_image(&image, &mut shown, &mut out).unwrap();
        assert!(out.is_empty(), "the frame after writes nothing");

        // Covered, then uncovered: the half-blocks the diff rewrote in the
        // meantime are under a fresh copy of the same image.
        sync_image(&None, &mut shown, &mut out).unwrap();
        assert!(out.is_empty(), "the cells took it down, the diff did that");
        sync_image(&image, &mut shown, &mut out).unwrap();
        assert!(!out.is_empty());
    }

    #[test]
    fn a_kitty_image_is_deleted_outright_since_cells_cant_erase_it() {
        let image = Some(placement(GraphicsMode::Kitty, Rect::new(0, 0, 2, 1)));
        let mut shown = None;
        let mut out = Vec::new();
        sync_image(&image, &mut shown, &mut out).unwrap();
        out.clear();
        sync_image(&None, &mut shown, &mut out).unwrap();
        assert_eq!(
            out,
            format!("\x1b_Ga=d,d=I,i={KITTY_IMAGE_ID},q=2\x1b\\").as_bytes()
        );
        assert!(shown.is_none());
    }

    /// The half-blocks of a clear image and the check for what's over them
    /// have to agree, or an image with a transparent corner never goes up.
    #[test]
    fn a_rendered_image_reads_as_uncovered_until_something_lands_on_it() {
        use ratatui::widgets::{Paragraph, Widget};
        let th = Theme::default();
        let area = Rect::new(2, 1, 8, 4);
        let clear = raster(8, 8, [0, 0, 0, 0]);
        let rect = fit(&clear, area, (2, 4));
        let lines = raster_to_lines(&clear, rect, rect.x - area.x, th);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 6));
        Paragraph::new(lines).render(area, &mut buffer);
        let image = ImagePlacement {
            rect,
            mode: GraphicsMode::Iterm,
            sequence: Arc::new(Vec::new()),
        };
        assert!(image.is_uncovered(&buffer));
        buffer[(rect.x + 1, rect.y + 1)].set_symbol(" ");
        assert!(!image.is_uncovered(&buffer));
    }

    #[test]
    fn the_half_blocks_show_at_once_and_only_the_last_renders_image_lands() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        runtime.block_on(async {
            let (tx, mut answers) = tokio::sync::mpsc::unbounded_channel();
            let jobs = Jobs::new(tx);
            let th = Theme::default();
            let mode = || GraphicsMode::Iterm;
            let mut visual = VisualPreview {
                title: String::new(),
                raster: Some(raster(4, 4, [255, 0, 0, 255])),
                message: None,
                source: None,
                rendered: None,
            };
            let lines = visual.render(Rect::new(0, 0, 8, 4), mode(), th, Some(&jobs));
            assert!(!lines.is_empty(), "the frame never waits on the encode");
            assert!(visual.placement().is_none());
            let Some(Answer::Image {
                ticket: stale,
                image,
            }) = answers.recv().await
            else {
                panic!("no answer");
            };

            visual.render(Rect::new(0, 0, 16, 8), mode(), th, Some(&jobs));
            visual.land_image(stale, image);
            assert!(visual.placement().is_none(), "that was the last area's");

            let Some(Answer::Image { ticket, image }) = answers.recv().await else {
                panic!("no answer");
            };
            visual.land_image(ticket, image);
            assert!(visual.placement().is_some());
        });
    }

    #[test]
    fn a_diagram_on_a_page_stays_on_the_half_blocks_it_can_scroll_with() {
        let th = Theme::default();
        let mut diagrams = [MermaidDiagram {
            source: "graph".into(),
            visual: VisualPreview {
                title: String::new(),
                raster: Some(raster(4, 4, [255, 0, 0, 255])),
                message: None,
                source: None,
                rendered: None,
            },
        }];
        markdown_diagram_lines("graph", &mut diagrams, 8, GraphicsMode::Iterm, th).unwrap();
        assert!(diagrams[0].visual.placement().is_none());
    }

    #[test]
    fn a_translucent_pixel_blends_into_the_theme_canvas() {
        let half_red = [255, 0, 0, 128];
        assert_eq!(
            rgba_color(half_red, crate::theme::BLACK_BACKGROUND),
            Some(Color::Rgb(128, 0, 0))
        );
        assert_eq!(
            rgba_color(half_red, crate::theme::WHITE_BACKGROUND),
            Some(Color::Rgb(255, 127, 127))
        );
        assert_eq!(
            rgba_color([255, 0, 0, 0], crate::theme::WHITE_BACKGROUND),
            None
        );
    }
}
