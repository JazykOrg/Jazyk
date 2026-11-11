use super::RenderError;
#[cfg(not(target_arch = "wasm32"))]
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::{Arc, OnceLock};

// ---- the seam ----

// PlantUML text to SVG. In process through plantuml-little; the official native binary
// named by JAZYK_PLANTUML is the authorized swap. Mirrors docs/compiler/diagrams.md#the-renderer.
//
// The conversion links Graphviz as a C archive, so it is the one part of rendering
// that stays native. In the browser backend the emitters still run and every view
// still emits its `.puml`; the `.svg` beside it is whatever the project was seeded
// with. See docs/frontends/wasm.md#diagrams.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_svg(puml: &str) -> Result<String, RenderError> {
    match crate::sys::env_var("JAZYK_PLANTUML") {
        Ok(bin) if !bin.trim().is_empty() => native_svg(bin.trim(), puml),
        _ => in_process_svg(puml),
    }
}

// A panic inside the crate is a renderer defect like any other error: it names itself
// and never takes the build down. The crate reads a `[[...]]` on an element declaration
// as part of the name (a class takes its alias as its label, an actor splits in two)
// and never emits an anchor, so the links come off before conversion and go back on
// as anchors after; the official binary behind JAZYK_PLANTUML does both itself.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn in_process_svg(puml: &str) -> Result<String, RenderError> {
    let (plain, links) = strip_element_links(puml);
    let plain = adapt_component_for_crate(&plain);
    match std::panic::catch_unwind(|| {
        if crate::vfs::is_scoped() {
            hosted_svg(&plain)
        } else {
            pure_svg(&plain)
        }
    }) {
        Ok(Ok(svg)) => Ok(anchor_entities(&svg, &links)),
        Ok(Err(e)) => Err(RenderError(e.to_string())),
        Err(panic) => {
            let message = panic
                .downcast_ref::<String>()
                .cloned()
                .or_else(|| panic.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_else(|| "no message".to_string());
            Err(RenderError(format!("renderer panicked: {}", message)))
        }
    }
}

// The dependency's convenience converter runs a filesystem preprocessor. Every
// in-process conversion uses its public pure parser/layout/render stages instead: the
// emitters write no preprocessor syntax, and graph text is inert (see `inert`). Hosted
// diagrams also refuse inline image markup and directive lines before any layout can
// invoke the dependency's image loader.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn hosted_svg(source: &str) -> plantuml_little::Result<String> {
    if source.to_ascii_lowercase().contains("<img")
        || source
            .lines()
            .any(|line| line.trim_start().starts_with('!'))
    {
        return Err(plantuml_little::Error::Render(
            "hosted diagrams do not load external images or preprocessor directives".into(),
        ));
    }
    pure_svg(source)
}

#[cfg(not(target_arch = "wasm32"))]
fn pure_svg(source: &str) -> plantuml_little::Result<String> {
    struct Sprites;
    impl Drop for Sprites {
        fn drop(&mut self) {
            plantuml_little::render::svg_richtext::clear_sprites();
        }
    }
    plantuml_little::render::svg_richtext::clear_sprites();
    let _sprites = Sprites;
    let diagram = plantuml_little::parser::parse(source)?;
    let skin = plantuml_little::style::parse_skinparams(source);
    let layout = plantuml_little::layout::layout(&diagram, &skin)?;
    let meta = plantuml_little::parser::common::parse_meta(source);
    plantuml_little::render::svg::render(&diagram, &layout, &skin, &meta)
}

#[cfg(target_arch = "wasm32")]
pub fn render_svg(_puml: &str) -> Result<String, RenderError> {
    Err(RenderError(
        "the browser backend does not convert PlantUML: Graphviz is a C archive".to_string(),
    ))
}

#[cfg(target_arch = "wasm32")]
pub fn render_png(_svg: &str) -> Result<Vec<u8>, RenderError> {
    Err(RenderError(
        "the browser backend does not rasterize: resvg needs the native fonts".to_string(),
    ))
}

