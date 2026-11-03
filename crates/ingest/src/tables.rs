// SPDX-License-Identifier: AGPL-3.0-or-later
//! Tables: CSV and XLSX, rows to objects (M6 Task 4, ADR-0026).
//!
//! A table becomes a `Dataset` object holding the file, with one object per row under it. The
//! rows' type is named from the file (CSV) or the sheet (XLSX): an existing type of that name is
//! reused and the kinds of its fields rule, otherwise one field per column with its kind inferred
//! from the column's values (boolean, number, date, datetime, URL, email, else text; a column that
//! is all empty is text). The first column is the title unless `title_column` names another. A
//! cell that does not fit its column's kind is kept as text under `props_json._import` and
//! counted in the warnings (M6 Decision 11); it never fails the row. A workbook becomes a
//! `Dataset` per sheet under one for the file.
//!
//! Caps: a cell over [`MAX_CELL_BYTES`] is cut and said so; rows past [`MAX_ROWS`] are left out
//! and counted. The file is read whole under the source cap. Nothing here panics on bad input:
//! bytes that are not UTF-8 are read lossily with a warning, and what the parsers refuse is an
//! [`IngestError::Unreadable`].

use std::collections::{BTreeSet, HashMap};
use std::io::Cursor;

use calamine::{Data, Reader as _, Xlsx as Workbook, open_workbook_from_rs};
use dbme_core::model::{FieldKind, FieldRow, Props, key_from_label};
use dbme_core::value::validate_field_value;
use dbme_core::{ContentHash, Id};
use serde_json::{Map, Value, json};

use crate::apply::{date_of, datetime_of};
use crate::importer::{
    AttachmentDraft, Ctx, Extract, ExtractedObject, FieldDraft, Importer, ProvenanceDraft,
    TypeDraft,
};
use crate::{IngestError, Sniffed, Source};

/// Rows past this many are left out, and the warning says how many.
pub const MAX_ROWS: usize = 1_000_000;
/// A cell longer than this is cut to it.
pub const MAX_CELL_BYTES: usize = 1 << 20;
/// The key under which cells that did not fit their column's kind are kept, as text.
pub const KEPT_AS_TEXT: &str = "_import";
/// A title longer than this is cut (the model allows 10,000; a cell that long is not a title).
const TITLE_CHARS: usize = 1_000;

/// The CSV importer: `.csv` and `.tsv`, the delimiter sniffed from the first line.
#[derive(Clone, Copy, Debug, Default)]
pub struct Csv;

/// The XLSX importer: `.xlsx` and `.xlsm`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Xlsx;

impl Importer for Csv {
    fn id(&self) -> &'static str {
        "csv"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["csv", "tsv"])
    }

    fn plan(&self, source: &Source, ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let bytes = source.read()?;
        let name = type_name(&source.stem());
        csv_extract(
            source,
            &bytes,
            &existing_fields(ctx, &name),
            ctx.options.title_column.as_deref(),
        )
    }
}

/// What a CSV becomes: a `Dataset` holding the file, one object per row under it, and the type
/// the rows need. `existing` is that type's fields as the vault has them, which rule over the
/// inference; the probe for the fuzzers passes none.
pub(crate) fn csv_extract(
    source: &Source,
    bytes: &[u8],
    existing: &HashMap<String, FieldKind>,
    title_column: Option<&str>,
) -> Result<Extract, IngestError> {
    let hash = ContentHash::of(bytes);
    let table = read_csv(source, bytes)?;
    let mut extract = Extract::default();
    let name = type_name(&source.stem());
    let (draft, rows) = table.into_objects(
        &name,
        source,
        &hash,
        existing,
        title_column,
        &mut extract.warnings,
    );
    let mut dataset = dataset(source, &hash, &source.stem());
    dataset.children = rows;
    extract.types.push(draft);
    extract.objects.push(dataset);
    Ok(extract)
}

