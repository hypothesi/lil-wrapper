use rewrap_core::{CustomMarkers, Settings};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const DEFAULT_TAB_WIDTH: usize = 4;
const DEFAULT_COLUMN_NUMBER: f64 = 80.0;
const DEFAULT_TAB_WIDTH_NUMBER: f64 = 4.0;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(untagged)]
pub enum Ruler {
    Column(usize),
    Detailed { column: usize },
}

impl Ruler {
    const fn column(&self) -> usize {
        match self {
            Self::Column(column) | Self::Detailed { column } => *column,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawSettings {
    pub wrapping_column: Option<i64>,
    pub rulers: Vec<Ruler>,
    pub word_wrap_column: i64,
    pub tab_width: f64,
    pub double_sentence_spacing: bool,
    pub reformat: bool,
    pub whole_comment: bool,
}

#[must_use]
pub fn resolve_settings(raw: &RawSettings) -> Settings {
    let columns = columns_from_raw(raw);
    Settings {
        column: columns[0],
        tab_width: valid_tab_width(raw.tab_width),
        double_sentence_spacing: raw.double_sentence_spacing,
        reformat: raw.reformat,
        whole_comment: raw.whole_comment,
    }
}

fn columns_from_raw(raw: &RawSettings) -> Vec<usize> {
    if let Some(column) = raw.wrapping_column.filter(|column| *column != 0) {
        return vec![valid_column(column)];
    }
    if raw.rulers.first().is_some_and(|ruler| match ruler {
        Ruler::Column(column) => *column != 0,
        Ruler::Detailed { .. } => true,
    }) {
        return raw.rulers.iter().map(Ruler::column).collect();
    }
    vec![valid_column(raw.word_wrap_column)]
}

fn valid_column(column: i64) -> usize {
    usize::try_from(column).unwrap_or(0)
}

fn valid_tab_width(tab_width: f64) -> usize {
    integral_usize(tab_width).unwrap_or(DEFAULT_TAB_WIDTH)
}

#[derive(Clone, Copy, Debug)]
struct CoreOptions {
    double_sentence_spacing: bool,
    reformat: bool,
    whole_comment: bool,
}

#[derive(Clone, Copy, Debug)]
struct ConfiguredRuler {
    column: f64,
    detailed: bool,
}

#[derive(Clone, Debug)]
pub struct Configuration {
    wrapping_column: Option<f64>,
    rulers: Vec<ConfiguredRuler>,
    word_wrap_column: f64,
    tab_width: f64,
    core: CoreOptions,
    auto_wrap_enabled: bool,
    pub custom_markers: CustomMarkers,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            wrapping_column: None,
            rulers: Vec::new(),
            word_wrap_column: DEFAULT_COLUMN_NUMBER,
            tab_width: DEFAULT_TAB_WIDTH_NUMBER,
            core: CoreOptions {
                double_sentence_spacing: false,
                reformat: false,
                whole_comment: true,
            },
            auto_wrap_enabled: false,
            custom_markers: CustomMarkers::default(),
        }
    }
}

impl Configuration {
    #[must_use]
    pub fn from_value(value: &Value) -> Self {
        let settings = value.get("settings").unwrap_or(value);
        let rewrap = settings.get("rewrap").unwrap_or(settings);
        let editor = settings.get("editor").unwrap_or(settings);
        Self::from_sections(rewrap, editor)
    }

    pub fn from_sections(rewrap: &Value, editor: &Value) -> Self {
        Self {
            wrapping_column: number(rewrap.get("wrappingColumn")),
            rulers: editor
                .get("rulers")
                .and_then(Value::as_array)
                .map(|rulers| rulers.iter().filter_map(configured_ruler).collect())
                .unwrap_or_default(),
            word_wrap_column: number(editor.get("wordWrapColumn")).unwrap_or(DEFAULT_COLUMN_NUMBER),
            tab_width: number(editor.get("tabSize"))
                .or_else(|| number(rewrap.get("tabWidth")))
                .unwrap_or(DEFAULT_TAB_WIDTH_NUMBER),
            core: CoreOptions {
                double_sentence_spacing: rewrap
                    .get("doubleSentenceSpacing")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                reformat: rewrap
                    .get("reformat")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                whole_comment: rewrap
                    .get("wholeComment")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
            },
            auto_wrap_enabled: rewrap
                .pointer("/autoWrap/enabled")
                .and_then(Value::as_bool)
                .or_else(|| rewrap.get("autoWrapEnabled").and_then(Value::as_bool))
                .unwrap_or(false),
            custom_markers: custom_markers(rewrap),
        }
    }

    #[must_use]
    pub fn columns(&self) -> Vec<usize> {
        if let Some(column) = self.wrapping_column.filter(|column| *column != 0.0) {
            return vec![valid_number_column(column)];
        }
        if self
            .rulers
            .first()
            .is_some_and(|ruler| ruler.detailed || ruler.column != 0.0)
        {
            return self
                .rulers
                .iter()
                .map(|ruler| valid_number_column(ruler.column))
                .collect();
        }
        vec![valid_number_column(self.word_wrap_column)]
    }

    /// Builds core settings for the selected column.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured tab size is not a positive integer.
    pub fn settings(
        &self,
        column: usize,
        tab_width: Option<f64>,
    ) -> Result<Settings, &'static str> {
        let tab_width = integral_usize(tab_width.unwrap_or(self.tab_width))
            .ok_or("tab size must be a positive integer")?;
        Ok(Settings {
            column,
            tab_width,
            double_sentence_spacing: self.core.double_sentence_spacing,
            reformat: self.core.reformat,
            whole_comment: self.core.whole_comment,
        })
    }

    #[must_use]
    pub const fn auto_wrap_enabled(&self) -> bool {
        self.auto_wrap_enabled
    }
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.and_then(Value::as_f64)
}

fn configured_ruler(value: &Value) -> Option<ConfiguredRuler> {
    if let Some(column) = value.as_f64() {
        return Some(ConfiguredRuler {
            column,
            detailed: false,
        });
    }
    value
        .get("column")
        .and_then(Value::as_f64)
        .map(|column| ConfiguredRuler {
            column,
            detailed: true,
        })
}

fn valid_number_column(column: f64) -> usize {
    integral_usize(column).unwrap_or(0)
}

fn integral_usize(number: f64) -> Option<usize> {
    if !number.is_finite() || number.fract() != 0.0 || number < 1.0 {
        return None;
    }
    format!("{number:.0}").parse().ok()
}

fn custom_markers(rewrap: &Value) -> CustomMarkers {
    let markers = rewrap.get("customMarkers").unwrap_or(rewrap);
    let line = markers
        .get("lineComment")
        .or_else(|| markers.get("line"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let block_value = markers.get("blockComment").or_else(|| markers.get("block"));
    let block = block_value
        .and_then(Value::as_array)
        .filter(|parts| parts.len() >= 2)
        .and_then(|parts| Some((parts[0].as_str()?.to_owned(), parts[1].as_str()?.to_owned())))
        .unwrap_or_default();
    CustomMarkers { line, block }
}
