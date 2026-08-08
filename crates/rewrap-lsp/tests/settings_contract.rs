use rewrap_lsp::{RawSettings, Ruler, resolve_settings};

fn raw() -> RawSettings {
    RawSettings {
        wrapping_column: None,
        rulers: Vec::new(),
        word_wrap_column: 80,
        tab_width: 4.0,
        double_sentence_spacing: false,
        reformat: false,
        whole_comment: true,
    }
}

#[test]
fn explicit_rewrap_column_precedes_editor_rulers_and_word_wrap_column() {
    let settings = resolve_settings(&RawSettings {
        wrapping_column: Some(45),
        rulers: vec![Ruler::Column(60)],
        word_wrap_column: 90,
        ..raw()
    });

    assert_eq!(settings.column, 45);
}

#[test]
fn rulers_precede_word_wrap_column_and_accept_both_vscode_shapes() {
    let numeric = resolve_settings(&RawSettings {
        rulers: vec![Ruler::Column(60), Ruler::Detailed { column: 72 }],
        word_wrap_column: 90,
        ..raw()
    });
    let detailed = resolve_settings(&RawSettings {
        rulers: vec![Ruler::Detailed { column: 88 }],
        word_wrap_column: 90,
        ..raw()
    });

    assert_eq!(numeric.column, 60);
    assert_eq!(detailed.column, 88);
}

#[test]
fn word_wrap_column_is_the_final_column_fallback() {
    let settings = resolve_settings(&RawSettings {
        word_wrap_column: 96,
        ..raw()
    });

    assert_eq!(settings.column, 96);
}

#[test]
fn detailed_zero_ruler_is_present_while_numeric_zero_falls_through() {
    let detailed = resolve_settings(&RawSettings {
        rulers: vec![Ruler::Detailed { column: 0 }],
        word_wrap_column: 96,
        ..raw()
    });
    let numeric = resolve_settings(&RawSettings {
        rulers: vec![Ruler::Column(0)],
        word_wrap_column: 96,
        ..raw()
    });

    assert_eq!(detailed.column, 0);
    assert_eq!(numeric.column, 96);
}

#[test]
fn zero_or_invalid_columns_mean_unbounded_wrapping() {
    assert_eq!(
        resolve_settings(&RawSettings {
            wrapping_column: Some(0),
            word_wrap_column: 0,
            ..raw()
        })
        .column,
        0
    );
    assert_eq!(
        resolve_settings(&RawSettings {
            wrapping_column: Some(-5),
            ..raw()
        })
        .column,
        0
    );
}

#[test]
fn validates_tab_width_and_preserves_other_rewrap_settings() {
    let configured = resolve_settings(&RawSettings {
        tab_width: 2.0,
        double_sentence_spacing: true,
        reformat: true,
        whole_comment: false,
        ..raw()
    });
    let invalid = resolve_settings(&RawSettings {
        tab_width: 2.5,
        ..raw()
    });

    assert_eq!(configured.tab_width, 2);
    assert!(configured.double_sentence_spacing);
    assert!(configured.reformat);
    assert!(!configured.whole_comment);
    assert_eq!(invalid.tab_width, 4);
}