/// The fields of the vault's type of this name, by key: an existing type's kinds rule over the
/// inference (M6 Task 4). Empty when the vault has no such type.
fn existing_fields(ctx: &Ctx<'_>, type_name: &str) -> HashMap<String, FieldKind> {
    ctx.read
        .types()
        .ok()
        .and_then(|types| types.into_iter().find(|t| t.name == type_name))
        .and_then(|t| ctx.read.fields(t.id).ok())
        .map(|fields| fields.into_iter().map(|f| (f.key, f.kind)).collect())
        .unwrap_or_default()
}

/// Whether a spreadsheet would read the cell as a formula: it begins with `=` or `@`, or with a
/// `+` or `-` that does not start a number (brief §10, formula injection). Nothing here evaluates
/// anything and such a cell is kept as text; the plan counts them so the person knows, and a
/// later export to a spreadsheet must quote them.
pub(crate) fn looks_like_formula(text: &str) -> bool {
    let trimmed = text.trim_start();
    match trimmed.as_bytes().first() {
        Some(b'=' | b'@') => true,
        Some(b'+' | b'-') => trimmed.parse::<f64>().is_err(),
        _ => false,
    }
}

impl Importer for Xlsx {
    fn id(&self) -> &'static str {
        "xlsx"
    }

    fn accepts(&self, sniffed: &Sniffed) -> bool {
        sniffed.has_extension(&["xlsx", "xlsm"])
    }

    fn plan(&self, source: &Source, ctx: &Ctx<'_>) -> Result<Extract, IngestError> {
        let bytes = source.read()?;
        let hash = ContentHash::of(&bytes);
        let mut book: Workbook<_> = open_workbook_from_rs(Cursor::new(bytes))
            .map_err(|e| IngestError::unreadable(&source.path, e))?;
        let mut extract = Extract::default();
        let mut file = dataset(source, &hash, &source.stem());
        for sheet in book.sheet_names() {
            let range = book
                .worksheet_range(&sheet)
                .map_err(|e| IngestError::unreadable(&source.path, e))?;
            let Some(table) = read_sheet(&sheet, &range, &source.uri(), &mut extract.warnings)
            else {
                continue;
            };
            let name = type_name(&sheet);
            let (draft, rows) = table.into_objects(
                &name,
                source,
                &hash,
                &existing_fields(ctx, &name),
                ctx.options.title_column.as_deref(),
                &mut extract.warnings,
            );
            let mut dataset = ExtractedObject::new(
                &sheet,
                "Dataset",
                ProvenanceDraft {
                    source_uri: format!("{}#sheet={sheet}", source.uri()),
                    original_blob: Some(hash.clone()),
                    confidence: 1.0,
                },
            );
            dataset.children = rows;
            extract.types.push(draft);
            file.children.push(dataset);
        }
        if file.children.is_empty() {
            extract
                .warnings
                .push("no sheet has a header row and data".into());
        }
        extract.objects.push(file);
        Ok(extract)
    }
}

/// The `Dataset` that holds the file.
fn dataset(source: &Source, hash: &ContentHash, title: &str) -> ExtractedObject {
    let mut object = ExtractedObject::new(
        title,
        "Dataset",
        ProvenanceDraft::from_source(source, Some(hash.clone())),
    );
    object
        .attachments
        .push(AttachmentDraft::original(source, hash.clone()));
    object
}

/// A type name from a file stem or a sheet name: trimmed, bounded, never blank.
fn type_name(text: &str) -> String {
    let trimmed = text.trim();
    let name: String = if trimmed.is_empty() {
        "Table".to_owned()
    } else {
        trimmed.chars().take(200).collect()
    };
    name
}

// ---- cells and tables -------------------------------------------------------------------------

/// A cell as the inference sees it.
#[derive(Clone, Debug, PartialEq)]
enum Cell {
    Empty,
    Text(String),
    Number(f64),
    Bool(bool),
    Date(String),
    Datetime(String),
}

