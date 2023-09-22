use std::fmt;

use itertools::Itertools;


#[derive(Clone, Debug, Default)]
pub struct TD {
    pub text: String,
    pub classes: Vec<String>,
    pub row_span: Option<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct HtmlTable {
    rows: Vec<String>,
}

pub fn td(text: impl fmt::Display) -> TD { td_safe(html_escape::encode_text(&text.to_string())) }

pub fn td_safe(text: impl fmt::Display) -> TD {
    TD {
        text: text.to_string(),
        classes: vec![],
        row_span: None,
    }
}

impl TD {
    pub fn with_classes(mut self, classes: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.classes.extend(classes.into_iter().map(|class| class.into()));
        self
    }

    pub fn with_row_span(mut self, row_span: usize) -> Self {
        self.row_span = Some(row_span);
        self
    }

    pub fn to_html(&self) -> String {
        let mut attributes = vec![];
        if let Some(row_span) = self.row_span {
            attributes.push(format!("rowspan='{}'", row_span));
        }
        if !self.classes.is_empty() {
            attributes.push(format!("class='{}'", self.classes.join(" ")));
        }
        format!("<td {}>{}</td>", attributes.join(" "), &self.text)
    }
}

impl HtmlTable {
    pub fn new() -> Self { HtmlTable { rows: vec![] } }

    pub fn num_rows(&self) -> usize { self.rows.len() }

    pub fn add_row(&mut self, row: impl IntoIterator<Item = TD>) {
        self.rows
            .push(format!("<tr>{}</tr>", row.into_iter().map(|cell| cell.to_html()).join("")));
    }

    pub fn to_html(&self) -> String { format!("<table>{}</table>", self.rows.join("")) }
}