// The native binary runs under the command timeout (limits::command_timeout), the
// way every external command does: stdin written and both streams drained on their own
// threads, so a child that never reads or floods its output cannot stall the build, and
// a child past the deadline is killed and reported as a renderer error.
// Mirrors docs/compiler/diagrams.md#the-renderer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn native_svg(bin: &str, puml: &str) -> Result<String, RenderError> {
    native_svg_within(bin, puml, crate::limits::command_timeout())
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn native_svg_within(
    bin: &str,
    puml: &str,
    timeout: std::time::Duration,
) -> Result<String, RenderError> {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    let fail = |e: &dyn fmt::Display| RenderError(format!("{}: {}", bin, e));
    let mut child = Command::new(bin)
        .args(["-tsvg", "-pipe"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail(&e))?;
    // A child that dies without reading (or before the write finishes) breaks the
    // pipe; its exit status below is the story, so the write tolerates that one.
    let input = child.stdin.take().map(|mut stdin| {
        let bytes = puml.as_bytes().to_vec();
        std::thread::spawn(move || match stdin.write_all(&bytes) {
            Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => Err(e),
            _ => Ok(()),
        })
    });
    fn collect<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::channel();
        if let Some(mut pipe) = pipe {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        rx
    }
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Some(st) = child.try_wait().map_err(|e| fail(&e))? {
            break st;
        }
        if started.elapsed() >= timeout {
            child.kill().ok();
            child.wait().ok();
            return Err(RenderError(format!(
                "{} killed after {}s (JAZYK_COMMAND_TIMEOUT)",
                bin,
                timeout.as_secs()
            )));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    // A grandchild holding a pipe open past the exit costs its output, never the build.
    let take =
        |rx: mpsc::Receiver<Vec<u8>>| rx.recv_timeout(Duration::from_secs(2)).unwrap_or_default();
    let out = take(stdout);
    let err = take(stderr);
    if let Some(Ok(Err(e))) = input.map(|h| h.join()) {
        return Err(fail(&e));
    }
    if !status.success() {
        return Err(RenderError(format!(
            "{} exited with {}: {}",
            bin,
            status,
            String::from_utf8_lossy(&err).trim()
        )));
    }
    String::from_utf8(out).map_err(|e| fail(&e))
}

// The system fonts, loaded once per process: text measures as a viewer would draw it.
#[cfg(not(target_arch = "wasm32"))]
fn fonts() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

// The longer side of a rasterized view, in pixels.
#[cfg(not(target_arch = "wasm32"))]
const PNG_MAX_SIDE: f32 = 8192.0;

// SVG to PNG bytes through resvg.
#[cfg(not(target_arch = "wasm32"))]
pub fn render_png(svg: &str) -> Result<Vec<u8>, RenderError> {
    if crate::vfs::is_scoped() {
        return Err(RenderError(
            "hosted PNG export is unavailable; use SVG".into(),
        ));
    }
    let opt = resvg::usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &opt).map_err(|e| RenderError(e.to_string()))?;
    // At most PNG_MAX_SIDE pixels on the longer side: a wide view scales down instead
    // of asking for a gigabyte of pixels. Mirrors docs/compiler/diagrams.md#rendering.
    let size = tree.size();
    let longer = size.width().max(size.height());
    let scale = if longer > PNG_MAX_SIDE {
        PNG_MAX_SIDE / longer
    } else {
        1.0
    };
    let width = (size.width() * scale).ceil().max(1.0) as u32;
    let height = (size.height() * scale).ceil().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)
        .ok_or_else(|| RenderError("the svg has no size".to_string()))?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap.encode_png().map_err(|e| RenderError(e.to_string()))
}