impl Cell {
    /// A CSV cell, or a workbook's string cell: typed by what the text looks like.
    fn from_text(text: &str) -> Self {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Self::Empty;
        }
        match trimmed.to_ascii_lowercase().as_str() {
            "true" | "yes" => return Self::Bool(true),
            "false" | "no" => return Self::Bool(false),
            _ => {}
        }
        if trimmed
            .bytes()
            .next()
            .is_some_and(|b| b.is_ascii_digit() || b == b'-' || b == b'+' || b == b'.')
            && let Ok(number) = trimmed.parse::<f64>()
            && number.is_finite()
        {
            return Self::Number(number);
        }
        if fits(FieldKind::Date, &json!(trimmed)) {
            return Self::Date(trimmed.to_owned());
        }
        if fits(FieldKind::Datetime, &json!(trimmed)) {
            return Self::Datetime(trimmed.to_owned());
        }
        Self::Text(text.to_owned())
    }

    /// The cell as text, for a title or for `_import`.
    fn text(&self) -> String {
        match self {
            Self::Empty => String::new(),
            Self::Text(t) | Self::Date(t) | Self::Datetime(t) => t.clone(),
            Self::Number(n) => format!("{n}"),
            Self::Bool(b) => b.to_string(),
        }
    }

    /// The cell as a value of `kind`, if it fits.
    fn value_as(&self, kind: FieldKind) -> Option<Value> {
        let value = match (kind, self) {
            (_, Self::Empty) => return None,
            (FieldKind::Boolean, Self::Bool(b)) => json!(b),
            (FieldKind::Number, Self::Number(n)) => number(*n),
            (FieldKind::Date, Self::Date(d)) => json!(d),
            (FieldKind::Datetime, Self::Datetime(d)) => json!(d),
            (FieldKind::Datetime, Self::Date(d)) => json!(format!("{d}T00:00:00Z")),
            (FieldKind::Boolean | FieldKind::Number | FieldKind::Date | FieldKind::Datetime, _) => {
                return None;
            }
            (FieldKind::Url, Self::Text(t)) if looks_like_url(t) => json!(t.trim()),
            (FieldKind::Email, Self::Text(t)) if looks_like_email(t) => json!(t.trim()),
            (FieldKind::Url | FieldKind::Email, _) => return None,
            (_, other) => json!(other.text()),
        };
        fits(kind, &value).then_some(value)
    }
}

/// A JSON number: an integer when the value is whole and fits, else the float.
fn number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9_007_199_254_740_992.0 {
        json!(n as i64)
    } else {
        json!(n)
    }
}

fn looks_like_url(text: &str) -> bool {
    let t = text.trim();
    (t.starts_with("http://") || t.starts_with("https://"))
        && t.len() > 8
        && !t.contains(char::is_whitespace)
}

fn looks_like_email(text: &str) -> bool {
    let t = text.trim();
    match t.split_once('@') {
        Some((local, domain)) => {
            !local.is_empty()
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
                && !t.contains(char::is_whitespace)
                && t.matches('@').count() == 1
        }
        None => false,
    }
}

/// Whether `value` is a valid value of a field of `kind`, by the store's own rule.
fn fits(kind: FieldKind, value: &Value) -> bool {
    let field = FieldRow {
        id: Id::from_parts(0, 0),
        type_id: Id::from_parts(0, 0),
        key: "column".into(),
        label: "Column".into(),
        kind,
        options_json: json!({}),
        required: false,
        sort_order: 0,
    };
    validate_field_value(&field, value).is_ok()
}

/// A table as read: a header and rows of cells.
struct Table {
    /// The header cells as written.
    header: Vec<String>,
    rows: Vec<Vec<Cell>>,
    /// `path#row=` or `path#sheet=<name>&row=`.
    uri_prefix: String,
    /// What reading it was worth telling: cut cells, ragged rows, rows past the cap.
    pending: Vec<String>,
}

