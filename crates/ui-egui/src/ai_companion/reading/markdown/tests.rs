use super::*;
use crate::headless::HeadlessView;

fn render(text: &str, width: f32) -> HeadlessView {
    let mut view = HeadlessView::new();
    for frame in 0..4 {
        view.run(HeadlessView::raw_input(egui::vec2(width, 1200.0), 1.0, frame as f64 / 60.0, vec![]), |ui| show(ui, text));
    }
    view
}

#[test]
fn commonmark_formats_headings_inline_styles_lists_quotes_rules_code_and_tables() {
    let source = "### Exposure\n\nPreserve **detail** and *natural color*, use `small changes`.\n\n- Sky\n- Trees\n\n1. Inspect\n2. Adjust\n\n> Keep the mood.\n\n---\n\n```rust\nlet exposure = 0.2;\n```\n\n| Area | Advice |\n| --- | --- |\n| Sky | Preserve texture |\n\n[More information][guide]\n\n[guide]: https://example.com/guide\n";
    let view = render(source, 420.0);
    let painted = view.painted_text().join(" ");
    for expected in [
        "Exposure",
        "detail",
        "natural color",
        "small changes",
        "Sky",
        "Trees",
        "Inspect",
        "Adjust",
        "Keep the mood.",
        "let exposure",
        "Area",
        "Advice",
        "Preserve texture",
        "More information",
    ] {
        assert!(painted.contains(expected), "missing {expected}: {painted}");
    }
    for delimiter in ["###", "**detail**", "*natural color*", "```", "| --- |", "[More information]"] {
        assert!(!painted.contains(delimiter), "unrendered syntax {delimiter}: {painted}");
    }
}

#[test]
fn images_are_placeholders_and_only_web_links_reach_the_viewer() {
    let source = "![远程 **预览**](https://example.com/photo.jpg)\n\n![本机](file:///private.jpg)\n\n![内嵌](data:image/png;base64,AAAA)\n\n[网页](https://example.com) [文件](file:///private.txt) [脚本](javascript:alert%281%29) [相对](relative/path)\n\n[嵌套 ![图片](https://example.com/nested.jpg)](https://example.com)\n\n[引用][local]\n\n[local]: file:///private.txt\n";
    let source = display_source(source);
    let links: Vec<_> = Parser::new_ext(&source, options())
        .filter_map(|event| match event {
            Event::Start(Tag::Image { .. }) => panic!("image node reached the renderer"),
            Event::Start(Tag::Link { dest_url, .. }) => Some(dest_url.to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(links, vec!["https://example.com", "https://example.com"]);
    let painted = render(&source, 420.0).painted_text().join(" ");
    for expected in ["远程 预览", "本机", "内嵌", "文件", "脚本", "相对", "引用", "嵌套", "图片"] {
        assert!(painted.contains(expected), "lost label {expected}: {painted}");
    }
}

#[test]
fn incomplete_stream_and_excessive_nesting_remain_readable() {
    for source in ["### Partial\n\n**not finished", "```rust\nlet value = 42;", "| Area | Advice |\n| --- | --- |\n| Sky | still arriving"] {
        let painted = render(source, 300.0).painted_text().join(" ");
        assert!(!painted.trim().is_empty(), "stream disappeared: {source}");
    }
    let source = format!("{}Keep this text.", "> ".repeat(80));
    assert!(render(&source, 300.0).painted_text().join(" ").contains("Keep this text."));
}

#[test]
fn indented_code_keeps_image_and_link_syntax_literal() {
    let source = "    ![alt](https://example.com/p.png)\n    [local](file:///private.txt)\n\nAfter code.";
    let painted = render(source, 420.0).painted_text().join(" ");
    assert!(painted.contains("![alt](https://example.com/p.png)"), "code reinterpreted as an image: {painted}");
    assert!(painted.contains("[local](file:///private.txt)"), "code reinterpreted as a link: {painted}");
    assert!(painted.contains("After code."));
}

#[test]
fn reference_links_keep_their_exact_destinations_and_do_not_enter_streaming_code() {
    use egui::cache::ComputerMut;
    let source = "[Read][guide]\n\n[guide]: https://example.com/?q=&amp;amp;\n\n```text\nstill arriving";
    let prepared = Prepare.compute((source, "Image"));
    let mut links = Vec::new();
    for block in prepared.0.as_ref().unwrap() {
        for event in Parser::new_ext(&block.source, options()) {
            if let Event::Start(Tag::Link { dest_url, .. }) = event {
                links.push(dest_url.to_string());
            }
        }
    }
    assert_eq!(links, vec!["https://example.com/?q=&amp;"]);
    let painted = render(source, 420.0).painted_text().join(" ");
    assert!(painted.contains("Read") && painted.contains("still arriving"));
    assert!(!painted.contains("[guide]:"), "reference definition leaked into unfinished code: {painted}");
}

#[test]
fn only_a_user_click_opens_a_web_link() {
    let source = "[Read the guide](https://example.com/guide) [Local file](file:///private.txt)";
    let view = render(source, 420.0);
    let position = view
        .painted_shapes()
        .iter()
        .find_map(|shape| match shape {
            egui::Shape::Text(text) if text.galley.text() == "Read the guide" => Some(text.visual_bounding_rect().center()),
            _ => None,
        })
        .unwrap();
    let clicked = |position| {
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE },
            egui::Event::PointerButton { pos: position, button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE },
        ]
    };
    let idle = view.ctx.run_ui(HeadlessView::raw_input(egui::vec2(420.0, 1200.0), 1.0, 0.1, vec![]), |ui| show(ui, source));
    assert!(!idle.platform_output.commands.iter().any(|command| matches!(command, egui::OutputCommand::OpenUrl(_))));
    let output = view.ctx.run_ui(HeadlessView::raw_input(egui::vec2(420.0, 1200.0), 1.0, 0.2, clicked(position)), |ui| show(ui, source));
    assert!(
        output
            .platform_output
            .commands
            .iter()
            .any(|command| matches!(command, egui::OutputCommand::OpenUrl(url) if url.url == "https://example.com/guide"))
    );
}

#[test]
fn wide_code_and_tables_do_not_expand_the_message_viewport() {
    let source = format!(
        "Before the wide content.\n\n```text\n{}\n```\n\n| First column | Second column |\n| --- | --- |\n| {} | useful advice |\n\nAfter the wide content.",
        "very_long_code_line_".repeat(30),
        "wide_table_value_".repeat(20)
    );
    let mut view = HeadlessView::new();
    let mut rect = egui::Rect::NOTHING;
    for frame in 0..4 {
        view.run(HeadlessView::raw_input(egui::vec2(320.0, 1200.0), 1.0, frame as f64 / 60.0, vec![]), |ui| {
            rect = ui.scope(|ui| show(ui, &source)).response.rect;
        });
    }
    assert!(rect.width() <= 320.0, "wide Markdown expanded the host: {rect:?}");
    let painted = view.painted_text().join(" ");
    assert!(painted.contains("Before the wide content.") && painted.contains("After the wide content."));
}
