mod dumps;
mod snapshots;

use std::{fs::File, io::BufReader, sync::LazyLock};

use glam::Vec2;

use crate::{
    format::font::FontInfo,
    layout::{
        message_text_layouter::{
            commands::Command, LayoutParams, LineInfo, MessageLayerLayouter,
            MessageTextLayouterDefaults,
        },
        MessageTextParser,
    },
    vm::command::types::{MessageTextLayout, MessageboxType},
};

pub struct TestFonts {
    bold_font: FontInfo,
    normal_font: FontInfo,
}

pub fn read_fonts() -> TestFonts {
    fn read_font(path: &str) -> FontInfo {
        let font = File::open(path).unwrap();
        let mut font = BufReader::new(font);
        shin_core::format::font::read_font_metrics(&mut font).unwrap()
    }

    let normal = read_font("test_assets/newrodin-medium.fnt");
    let bold = read_font("test_assets/newrodin-bold.fnt");
    // let system = read_font("test_assets/system.fnt");

    TestFonts {
        bold_font: bold,
        normal_font: normal,
    }
}

pub fn make_snapshot(
    text_alignment: MessageTextLayout,
    messagebox_type: MessageboxType,
    text: &str,
) -> (Vec<Command>, Vec<LineInfo>, Vec2) {
    // share fonts between invocations in the same process
    static FONTS: LazyLock<TestFonts> = LazyLock::new(read_fonts);

    let normal = &FONTS.normal_font;
    let bold = &FONTS.bold_font;

    // layout params used by the MessageLayer
    let layout_params = LayoutParams {
        layout_width: 1500.0,
        text_alignment,
        line_padding_above: 0.0,
        line_padding_below: 0.0,
        line_padding_between: 4.0,
        rubi_size: 20.0,
        text_size: 50.0,
        base_font_horizontal_scale: 0.9697,
        follow_kinsoku_shori_rules: true,
        always_leave_space_for_rubi: true,
        perform_soft_breaks: true,
        word_wrap: false,
    };
    let defaults = MessageTextLayouterDefaults {
        color: 999,
        draw_speed: 80,
        fade: 200,
    };

    let mut layouter =
        MessageLayerLayouter::new(normal, bold, messagebox_type, layout_params, defaults);
    let parser = MessageTextParser::new(text);
    parser.parse_into(&mut layouter);

    layouter.finish()
}

/// Every glyph 10 units wide, with ascent + descent 10, so a char is as wide as the text size
struct MonoFont;

impl crate::layout::message_text_layouter::font::FontMetrics for MonoFont {
    fn get_ascent(&self) -> u32 {
        8
    }

    fn get_descent(&self) -> u32 {
        2
    }

    fn get_glyph_info(&self, _: char) -> Option<crate::format::font::GlyphInfo> {
        Some(crate::format::font::GlyphInfo {
            bearing_x: 0,
            bearing_y: 8,
            advance_width: 10,
            actual_width: 10,
            actual_height: 10,
            texture_width: 16,
            texture_height: 16,
        })
    }
}

fn lines(text: &str, word_wrap: bool) -> Vec<String> {
    let params = LayoutParams {
        layout_width: 120.0,
        text_size: 10.0,
        word_wrap,
        ..LayoutParams::default()
    };
    let defaults = MessageTextLayouterDefaults {
        color: 999,
        draw_speed: 100,
        fade: 0,
    };
    let (commands, _, _) =
        crate::layout::MessageTextLayouter::new(MonoFont, MonoFont, params, defaults).parse(text);
    let mut lines = Vec::<String>::new();
    for command in commands {
        if let Command::Char(c) = command {
            if lines.len() <= c.line_index {
                lines.resize(c.line_index + 1, String::new());
            }
            lines[c.line_index].push(c.codepoint);
        }
    }
    lines
}

#[test]
fn word_wrap_keeps_words_whole() {
    // 12 chars fit a line: the original's break falls inside "Valentine"
    let text = "Seven Sisters' Valentine";
    assert_eq!(lines(text, false), ["Seven Sister", "s' Valentine"]);
    assert_eq!(lines(text, true), ["Seven ", "Sisters' ", "Valentine"]);
    // a word longer than the line still breaks
    assert_eq!(
        lines("Ushiromiya-Battler", true),
        ["Ushiromiya-B", "attler"]
    );
}