impl Table {
    /// The type draft and one object per row.
    fn into_objects(
        self,
        type_name: &str,
        source: &Source,
        hash: &ContentHash,
        existing: &HashMap<String, FieldKind>,
        title_column: Option<&str>,
        warnings: &mut Vec<String>,
    ) -> (TypeDraft, Vec<ExtractedObject>) {
        warnings.extend(self.pending.iter().cloned());
        let keys = keys_for(&self.header);
        let title_column = self.title_column(&keys, title_column);
        // An existing type's fields rule; the rest are inferred.
        let kinds: Vec<FieldKind> = (0..self.header.len())
            .map(|c| {
                existing
                    .get(&keys[c])
                    .copied()
                    .unwrap_or_else(|| infer_kind(self.rows.iter().map(|r| &r[c])))
            })
            .collect();
        let fields = (0..self.header.len())
            .filter(|&c| c != title_column)
            .map(|c| FieldDraft {
                key: keys[c].clone(),
                label: label_for(&self.header[c], c),
                kind: kinds[c],
            })
            .collect();
        let draft = TypeDraft {
            name: type_name.to_owned(),
            fields,
        };

        let mut misfits: Vec<usize> = vec![0; self.header.len()];
        let objects = self
            .rows
            .iter()
            .enumerate()
            .map(|(n, row)| {
                let mut props = Props::new();
                let mut kept = Map::new();
                for (c, cell) in row.iter().enumerate() {
                    if c == title_column || *cell == Cell::Empty {
                        continue;
                    }
                    match cell.value_as(kinds[c]) {
                        Some(value) => {
                            props.insert(keys[c].clone(), value);
                        }
                        None => {
                            misfits[c] += 1;
                            kept.insert(keys[c].clone(), json!(cell.text()));
                        }
                    }
                }
                let confidence = if kept.is_empty() { 1.0 } else { 0.8 };
                if !kept.is_empty() {
                    props.insert(KEPT_AS_TEXT.to_owned(), Value::Object(kept));
                }
                let title = row[title_column].text();
                let title: String = if title.trim().is_empty() {
                    format!("Row {}", n + 1)
                } else {
                    title.chars().take(TITLE_CHARS).collect()
                };
                let mut object = ExtractedObject::new(
                    &title,
                    type_name,
                    ProvenanceDraft {
                        source_uri: format!("{}{}", self.uri_prefix, n + 1),
                        original_blob: Some(hash.clone()),
                        confidence,
                    },
                );
                object.props = props;
                object
            })
            .collect();
        for (c, count) in misfits.iter().enumerate() {
            if *count > 0 {
                warnings.push(format!(
                    "{}: {count} cell{} of {:?} did not fit {} and {} kept as text under {KEPT_AS_TEXT}",
                    source.uri(),
                    if *count == 1 { "" } else { "s" },
                    self.header[c],
                    kinds[c].as_str(),
                    if *count == 1 { "is" } else { "are" },
                ));
            }
        }
        (draft, objects)
    }

    /// The column whose cells are the titles: the one named by header or key, else the first.
    fn title_column(&self, keys: &[String], wanted: Option<&str>) -> usize {
        wanted
            .and_then(|w| {
                let w = w.trim();
                self.header
                    .iter()
                    .position(|h| h.trim().eq_ignore_ascii_case(w))
                    .or_else(|| keys.iter().position(|k| k == w))
            })
            .unwrap_or(0)
    }
}

/// The kind that fits a column: boolean, number, date, datetime, URL, email, else text. A kind
/// wins when at most one cell in ten (and at least one cell) does not fit it.
fn infer_kind<'a>(cells: impl Iterator<Item = &'a Cell>) -> FieldKind {
    let cells: Vec<&Cell> = cells.filter(|c| **c != Cell::Empty).collect();
    if cells.is_empty() {
        return FieldKind::Text;
    }
    let allowed = cells.len().div_ceil(10);
    for kind in [
        FieldKind::Boolean,
        FieldKind::Number,
        FieldKind::Date,
        FieldKind::Datetime,
        FieldKind::Url,
        FieldKind::Email,
    ] {
        let misfits = cells.iter().filter(|c| c.value_as(kind).is_none()).count();
        if misfits <= allowed {
            // A date column with a time in it somewhere is a datetime column: the dates fit.
            if kind == FieldKind::Date && cells.iter().any(|c| matches!(c, Cell::Datetime(_))) {
                return FieldKind::Datetime;
            }
            return kind;
        }
    }
    FieldKind::Text
}

