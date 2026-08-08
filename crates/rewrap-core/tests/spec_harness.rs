mod support;

use std::collections::HashSet;

use support::specs::load_corpus;

#[test]
fn loads_every_reference_spec_without_errors() {
    let corpus = load_corpus().unwrap_or_else(|errors| {
        panic!(
            "spec parser errors:\n{}",
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        )
    });

    let primary = corpus
        .cases
        .iter()
        .filter(|case| !case.reformat_alternative)
        .count();
    let reformat = corpus
        .cases
        .iter()
        .filter(|case| case.reformat_alternative)
        .count();

    assert_eq!(corpus.files.len(), 58, "reference Markdown file count");
    assert_eq!(primary, 474, "primary reference expectations");
    assert_eq!(reformat, 8, "additional reformat expectations");
    assert_eq!(corpus.cases.len(), 482, "complete parsed expectation count");
    assert_eq!(
        corpus
            .cases
            .iter()
            .filter(|case| case.settings.reformat)
            .count(),
        12,
        "expectations skipped by the original runner"
    );
}

#[test]
fn assigns_a_unique_stable_id_to_every_expectation() {
    let corpus = load_corpus().expect("valid reference corpus");
    let ids = corpus
        .cases
        .iter()
        .map(|case| case.id.as_str())
        .collect::<HashSet<_>>();

    assert_eq!(ids.len(), corpus.cases.len());
    assert!(ids.contains("content-types/plaintext.md#1"));
    assert!(ids.iter().any(|id| id.ends_with(":reformat")));
}

#[test]
fn preserves_reference_settings_selections_and_whitespace_markers() {
    let corpus = load_corpus().expect("valid reference corpus");

    assert!(corpus.cases.iter().any(|case| case.settings.tab_width != 4));
    assert!(
        corpus
            .cases
            .iter()
            .any(|case| case.settings.double_sentence_spacing)
    );
    assert!(corpus.cases.iter().any(|case| !case.settings.whole_comment));
    assert!(corpus.cases.iter().any(|case| !case.selections.is_empty()));
    assert!(
        corpus
            .cases
            .iter()
            .flat_map(|case| case.input.iter())
            .any(|line| line.contains('\t'))
    );
    assert!(corpus.cases.iter().all(|case| {
        case.input
            .iter()
            .all(|line| !line.contains(['¦', '«', '»', '·']))
    }));
}
