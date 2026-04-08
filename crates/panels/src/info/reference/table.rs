use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TableId(pub u64);

/// Column width
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ColumnWidth {
    Fixed(f32),
    Flex(f32),
    Auto,
}

/// Data type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataType {
    String,
    Integer,
    Float,
    Boolean,
    Timestamp,
    Custom(String),
}

/// Formatter type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FormatterType {
    Currency { decimals: u8, symbol: String },
    Percentage { decimals: u8 },
    Number { decimals: u8, thousands_separator: bool },
    DateTime { format: String },
    ColoredNumber { positive_color: String, negative_color: String },
    Custom(String),
}

/// Alignment
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum Alignment {
    Left,
    Center,
    Right,
}

/// Column definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColumnDefinition {
    pub id: String,
    pub label: String,
    pub width: ColumnWidth,
    pub data_type: DataType,
    pub formatter: Option<FormatterType>,
    pub sortable: bool,
    pub filterable: bool,
    pub alignment: Alignment,
}

/// Data source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataSourceConfig {
    Static(Vec<HashMap<String, String>>),
    Dynamic {
        source_id: String,
        update_interval_ms: u64,
    },
    Query {
        sql: String,
        database: String,
    },
}

/// Configuration for table panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableConfig {
    pub columns: Vec<ColumnDefinition>,
    pub data_source: DataSourceConfig,
    pub sortable: bool,
    pub filterable: bool,
    pub row_height: f32,
    pub header_height: f32,
    pub striped_rows: bool,
    pub selectable_rows: bool,
    pub virtualized: bool,
}

impl Default for TableConfig {
    fn default() -> Self {
        Self {
            columns: Vec::new(),
            data_source: DataSourceConfig::Static(Vec::new()),
            sortable: true,
            filterable: true,
            row_height: 30.0,
            header_height: 40.0,
            striped_rows: true,
            selectable_rows: false,
            virtualized: true,
        }
    }
}

/// Cell value
#[derive(Debug, Clone)]
pub enum CellValue {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Timestamp(i64),
    Null,
}

/// Row metadata
#[derive(Debug, Clone, Default)]
pub struct RowMetadata {
    pub color: Option<String>,
    pub bold: bool,
    pub selectable: bool,
}

/// Table row
#[derive(Debug, Clone)]
pub struct TableRow {
    pub cells: HashMap<String, CellValue>,
    pub metadata: RowMetadata,
}

/// Sort direction
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Column filter
#[derive(Debug, Clone)]
pub enum ColumnFilter {
    String { contains: String },
    Number { min: Option<f64>, max: Option<f64> },
    Boolean { value: bool },
    Custom { predicate: String },
}

/// Table state
#[derive(Clone, Debug, Default)]
pub struct TableState {
    pub rows: Vec<TableRow>,
    pub filtered_rows: Vec<usize>,
    pub sort_column: Option<String>,
    pub sort_direction: SortDirection,
    pub filters: HashMap<String, ColumnFilter>,
    pub selected_rows: Vec<usize>,
    pub scroll_offset: f32,
    pub visible_range: (usize, usize),
}

impl Default for SortDirection {
    fn default() -> Self {
        SortDirection::Ascending
    }
}

impl TableState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get column headers from row data (assuming first row keys are headers)
    pub fn column_headers(&self) -> Vec<&str> {
        if let Some(first_row) = self.rows.first() {
            let mut headers: Vec<&str> = first_row.cells.keys()
                .map(|s| s.as_str())
                .collect();
            headers.sort();
            headers
        } else {
            Vec::new()
        }
    }

    /// Get visible rows (filtered, sorted, limited to max_rows)
    pub fn visible_rows(&self, max_rows: usize) -> Vec<Vec<String>> {
        let headers = self.column_headers();

        self.filtered_rows.iter()
            .take(max_rows)
            .filter_map(|&idx| self.rows.get(idx))
            .map(|row| {
                headers.iter()
                    .map(|&header| {
                        row.cells.get(header)
                            .map(|cell| self.format_cell_value(cell))
                            .unwrap_or_else(|| String::new())
                    })
                    .collect()
            })
            .collect()
    }

    fn format_cell_value(&self, cell: &CellValue) -> String {
        match cell {
            CellValue::String(s) => s.clone(),
            CellValue::Integer(i) => i.to_string(),
            CellValue::Float(f) => format!("{:.2}", f),
            CellValue::Boolean(b) => if *b { "true" } else { "false" }.to_string(),
            CellValue::Timestamp(ts) => {
                // Format as simple date/time
                let secs = ts / 1000;
                let mins = secs / 60;
                let hours = mins / 60;
                let days = hours / 24;
                if days > 0 {
                    format!("{}d {}h", days, hours % 24)
                } else if hours > 0 {
                    format!("{}h {}m", hours, mins % 60)
                } else {
                    format!("{}m", mins)
                }
            }
            CellValue::Null => "".to_string(),
        }
    }

    /// Get proportional column widths based on total width
    pub fn column_widths(&self, total_width: f32) -> Vec<f32> {
        let headers = self.column_headers();
        let count = headers.len() as f32;

        if count == 0.0 {
            return Vec::new();
        }

        // Simple equal distribution for now
        vec![total_width / count; headers.len()]
    }

    /// Get sort indicator (column index, ascending)
    pub fn sort_indicator(&self) -> Option<(usize, bool)> {
        if let Some(ref sort_col) = self.sort_column {
            let headers = self.column_headers();
            if let Some(idx) = headers.iter().position(|&h| h == sort_col) {
                let ascending = matches!(self.sort_direction, SortDirection::Ascending);
                return Some((idx, ascending));
            }
        }
        None
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TablePanel {
    id: TableId,
    title: String,
}

impl TablePanel {
    pub fn new(id: TableId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TableId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "table"
    }

    pub fn kind_label(&self) -> &'static str {
        "Table"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 150.0)
    }
}