/// Field keys from headers: lower case, letters, digits and `_`, starting with a letter, at most
/// 64 long, distinct.
fn keys_for(header: &[String]) -> Vec<String> {
    let mut taken = BTreeSet::new();
    header
        .iter()
        .enumerate()
        .map(|(c, h)| {
            let mut key = key_from_label(h).unwrap_or_else(|| format!("column_{}", c + 1));
            let base = key.clone();
            let mut n = 2;
            while !taken.insert(key.clone()) {
                key = format!("{base}_{n}");
                n += 1;
            }
            key
        })
        .collect()
}

/// A field label from a header: as written, or the column's number when blank; bounded.
fn label_for(header: &str, c: usize) -> String {
    let trimmed = header.trim();
    if trimmed.is_empty() {
        format!("Column {}", c + 1)
    } else {
        trimmed.chars().take(200).collect()
    }
}

// ---- CSV --------------------------------------------------------------------------------------

/// The delimiter the first line uses most: `,`, `;` or tab.
fn sniff_delimiter(bytes: &[u8]) -> u8 {
    let line = bytes.split(|b| *b == b'\n').next().unwrap_or(&[]);
    let count = |d: u8| line.iter().filter(|b| **b == d).count();
    let (mut best, mut most) = (b',', count(b','));
    for d in *b";\t" {
        let n = count(d);
        if n > most {
            best = d;
            most = n;
        }
    }
    best
}

/// Reads a CSV: the first record is the header, the rest are rows; ragged rows are padded or
/// cut to the header and counted; the caps apply.
fn read_csv(source: &Source, bytes: &[u8]) -> Result<Table, IngestError> {
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes);
    let delimiter = sniff_delimiter(bytes);
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(false)
        .from_reader(bytes);
    let mut header: Option<Vec<String>> = None;
    let mut rows = Vec::new();
    let mut lossy = false;
    let mut cut = 0usize;
    let mut ragged = 0usize;
    let mut beyond = 0usize;
    let mut formulas = 0usize;
    for record in reader.byte_records() {
        let record = record.map_err(|e| IngestError::unreadable(&source.path, e))?;
        let cells: Vec<String> = record
            .iter()
            .map(|field| {
                let field = if field.len() > MAX_CELL_BYTES {
                    cut += 1;
                    &field[..MAX_CELL_BYTES]
                } else {
                    field
                };
                let text = String::from_utf8_lossy(field);
                lossy |= matches!(text, std::borrow::Cow::Owned(_));
                text.into_owned()
            })
            .collect();
        let Some(width) = header.as_ref().map(Vec::len) else {
            header = Some(cells);
            continue;
        };
        if rows.len() == MAX_ROWS {
            beyond += 1;
            continue;
        }
        formulas += cells.iter().filter(|t| looks_like_formula(t)).count();
        let mut row: Vec<Cell> = cells.iter().map(|t| Cell::from_text(t)).collect();
        if row.len() != width {
            ragged += 1;
            row.resize(width, Cell::Empty);
        }
        rows.push(row);
    }
    let header = header
        .filter(|h| !h.is_empty())
        .ok_or_else(|| IngestError::unreadable(&source.path, "no header row"))?;
    let mut table = Table {
        header,
        rows,
        uri_prefix: format!("{}#row=", source.uri()),
        pending: Vec::new(),
    };
    table.header.iter_mut().for_each(|h| {
        *h = h.trim().to_owned();
    });
    Ok(table.with_notes(source, lossy, cut, ragged, beyond, formulas))
}

/// The note on cells a spreadsheet would run as formulas.
fn formula_note(formulas: usize) -> String {
    format!(
        "{formulas} cell(s) begin with =, @, + or - like a formula; kept as text, nothing is run"
    )
}

