//! Assistant-only CommonMark adapter. Source messages and history remain untouched.
use std::ops::Range;
use std::sync::Arc;

use egui_commonmark::{CommonMarkCache, CommonMarkViewer};
use pulldown_cmark::{Event, Options, Parser, Tag};

fn options() -> Options {
    // Keep parsing identical to egui_commonmark_backend's parser_options.
    Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_FOOTNOTES | Options::ENABLE_DEFINITION_LIST
}

fn web_link(url: &str) -> bool {
    url.split_once("://").is_some_and(|(scheme, address)| {
        (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
            && !address.is_empty()
            && !url.chars().any(|ch| ch.is_control() || ch.is_whitespace())
    })
}

fn escape(text: &str) -> String {
    let mut result = String::new();
    for ch in text.chars() {
        if ch.is_ascii_punctuation() {
            result.push('\\');
        }
        result.push(ch);
    }
    result
}

/// Use CommonMark's own spans, including reference links and escaped markup.
/// Disabling crate image features alone is insufficient: the host may already
/// have image loaders installed, so remove image nodes before invoking the viewer.
fn display_source(text: &str) -> String {
    let mut edits = Vec::new();
    let mut capture: Option<(Range<usize>, String, usize, bool, Option<String>)> = None;
    for (event, span) in Parser::new_ext(text, options()).into_offset_iter() {
        if let Some((range, label, depth, image, destination)) = capture.as_mut() {
            match event {
                Event::Start(_) => *depth += 1,
                Event::End(_) => *depth = depth.saturating_sub(1),
                Event::Text(value) | Event::Code(value) => label.push_str(&value),
                Event::SoftBreak | Event::HardBreak => label.push(' '),
                _ => {}
            }
            if *depth == 0 {
                range.end = range.end.max(span.end);
                let visible = if *image { format!("[{}: {label}]", super::tr("Image")) } else { label.clone() };
                let visible = escape(&visible);
                // Canonical inline links keep reference links working when an
                // outer block is rendered independently from its definitions.
                let replacement = if let Some(url) = destination {
                    let url = url.replace('&', "&amp;").replace('\\', "%5C").replace('<', "%3C").replace('>', "%3E");
                    format!("[{visible}](<{url}>)")
                } else {
                    visible
                };
                edits.push((range.clone(), replacement));
                capture = None;
            }
        } else {
            let image = matches!(event, Event::Start(Tag::Image { .. }));
            let link = match &event {
                Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url),
                _ => None,
            };
            if image || link.is_some() {
                let destination = link.filter(|url| web_link(url)).map(|url| url.to_string());
                capture = Some((span, String::new(), 1, image, destination));
            }
        }
    }
    let mut result = String::new();
    let mut start = 0;
    for (span, replacement) in edits {
        if let Some(prefix) = text.get(start..span.start) {
            result.push_str(prefix);
            result.push_str(&replacement);
            start = span.end;
        }
    }
    result.push_str(text.get(start..).unwrap_or_default());
    result
}

/// Split only at complete outer blocks, preserving list/quote nesting. Wide
/// blocks get their own horizontal viewport; ordinary paragraphs still wrap.
fn blocks(text: &str) -> Option<Vec<(Range<usize>, bool)>> {
    let mut result = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    let mut wide = false;
    for (event, span) in Parser::new_ext(text, options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    // Indented code spans omit the first line's indentation.
                    // Preserve it when the component parses this block again.
                    let prefix = text.get(..span.start).unwrap_or_default();
                    let line_start = prefix.rfind('\n').map_or(0, |offset| offset + 1);
                    start = if prefix.get(line_start..).is_some_and(|indent| indent.chars().all(|ch| ch == ' ' || ch == '\t')) {
                        line_start
                    } else {
                        span.start
                    };
                    wide = false;
                }
                wide |= matches!(tag, Tag::CodeBlock(_) | Tag::Table(_));
                depth += 1;
                if depth > 32 {
                    return None;
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    result.push((start..span.end, wide));
                }
            }
            Event::Rule if depth == 0 => result.push((span, false)),
            _ => {}
        }
    }
    Some(result)
}

struct Block {
    source: String,
    wide: bool,
    code_columns: usize,
}
struct Prepared(Option<Vec<Block>>);

#[derive(Default)]
struct Prepare;
impl egui::cache::ComputerMut<(&str, &str), Arc<Prepared>> for Prepare {
    fn compute(&mut self, (text, _language): (&str, &str)) -> Arc<Prepared> {
        let source = display_source(text);
        let blocks = blocks(&source).map(|blocks| {
            blocks
                .into_iter()
                .filter_map(|(span, wide)| {
                    let block = display_source(source.get(span)?);
                    let mut parser = Parser::new_ext(&block, options());
                    let code_columns = if matches!(parser.next(), Some(Event::Start(Tag::CodeBlock(_)))) {
                        parser
                            .filter_map(|event| match event {
                                Event::Text(value) => Some(value.lines().map(|line| line.chars().count()).max().unwrap_or(0)),
                                _ => None,
                            })
                            .max()
                            .unwrap_or(0)
                    } else {
                        0
                    };
                    Some(Block { source: block, wide, code_columns })
                })
                .collect()
        });
        Arc::new(Prepared(blocks))
    }
}

pub(super) fn show(ui: &mut egui::Ui, text: &str) {
    // Egui evicts unused entries: old pages and streaming revisions do not
    // accumulate in a second, unbounded conversation archive.
    let prepared = ui
        .ctx()
        .memory_mut(|memory| memory.caches.cache::<egui::cache::FrameCache<Arc<Prepared>, Prepare>>().get((text, super::tr("Image"))).clone());
    let Some(blocks) = &prepared.0 else {
        ui.add(egui::Label::new(text).wrap());
        return;
    };
    let mut cache = CommonMarkCache::default();
    let width = ui.available_width().max(1.0);
    ui.scope(|ui| {
        ui.style_mut().url_in_tooltip = true;
        for (index, block) in blocks.iter().enumerate() {
            ui.push_id(("assistant-markdown", index), |ui| {
                if block.wide {
                    egui::ScrollArea::horizontal().id_salt("wide-block").max_width(width).auto_shrink([false, true]).show(ui, |ui| {
                        // Code lines retain their width. Bound layout dimensions for
                        // maliciously long input without truncating the actual text.
                        let font = egui::TextStyle::Monospace.resolve(ui.style());
                        let preferred = if block.code_columns > 0 {
                            ui.fonts_mut(|fonts| fonts.glyph_width(&font, 'M')) * block.code_columns as f32 + 32.0
                        } else {
                            0.0
                        };
                        ui.set_width(width.max(preferred.min(16384.0)));
                        CommonMarkViewer::new().show(ui, &mut cache, &block.source);
                    });
                } else {
                    CommonMarkViewer::new().show(ui, &mut cache, &block.source);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests;