// The links on element declarations, taken off the text: `(line without the link,
// [(element key, href)])`. A link is a ` [[../...]]` on a declaration line; the key is
// the alias after ` as `, or the quoted name of a package declared without one, the
// same name the crate writes as the element's `data-qualified-name`.
// The crate draws no actor in a component diagram and, with no interface declared, no
// link but `-->` and `..>`. The emitted `.puml` stays standard for the official binary;
// only the in-process conversion adapts it: an actor converts as a component with the
// «actor» stereotype, and without an interface each link converts to `-->` (`..>` for the
// dotted forms) in its stated direction, its label kept.
// Mirrors docs/compiler/diagrams.md#the-renderer.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn adapt_component_for_crate(puml: &str) -> String {
    let lines: Vec<&str> = puml.lines().collect();
    let is_component = lines
        .iter()
        .any(|l| l.trim_start().starts_with("component "));
    if !is_component {
        return puml.to_string();
    }
    let has_interface = lines
        .iter()
        .any(|l| l.trim_start().starts_with("interface "));
    let mut out = String::with_capacity(puml.len());
    for line in lines {
        let t = line.trim_start();
        let adapted = if let Some(rest) = t.strip_prefix("actor ") {
            // `actor "Name" as alias ...` or `actor Name`.
            if rest.starts_with('"') {
                format!("component {} <<actor>>", rest.trim_end())
            } else {
                let name = rest.split_whitespace().next().unwrap_or(rest);
                format!("component \"{}\" as {} <<actor>>", name, name)
            }
        } else if !has_interface {
            let parts: Vec<&str> = t.splitn(3, ' ').collect();
            match parts.as_slice() {
                [a, op, rest] if ["--", "*--", "o--", "--|>"].contains(op) => {
                    format!("{} --> {}", a, rest)
                }
                [a, op, rest] if ["..", "..|>"].contains(op) => format!("{} ..> {}", a, rest),
                _ => line.to_string(),
            }
        } else {
            line.to_string()
        };
        out.push_str(&adapted);
        out.push('\n');
    }
    out
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn strip_element_links(puml: &str) -> (String, Vec<(String, String)>) {
    let mut plain = String::with_capacity(puml.len());
    let mut links: Vec<(String, String)> = Vec::new();
    for line in puml.lines() {
        let stripped = line.find(" [[../").and_then(|start| {
            let after = &line[start + 3..];
            let end = after.find("]]")?;
            let href = &after[..end];
            let head = &line[..start];
            let tail = &after[end + 2..];
            let key = match head.rsplit_once(" as ") {
                Some((_, alias)) => alias.split_whitespace().next()?.to_string(),
                None => head
                    .split_once(' ')
                    .map(|(_, name)| name.trim().trim_matches('"').to_string())?,
            };
            Some((format!("{}{}", head, tail), key, href.to_string()))
        });
        match stripped {
            Some((clean, key, href)) => {
                plain.push_str(&clean);
                links.push((key, href));
            }
            None => plain.push_str(line),
        }
        plain.push('\n');
    }
    (plain, links)
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn xml_attr(s: &str) -> String {
    #[cfg(test)]
    super::tests::XML_ATTR_CALLS.with(|c| c.set(c.get() + 1));
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// The value of an attribute in one opening tag.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn attr_value(tag: &str, name: &str) -> Option<String> {
    let marker = format!(" {}=\"", name);
    let rest = &tag[tag.find(&marker)? + marker.len()..];
    Some(rest[..rest.find('"')?].to_string())
}

// Every group the crate drew for a linked element (`class="entity"` on a structural
// or use case element, `participant participant-head` and `-tail` on a sequence
// participant's two boxes), keyed by its `data-qualified-name`, wrapped in `<a href>`:
// the whole shape is the click target, as the official renderer draws it. An element
// the crate did not draw under that key keeps no link; nothing else in the document
// changes. The first link under a key wins. One scan collects the insertion points
// and one pass writes the output, so a view of thousands of elements stays linear.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn anchor_entities(svg: &str, links: &[(String, String)]) -> String {
    if links.is_empty() {
        return svg.to_string();
    }
    let mut hrefs: std::collections::HashMap<String, &str> =
        std::collections::HashMap::with_capacity(links.len());
    for (key, href) in links {
        hrefs.entry(xml_attr(key)).or_insert(href.as_str());
    }
    // Opening anchors in document order; closing positions sorted below.
    let mut opens: Vec<(usize, String)> = Vec::new();
    let mut closes: Vec<usize> = Vec::new();
    let mut from = 0;
    while let Some(rel) = svg[from..].find("<g ") {
        let start = from + rel;
        from = start + "<g ".len();
        let Some(tag_len) = svg[start..].find('>') else {
            break;
        };
        let tag = &svg[start..=start + tag_len];
        let linked = match attr_value(tag, "class").as_deref() {
            Some("entity") => true,
            Some(c) => c.starts_with("participant participant-"),
            None => false,
        };
        if !linked {
            continue;
        }
        let Some(key) = attr_value(tag, "data-qualified-name") else {
            continue;
        };
        let Some(href) = hrefs.get(key.as_str()) else {
            continue;
        };
        let Some(end) = group_end(svg, start) else {
            break;
        };
        opens.push((
            start,
            format!("<a href=\"{0}\" xlink:href=\"{0}\">", xml_attr(href)),
        ));
        closes.push(end);
    }
    closes.sort_unstable();
    let extra: usize = opens.iter().map(|(_, a)| a.len()).sum::<usize>() + 4 * closes.len();
    let mut out = String::with_capacity(svg.len() + extra);
    let (mut o, mut c, mut at) = (0, 0, 0);
    // A group closing where the next opens: the `</a>` comes first.
    while o < opens.len() || c < closes.len() {
        let close_next = c < closes.len() && (o == opens.len() || closes[c] <= opens[o].0);
        let pos = if close_next { closes[c] } else { opens[o].0 };
        out.push_str(&svg[at..pos]);
        at = pos;
        if close_next {
            out.push_str("</a>");
            c += 1;
        } else {
            out.push_str(&opens[o].1);
            o += 1;
        }
    }
    out.push_str(&svg[at..]);
    out
}

// The index just past the `</g>` closing the group opened at `start`, nesting counted.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn group_end(svg: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < svg.len() {
        let rest = &svg[i..];
        if rest.starts_with("<g ") || rest.starts_with("<g>") {
            depth += 1;
            i += 2;
        } else if rest.starts_with("</g>") {
            depth -= 1;
            i += 4;
            if depth == 0 {
                return Some(i);
            }
        } else {
            i += rest.chars().next()?.len_utf8();
        }
    }
    None
}