impl Table {
    /// The counts of what reading cost, as warnings the extract will carry.
    fn with_notes(
        mut self,
        source: &Source,
        lossy: bool,
        cut: usize,
        ragged: usize,
        beyond: usize,
        formulas: usize,
    ) -> Self {
        let mut notes = Vec::new();
        if lossy {
            notes.push("bytes that are not UTF-8 were read as �".to_owned());
        }
        if formulas > 0 {
            notes.push(formula_note(formulas));
        }
        if cut > 0 {
            notes.push(format!(
                "{cut} cell(s) over {MAX_CELL_BYTES} bytes were cut"
            ));
        }
        if ragged > 0 {
            notes.push(format!(
                "{ragged} row(s) had more or fewer cells than the header"
            ));
        }
        if beyond > 0 {
            notes.push(format!(
                "{beyond} row(s) past the {MAX_ROWS} limit were left out"
            ));
        }
        if self.rows.is_empty() {
            notes.push("a header and no rows".to_owned());
        }
        self.pending = notes
            .into_iter()
            .map(|n| format!("{}: {n}", source.uri()))
            .collect();
        self
    }
}

// ---- XLSX -------------------------------------------------------------------------------------

/// A sheet's cells as a table: the first row is the header. `None` for an empty sheet.
fn read_sheet(
    sheet: &str,
    range: &calamine::Range<Data>,
    uri: &str,
    warnings: &mut Vec<String>,
) -> Option<Table> {
    let mut rows = range.rows();
    let header: Vec<String> = rows.next()?.iter().map(|c| cell_of(c).text()).collect();
    if header.iter().all(|h| h.trim().is_empty()) {
        warnings.push(format!(
            "sheet {sheet:?}: the first row is blank, so it has no header"
        ));
        return None;
    }
    let mut errors = 0usize;
    let mut beyond = 0usize;
    let mut formulas = 0usize;
    let mut table_rows = Vec::new();
    for row in rows {
        if table_rows.len() == MAX_ROWS {
            beyond += 1;
            continue;
        }
        let cells: Vec<Cell> = row
            .iter()
            .map(|c| {
                match c {
                    Data::Error(_) => errors += 1,
                    Data::String(s) if looks_like_formula(s) => formulas += 1,
                    _ => {}
                }
                cell_of(c)
            })
            .collect();
        if cells.iter().all(|c| *c == Cell::Empty) {
            continue;
        }
        table_rows.push(cells);
    }
    if errors > 0 {
        warnings.push(format!(
            "sheet {sheet:?}: {errors} cell(s) hold a formula error and were left empty"
        ));
    }
    if formulas > 0 {
        warnings.push(format!("sheet {sheet:?}: {}", formula_note(formulas)));
    }
    if beyond > 0 {
        warnings.push(format!(
            "sheet {sheet:?}: {beyond} row(s) past the {MAX_ROWS} limit were left out"
        ));
    }
    if table_rows.is_empty() {
        warnings.push(format!("sheet {sheet:?}: a header and no rows"));
    }
    Some(Table {
        header,
        rows: table_rows,
        uri_prefix: format!("{uri}#sheet={sheet}&row="),
        pending: Vec::new(),
    })
}

/// A workbook cell as the inference sees it. Dates come typed by their style (gate 1b): a whole
/// serial is a date, a fractional one a datetime; serial day 25569 is 1970-01-01.
fn cell_of(data: &Data) -> Cell {
    match data {
        Data::Empty => Cell::Empty,
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => Cell::from_text(s),
        Data::Float(f) => Cell::Number(*f),
        Data::Int(i) => Cell::Number(*i as f64),
        Data::Bool(b) => Cell::Bool(*b),
        Data::Error(_) => Cell::Empty,
        Data::DateTime(dt) => {
            let serial = dt.as_f64();
            let ms = ((serial - 25_569.0) * 86_400_000.0).round() as i64;
            if serial.fract() == 0.0 {
                Cell::Date(date_of(ms))
            } else {
                Cell::Datetime(datetime_of(ms))
            }
        }
    }
}
