use anyhow::Result;
use binrw::{io::Cursor, BinRead};
use csv::Writer;
use encoding_rs::WINDOWS_1252;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(BinRead, Debug)]
struct RecordHeader {
    #[br(map = |x: [u8; 4]| String::from_utf8_lossy(&x).into_owned())]
    name: String,
    size: u32,
}

#[derive(BinRead, Debug)]
struct SubRecordHeader {
    #[br(map = |x: [u8; 4]| String::from_utf8_lossy(&x).into_owned())]
    name: String,
    size: u32,
}

#[derive(serde::Serialize)]
struct CsvRow<'a> {
    unique_id: String,
    record_type: &'a str,
    subrecord_type: &'a str,
    original_text: String,
    translated_text: &'static str,
}

lazy_static::lazy_static! {
    static ref TRANSLATABLE_SUBRECORDS: HashMap<&'static str, HashSet<&'static str>> = {
        let mut m = HashMap::new();
        m.insert("ACTI", ["FNAM"].iter().cloned().collect());
        m.insert("ALCH", ["FNAM"].iter().cloned().collect());
        m.insert("APPA", ["FNAM"].iter().cloned().collect());
        m.insert("ARMO", ["FNAM"].iter().cloned().collect());
        m.insert("BODY", ["FNAM"].iter().cloned().collect());
        m.insert("BOOK", ["FNAM", "TEXT"].iter().cloned().collect());
        m.insert("BSGN", ["FNAM", "DESC"].iter().cloned().collect());
        m.insert("CLAS", ["FNAM", "DESC"].iter().cloned().collect());
        m.insert("CLOT", ["FNAM"].iter().cloned().collect());
        m.insert("CONT", ["FNAM"].iter().cloned().collect());
        m.insert("CREA", ["FNAM"].iter().cloned().collect());
        m.insert("DIAL", ["NAME"].iter().cloned().collect());
        m.insert("DOOR", ["FNAM"].iter().cloned().collect());
        m.insert("ENCH", ["FNAM"].iter().cloned().collect());
        m.insert("FACT", ["FNAM"].iter().cloned().collect());
        m.insert("GLOB", ["FNAM"].iter().cloned().collect());
        m.insert("GMST", ["STRV"].iter().cloned().collect());
        m.insert("INFO", ["NAME"].iter().cloned().collect());
        m.insert("INGR", ["FNAM"].iter().cloned().collect());
        m.insert("LEVC", ["NNAM"].iter().cloned().collect());
        m.insert("LEVI", ["NNAM"].iter().cloned().collect());
        m.insert("LIGH", ["FNAM"].iter().cloned().collect());
        m.insert("LOCK", ["FNAM"].iter().cloned().collect());
        m.insert("MGEF", ["DESC"].iter().cloned().collect());
        m.insert("MISC", ["FNAM"].iter().cloned().collect());
        m.insert("NPC_", ["FNAM"].iter().cloned().collect());
        m.insert("PGRD", ["NAME"].iter().cloned().collect());
        m.insert("PROB", ["FNAM"].iter().cloned().collect());
        m.insert("RACE", ["FNAM", "DESC"].iter().cloned().collect());
        m.insert("REGN", ["FNAM"].iter().cloned().collect());
        m.insert("REPA", ["FNAM"].iter().cloned().collect());
        m.insert("SKIL", ["DESC"].iter().cloned().collect());
        m.insert("SNDG", ["FNAM"].iter().cloned().collect());
        m.insert("SOUN", ["FNAM"].iter().cloned().collect());
        m.insert("SPEL", ["FNAM"].iter().cloned().collect());
        m.insert("SSCR", ["NAME"].iter().cloned().collect());
        m.insert("STAT", ["FNAM"].iter().cloned().collect());
        m.insert("WEAP", ["FNAM"].iter().cloned().collect());
        m
    };
    static ref ID_SUBRECORD_CANDIDATES: Vec<&'static str> = vec!["NAME", "INAM", "CNAM", "BNAM", "ANAM", "NNAM"];
}

fn decode_text(bytes: &[u8]) -> String {
    let null_pos = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    let (decoded, _, _) = WINDOWS_1252.decode(&bytes[..null_pos]);
    decoded.into_owned()
}

