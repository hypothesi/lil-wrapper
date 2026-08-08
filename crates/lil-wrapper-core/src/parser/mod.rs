mod comments;
mod latex;
mod markdown;
mod plain;
mod rst;
mod sgml;

use crate::RewrapRequest;
use crate::language::{LanguageKind, language_kind};
use crate::model::{Block, ParsedLine};

pub(crate) use markdown::parse_markdown;

pub(crate) fn parse_document(request: &RewrapRequest) -> Vec<Block> {
    let lines = request
        .lines
        .iter()
        .cloned()
        .map(|line| ParsedLine::new("", line))
        .collect::<Vec<_>>();
    match language_kind(&request.file) {
        LanguageKind::Plain => {
            if request.file.custom_markers.line.is_empty()
                && (request.file.custom_markers.block.0.is_empty()
                    || request.file.custom_markers.block.1.is_empty())
            {
                plain::parse_plain(&lines, 0, request.settings)
            } else {
                comments::parse_source(request, &lines)
            }
        }
        LanguageKind::Markdown => {
            let mut settings = request.settings;
            settings.reformat = false;
            parse_markdown(&lines, 0, settings, true)
        }
        LanguageKind::Source | LanguageKind::Yaml => comments::parse_source(request, &lines),
        LanguageKind::Latex => latex::parse_latex(request, &lines),
        LanguageKind::Html => sgml::parse_html(request, &lines),
        LanguageKind::Rst => rst::parse_rst(&lines, 0, request.settings),
    }
}
