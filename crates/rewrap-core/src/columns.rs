use crate::DocState;

#[derive(Debug, Default)]
pub struct ColumnState {
    last_document: Option<DocState>,
    columns: Vec<(String, usize)>,
}

impl ColumnState {
    fn stored_column(&self, file_path: &str) -> Option<usize> {
        self.columns
            .iter()
            .find_map(|(path, column)| (path == file_path).then_some(*column))
    }

    fn set_column(&mut self, file_path: &str, column: usize) {
        if let Some((_, current)) = self.columns.iter_mut().find(|(path, _)| path == file_path) {
            *current = column;
        } else {
            self.columns.push((file_path.to_owned(), column));
        }
    }

    #[must_use]
    pub fn wrapping_column(&mut self, file_path: &str, rulers: &[usize]) -> usize {
        let first = rulers[0];
        let selected = self
            .stored_column(file_path)
            .filter(|column| rulers.contains(column))
            .unwrap_or(first);
        self.set_column(file_path, selected);
        selected
    }

    #[must_use]
    pub fn maybe_change_wrapping_column(&mut self, document: &DocState, rulers: &[usize]) -> usize {
        let first = rulers[0];
        let Some(current) = self.stored_column(&document.file_path) else {
            self.set_column(&document.file_path, first);
            return first;
        };
        let Some(index) = rulers.iter().position(|ruler| *ruler == current) else {
            self.set_column(&document.file_path, first);
            return first;
        };
        let selected = if self.last_document.as_ref() == Some(document) {
            rulers[(index + 1) % rulers.len()]
        } else {
            current
        };
        self.set_column(&document.file_path, selected);
        selected
    }

    pub fn save_document(&mut self, document: DocState) {
        self.last_document = Some(document);
    }
}
