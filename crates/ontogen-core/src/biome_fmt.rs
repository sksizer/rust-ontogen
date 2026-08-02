//! In-process TypeScript formatting via the biome formatter (feature `biome`).
//!
//! Replaces the previous `npx prettier` shell-out. Fully hermetic — no external
//! process, no Node, no `.prettierrc` resolution — so it can never fail on a
//! missing or mismatched tool the way the prettier path did on CI runners.

use biome_formatter::{IndentStyle, IndentWidth, LineWidth, QuoteStyle};
use biome_js_formatter::context::trailing_commas::TrailingCommas;
use biome_js_formatter::context::{JsFormatOptions, Semicolons};
use biome_js_formatter::format_node;
use biome_js_parser::JsParserOptions;
use biome_languages::JsFileSource;

/// Format a TypeScript source string in-process with biome.
///
/// Returns `Err` with a human-readable reason if biome can't parse the input;
/// the caller falls back to unformatted output plus a `cargo:warning`.
pub fn format_ts(input: &str) -> Result<String, String> {
    let source = JsFileSource::ts();
    let parsed = biome_js_parser::parse(input, source, JsParserOptions::default());
    // Canonical, widespread-standard TypeScript style: 2-space indent, double
    // quotes, semicolons, trailing commas everywhere, 100-col width. (Biome's
    // own defaults, except indent — biome defaults to tabs, but 2-space is the
    // near-universal TS/JS convention and keeps generated code consistent with
    // hand-written code.)
    let options = JsFormatOptions::new(source)
        .with_indent_style(IndentStyle::Space)
        .with_indent_width(IndentWidth::try_from(2u8).unwrap_or_default())
        .with_line_width(LineWidth::try_from(100u16).unwrap_or_default())
        .with_quote_style(QuoteStyle::Double)
        .with_semicolons(Semicolons::Always)
        .with_trailing_commas(TrailingCommas::All);
    // Third arg = embedded-language ranges (template chunks parsed as other
    // languages); empty here — generated API TS has none.
    let formatted =
        format_node(options, &parsed.syntax(), Vec::new()).map_err(|e| format!("biome format error: {e:?}"))?;
    let printed = formatted.print().map_err(|e| format!("biome print error: {e:?}"))?;
    Ok(printed.into_code())
}

#[cfg(test)]
mod tests {
    use super::format_ts;

    #[test]
    fn canonical_double_quotes_and_semicolons() {
        let out = format_ts("import {invoke} from '@tauri-apps/api/core'\n").expect("format");
        assert!(out.contains("\"@tauri-apps/api/core\""), "double quotes; got: {out}");
        assert!(out.trim_end().ends_with(';'), "semicolon; got: {out}");
        assert!(out.contains("{ invoke }"), "spaced braces; got: {out}");
    }

    #[test]
    fn two_space_indent_with_semicolons() {
        let out = format_ts("export interface Foo{a:string,b:number}\n").expect("format");
        assert!(out.contains("  a: string;"), "2-space + semicolon; got: {out}");
        assert!(out.contains("  b: number;"), "2-space + semicolon; got: {out}");
    }

    #[test]
    fn idempotent() {
        let once = format_ts("const x={a:1,b:2}\n").expect("format");
        let twice = format_ts(&once).expect("reformat");
        assert_eq!(once, twice, "formatting must be a fixed point");
    }
}
