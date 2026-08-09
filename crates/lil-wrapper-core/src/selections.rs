use crate::model::{Block, BlockKind, ParsedLine};
use crate::wrapping::wrap_lines;
use crate::{Edit, Selection, WrapRequest};

#[derive(Clone, Copy, Debug)]
struct LineRange {
    start: usize,
    end: isize,
}

impl LineRange {
    fn from_selection(selection: Selection) -> Self {
        let start = selection.anchor.line.min(selection.active.line);
        let end = selection.anchor.line.max(selection.active.line);
        if start == end && selection.anchor.character == selection.active.character {
            return Self {
                start,
                end: isize::try_from(start).unwrap_or(isize::MAX) - 1,
            };
        }
        if selection.active.line > selection.anchor.line && selection.active.character == 0 {
            return Self {
                start: selection.anchor.line,
                end: isize::try_from(selection.active.line - 1).unwrap_or(isize::MAX),
            };
        }
        if selection.anchor.line > selection.active.line && selection.anchor.character == 0 {
            return Self {
                start: selection.active.line,
                end: isize::try_from(selection.anchor.line - 1).unwrap_or(isize::MAX),
            };
        }
        Self {
            start,
            end: isize::try_from(end).unwrap_or(isize::MAX),
        }
    }

    fn is_empty(self) -> bool {
        self.end < isize::try_from(self.start).unwrap_or(isize::MAX)
    }

    fn touches(self, start: usize, end: usize) -> bool {
        if self.is_empty() {
            self.start >= start && self.start < end
        } else {
            self.start < end && usize::try_from(self.end).is_ok_and(|range_end| range_end >= start)
        }
    }
}

fn normalize(ranges: Vec<LineRange>) -> Vec<LineRange> {
    let mut output: Vec<LineRange> = Vec::new();
    for next in ranges {
        let Some(current) = output.last_mut() else {
            output.push(next);
            continue;
        };
        if current.end < isize::try_from(next.start).unwrap_or(isize::MAX) {
            output.push(next);
        } else if current.is_empty() && next.is_empty() {
            *current = next;
        } else if current.is_empty() {
            let shifted_start = next.start + 1;
            if usize::try_from(next.end).is_ok_and(|end| end >= shifted_start) {
                output.push(LineRange {
                    start: shifted_start,
                    end: next.end,
                });
            }
        } else if next.is_empty() {
            let empty = next;
            if current.start < next.start {
                current.end = isize::try_from(next.start - 1).unwrap_or(isize::MAX);
                output.push(empty);
            } else {
                *current = empty;
            }
        } else {
            current.end = current.end.max(next.end);
        }
    }
    output
}

fn selected_segments(
    block: &Block,
    ranges: &[LineRange],
    whole_comment_selected: bool,
) -> Vec<(usize, usize, bool)> {
    if whole_comment_selected {
        return vec![(0, block.lines.len(), true)];
    }
    let touching = ranges
        .iter()
        .copied()
        .filter(|range| range.touches(block.start, block.end()))
        .collect::<Vec<_>>();
    if touching.iter().any(|range| range.is_empty()) {
        return vec![(0, block.lines.len(), true)];
    }
    if touching.is_empty() {
        return vec![(0, block.lines.len(), false)];
    }

    let mut selected = vec![false; block.lines.len()];
    for range in touching {
        let start = range.start.saturating_sub(block.start);
        let end = usize::try_from(range.end)
            .unwrap_or_default()
            .saturating_sub(block.start)
            .min(block.lines.len() - 1);
        selected[start..=end].fill(true);
    }
    let mut segments = Vec::new();
    let mut start = 0;
    while start < selected.len() {
        let value = selected[start];
        let mut end = start + 1;
        while end < selected.len() && selected[end] == value {
            end += 1;
        }
        segments.push((start, end, value));
        start = end;
    }
    segments
}

pub(crate) fn render_selected(request: &WrapRequest, blocks: &[Block]) -> Edit {
    let select_all = request.selections.is_empty();
    let ranges = if select_all {
        vec![LineRange {
            start: 0,
            end: isize::MAX,
        }]
    } else {
        normalize(
            request
                .selections
                .iter()
                .copied()
                .map(LineRange::from_selection)
                .collect(),
        )
    };
    let mut output = Vec::new();
    for block in blocks {
        let whole_comment_selected = request.settings.whole_comment
            && block.comment.is_some()
            && blocks.iter().any(|candidate| {
                candidate.comment == block.comment
                    && ranges.iter().any(|range| {
                        range.is_empty() && range.touches(candidate.start, candidate.end())
                    })
            });
        for (start, end, selected) in selected_segments(block, &ranges, whole_comment_selected) {
            let lines = &block.lines[start..end];
            if selected {
                match &block.kind {
                    BlockKind::Wrap { default_tail } => output.extend(wrap_lines(
                        lines,
                        (start == 0).then_some(default_tail.as_deref()).flatten(),
                        request.settings,
                    )),
                    BlockKind::NoWrap => output.extend(lines.iter().map(ParsedLine::original)),
                }
            } else {
                output.extend(lines.iter().map(ParsedLine::original));
            }
        }
    }

    let mut start = 0;
    while start < request.lines.len().min(output.len()) && request.lines[start] == output[start] {
        start += 1;
    }
    let mut suffix = 0;
    while suffix < request.lines.len().saturating_sub(start)
        && suffix < output.len().saturating_sub(start)
        && request.lines[request.lines.len() - 1 - suffix] == output[output.len() - 1 - suffix]
    {
        suffix += 1;
    }
    if start == request.lines.len() && start == output.len() {
        return Edit::empty_with_selections(&request.selections);
    }
    Edit {
        start_line: start,
        end_line: isize::try_from(request.lines.len() - suffix - 1).unwrap_or(isize::MAX),
        lines: output[start..output.len() - suffix].to_vec(),
        selections: request.selections.clone(),
    }
}