fn is_translatable_text(text: &str) -> bool {
    let trimmed = text.trim();

    if trimmed.len() < 2 {
        return false;
    }

    let is_numeric = trimmed
        .chars()
        .all(|c| c.is_digit(10) || c == '.' || c == '-' || c == '+');
    if is_numeric && trimmed.parse::<f64>().is_ok() {
        return false;
    }

    let script_patterns = [
        "begin ",
        "end\n",
        "endif",
        "while (",
        "if (",
        "else\n",
        "getjournalindex",
        "messagebox",
        "additem",
        "removeitem",
        "startscript",
        "stopscript",
        "getglobal",
        "setglobal",
        "short ",
        "long ",
        "float ",
    ];
    let text_lower = trimmed.to_lowercase();
    if script_patterns.iter().any(|p| text_lower.starts_with(p)) {
        return false;
    }

    if trimmed.contains('\n')
        && trimmed.lines().any(|line| {
            let line_lower = line.trim().to_lowercase();
            line_lower.starts_with("if ")
                || line_lower.starts_with("set ")
                || line_lower.starts_with("short ")
                || line_lower.starts_with("long ")
                || line_lower.starts_with("float ")
        })
    {
        return false;
    }

    let code_patterns = ["==", "!=", ">=", "<=", "->", "=>", "&&", "||"];
    if code_patterns.iter().any(|p| trimmed.contains(p)) {
        return false;
    }

    let punct_count = trimmed
        .chars()
        .filter(|&c| "{}[]()=<>!&|;".contains(c))
        .count();
    if punct_count > 5 && (punct_count as f32 / trimmed.len() as f32) > 0.5 {
        return false;
    }

    if (trimmed.contains('\\') && trimmed.matches('\\').count() > 1)
        || trimmed.starts_with("data\\")
    {
        return false;
    }

    true
}

fn parse_subrecords(record_data: &[u8]) -> Result<HashMap<String, Vec<Vec<u8>>>> {
    let mut sub_records = HashMap::new();
    let mut cursor = Cursor::new(record_data);

    while let Ok(header) = SubRecordHeader::read_le(&mut cursor) {
        let mut data = vec![0; header.size as usize];
        cursor.read_exact(&mut data)?;
        sub_records
            .entry(header.name)
            .or_insert_with(Vec::new)
            .push(data);
    }

    Ok(sub_records)
}

pub fn extract(
    input_path: &Path,
    output_path: &Path,
    filter_types: Option<&HashSet<String>>,
) -> Result<()> {
    println!(
        "Extracting from {} to {}",
        input_path.display(),
        output_path.display()
    );

    let mut file = File::open(input_path)?;
    let mut wtr = Writer::from_path(output_path)?;

    let tes3_header = RecordHeader::read_le(&mut file)?;
    if tes3_header.name != "TES3" {
        return Err(anyhow::anyhow!("Invalid file: TES3 header not found."));
    }
    file.seek(SeekFrom::Current(tes3_header.size as i64))?;

    let mut record_count = 0;
    let mut string_count = 0;

    loop {
        let record_header = match RecordHeader::read_le(&mut file) {
            Ok(h) => h,
            Err(e) if e.is_eof() => break,
            Err(e) => return Err(e.into()),
        };

        let mut record_data = vec![0; record_header.size as usize];
        file.read_exact(&mut record_data)?;
        record_count += 1;

        let rec_type = &record_header.name;

        if let Some(types) = filter_types {
            if !types.contains(rec_type) {
                continue;
            }
        }

        if let Some(translatable_fields) = TRANSLATABLE_SUBRECORDS.get(rec_type.as_str()) {
            let sub_records = parse_subrecords(&record_data)?;

            let object_id = ID_SUBRECORD_CANDIDATES
                .iter()
                .find_map(|id_type| sub_records.get(*id_type).and_then(|v| v.first()))
                .map(|bytes| decode_text(bytes))
                .unwrap_or_else(|| "UNKNOWN_ID".to_string());

            for sub_rec_type in translatable_fields {
                if let Some(datas) = sub_records.get(*sub_rec_type) {
                    for (i, data) in datas.iter().enumerate() {
                        let original_text = decode_text(data);

                        if !is_translatable_text(&original_text) {
                            continue;
                        }

                        let mut unique_id = format!("{}|{}|{}", rec_type, object_id, sub_rec_type);
                        if datas.len() > 1 {
                            unique_id.push_str(&format!("_{}", i));
                        }

                        wtr.serialize(CsvRow {
                            unique_id,
                            record_type: rec_type,
                            subrecord_type: sub_rec_type,
                            original_text,
                            translated_text: "",
                        })?;
                        string_count += 1;
                    }
                }
            }
        }
    }

    wtr.flush()?;
    println!(
        "Extraction complete. Found {} strings in {} records.",
        string_count, record_count
    );
    Ok(())
}
