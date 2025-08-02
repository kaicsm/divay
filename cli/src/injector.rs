use anyhow::Result;
use binrw::{io::Cursor, BinRead, BinWrite};
use csv::Reader;
use encoding_rs::WINDOWS_1252;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

#[derive(BinRead, BinWrite, Debug)]
#[br(little)]
#[bw(little)]
struct RecordHeader {
    #[br(map = |x: [u8; 4]| String::from_utf8_lossy(&x).into_owned())]
    #[bw(map = |x| { let mut arr = [0u8; 4]; arr.copy_from_slice(x.as_bytes()); arr })]
    name: String,
    size: u32,
    unknown: u32, // Flags and other metadata
    flags: u32,
}

#[derive(BinRead, BinWrite, Debug, Clone)]
#[br(little)]
#[bw(little)]
struct SubRecordHeader {
    #[br(map = |x: [u8; 4]| String::from_utf8_lossy(&x).into_owned())]
    #[bw(map = |x| { let mut arr = [0u8; 4]; arr.copy_from_slice(x.as_bytes()); arr })]
    name: String,
    size: u32,
}

#[derive(serde::Deserialize)]
struct CsvRow {
    unique_id: String,
    original_text: String,
    translated_text: String,
}

#[derive(Debug, Clone)]
struct TranslationEntry {
    original_text: String,
    translated_text: String,
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

fn encode_text(text: &str) -> Vec<u8> {
    let (encoded, _, _) = WINDOWS_1252.encode(text);
    let mut bytes = encoded.into_owned();
    if !bytes.ends_with(&[0]) {
        // Ensure null terminator
        bytes.push(0);
    }
    bytes
}

fn parse_subrecords(record_data: &[u8]) -> Result<Vec<(SubRecordHeader, Vec<u8>)>> {
    let mut sub_records = Vec::new();
    let mut cursor = Cursor::new(record_data);

    while let Ok(header) = SubRecordHeader::read_le(&mut cursor) {
        let mut data = vec![0; header.size as usize];
        cursor.read_exact(&mut data)?;
        sub_records.push((header, data));
    }

    Ok(sub_records)
}

fn rebuild_record_data(sub_records: &Vec<(SubRecordHeader, Vec<u8>)>) -> Result<Vec<u8>> {
    let mut record_data = Vec::new();
    for (sub_header, sub_data) in sub_records.iter() {
        let mut sub_rec_cursor = Cursor::new(Vec::new());
        sub_header.write_le(&mut sub_rec_cursor)?;
        sub_rec_cursor.write_all(sub_data)?;
        record_data.extend_from_slice(sub_rec_cursor.get_ref());
    }
    Ok(record_data)
}

pub fn inject(
    input_path: &Path,
    csv_path: &Path,
    output_path: &Path,
    _patch_mode: bool,
) -> Result<()> {
    println!(
        "Injecting translations from {} in {} to {}",
        csv_path.display(),
        input_path.display(),
        output_path.display()
    );

    let mut translations: HashMap<String, TranslationEntry> = HashMap::new();
    let mut rdr = Reader::from_path(csv_path)?;
    for result in rdr.deserialize() {
        let row: CsvRow = result?;
        if !row.translated_text.trim().is_empty() {
            translations.insert(
                row.unique_id,
                TranslationEntry {
                    original_text: row.original_text,
                    translated_text: row.translated_text,
                },
            );
        }
    }
    println!("Loaded {} translations from the CSV.", translations.len());

    let mut input_file = File::open(input_path)?;
    let mut output_file = File::create(output_path)?;

    let tes3_header = RecordHeader::read_le(&mut input_file)?;
    if tes3_header.name != "TES3" {
        return Err(anyhow::anyhow!("Invalid file: TES3 header not found."));
    }
    let mut tes3_data = vec![0; tes3_header.size as usize];
    input_file.read_exact(&mut tes3_data)?;

    tes3_header.write_le(&mut output_file)?;
    output_file.write_all(&tes3_data)?;

    let mut records_processed = 0;
    let mut strings_injected = 0;

    loop {
        let mut record_header = match RecordHeader::read_le(&mut input_file) {
            Ok(h) => h,
            Err(e) if e.is_eof() => break,
            Err(e) => return Err(e.into()),
        };

        let mut record_data = vec![0; record_header.size as usize];
        input_file.read_exact(&mut record_data)?;
        records_processed += 1;

        let rec_type = &record_header.name;
        let original_record_size = record_header.size as i32;
        let mut new_record_data = record_data.clone();
        let mut current_record_size_change: i32 = 0;

        if let Some(translatable_fields) = TRANSLATABLE_SUBRECORDS.get(rec_type.as_str()) {
            let mut sub_records = parse_subrecords(&record_data)?;

            let object_id = ID_SUBRECORD_CANDIDATES
                .iter()
                .find_map(|id_type| {
                    sub_records
                        .iter()
                        .find(|(header, _)| &header.name == id_type)
                        .map(|(_, data)| decode_text(data))
                })
                .unwrap_or_else(|| "UNKNOWN_ID".to_string());

            let mut modified = false;
            let mut sub_record_counts: HashMap<String, usize> = HashMap::new();
            let mut sub_record_occurrence_map: HashMap<String, usize> = HashMap::new();

            for (sub_header, _) in &sub_records {
                *sub_record_occurrence_map
                    .entry(sub_header.name.clone())
                    .or_insert(0) += 1;
            }

            for (_, (sub_header, data)) in sub_records.iter_mut().enumerate() {
                let sub_rec_type = &sub_header.name;
                let entry_count = sub_record_counts.entry(sub_rec_type.clone()).or_insert(0);

                if translatable_fields.contains(sub_rec_type.as_str()) {
                    let original_text_in_record = decode_text(data);
                    let mut unique_id = format!("{}|{}|{}", rec_type, object_id, sub_rec_type);

                    let num_occurrences =
                        *sub_record_occurrence_map.get(sub_rec_type).unwrap_or(&0);
                    if num_occurrences > 1 {
                        unique_id.push_str(&format!("_{}", *entry_count));
                    }

                    if let Some(translation_entry) = translations.get(&unique_id) {
                        if original_text_in_record.trim() == translation_entry.original_text.trim()
                        {
                            let new_encoded_text = encode_text(&translation_entry.translated_text);
                            if new_encoded_text != *data {
                                current_record_size_change +=
                                    (new_encoded_text.len() as i32) - (data.len() as i32);
                                sub_header.size = new_encoded_text.len() as u32;
                                *data = new_encoded_text;
                                modified = true;
                                strings_injected += 1;
                            }
                        } else {
                            eprintln!(
                                "Warning: Original text mismatch for {}. Record: '{}', CSV: '{}'",
                                unique_id, original_text_in_record, translation_entry.original_text
                            );
                        }
                    }
                }
                *entry_count += 1;
            }

            if modified {
                new_record_data = rebuild_record_data(&sub_records)?;
                record_header.size = (original_record_size + current_record_size_change) as u32;
            }
        }

        record_header.write_le(&mut output_file)?;
        output_file.write_all(&new_record_data)?;
    }

    output_file.flush()?;
    println!(
        "Injection complete. {} strings injected into {} records.",
        strings_injected, records_processed
    );
    Ok(())
}
