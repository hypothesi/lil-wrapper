mod support;

use lil_wrapper_core::{CustomMarkers, File, WrapRequest, wrap};
use support::specs::{SpecCase, load_corpus};

#[test]
fn native_rust_core_passes_every_expectation_executed_by_the_original_runner() {
    let corpus = load_corpus().expect("valid reference corpus");
    let selected = corpus.cases.iter().filter(|case| case.only).count();
    let cases = corpus
        .cases
        .iter()
        .filter(|case| selected == 0 || case.only)
        .filter(|case| !case.settings.reformat)
        .collect::<Vec<_>>();
    assert_eq!(cases.len(), 470, "original executable expectation count");
    let mut failures = Vec::new();

    for case in cases {
        let edit = wrap(&WrapRequest {
            file: File {
                language: case.language.clone(),
                path: String::new(),
                custom_markers: CustomMarkers::default(),
            },
            settings: case.settings,
            selections: case.selections.clone(),
            lines: case.input.clone(),
        });
        let actual = apply_edit(&case.input, &edit);
        if actual != case.expected {
            failures.push(describe_failure(case, &actual));
        }
    }

    assert!(
        failures.is_empty(),
        "{} compatibility expectations failed (showing up to 20):\n\n{}",
        failures.len(),
        failures
            .into_iter()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n\n")
    );
}

fn apply_edit(input: &[String], edit: &lil_wrapper_core::Edit) -> Vec<String> {
    if edit.is_empty() {
        return input.to_vec();
    }

    let end = usize::try_from(edit.end_line).expect("nonempty edit end line");
    let mut output = input[..edit.start_line].to_vec();
    output.extend(edit.lines.iter().cloned());
    output.extend(input[end + 1..].iter().cloned());
    output
}

fn describe_failure(case: &SpecCase, actual: &[String]) -> String {
    format!(
        "{} [{} col {}, tab {}, reformat {}]\ninput:    {:?}\nexpected: {:?}\nactual:   {:?}",
        case.id,
        case.language,
        case.settings.column,
        case.settings.tab_width,
        case.settings.reformat,
        case.input,
        case.expected,
        actual
    )
}
