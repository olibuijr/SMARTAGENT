//! semdb-backed chunk storage and retrieval.

use std::collections::HashSet;
use std::path::Path;

use semdb::cli;
use semdb::json::{escape, parse, Value};
use semdb::storage::Db;

use crate::chunk::Chunk;

#[derive(Debug, Clone)]
pub struct RetrievedChunk {
    pub id: String,
    pub score: f32,
    pub doc_id: String,
    pub source: String,
    pub order: usize,
    pub start: usize,
    pub end: usize,
    pub text: String,
    pub kind: String,
}

pub fn open_or_create(path: &Path) -> Result<Db, String> {
    if path.exists() {
        Db::open(path)
    } else {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Db::create(path)
    }
}

pub fn put_chunks(db_path: &Path, chunks: &[Chunk], vectors: &[Vec<f32>]) -> Result<usize, String> {
    if chunks.len() != vectors.len() {
        return Err(format!("{} chunks but {} vectors", chunks.len(), vectors.len()));
    }
    let mut db = open_or_create(db_path)?;
    for (chunk, vector) in chunks.iter().zip(vectors.iter()) {
        db.put(&chunk.id, &meta_json(chunk), vector.clone())?;
    }
    Ok(chunks.len())
}

pub fn retrieve(
    db_path: &Path,
    query: &[f32],
    k: usize,
    exact: bool,
    doc_filter: Option<&str>,
) -> Result<Vec<RetrievedChunk>, String> {
    let db = Db::open(db_path)?;
    let wanted = if doc_filter.is_some() { db.index.len() } else { k };
    let mut out = Vec::new();
    for (id, score, meta) in cli::search(&db, query, wanted.max(k), exact) {
        let chunk = RetrievedChunk::from_meta(&id, score, &meta)?;
        if doc_filter.map(|d| d == chunk.doc_id).unwrap_or(true) {
            out.push(chunk);
        }
        if out.len() >= k {
            break;
        }
    }
    Ok(out)
}

pub fn get(db_path: &Path, id: &str) -> Result<RetrievedChunk, String> {
    let db = Db::open(db_path)?;
    let entry = db.get(id).ok_or_else(|| format!("id '{id}' not found"))?;
    RetrievedChunk::from_meta(id, 1.0, &entry.meta)
}

pub fn delete_doc(db_path: &Path, doc_id: &str) -> Result<usize, String> {
    let mut db = Db::open(db_path)?;
    let ids: Vec<String> = db
        .index
        .iter()
        .filter_map(|(id, e)| {
            RetrievedChunk::from_meta(id, 0.0, &e.meta)
                .ok()
                .filter(|c| c.doc_id == doc_id)
                .map(|_| id.clone())
        })
        .collect();
    for id in &ids {
        db.delete(id)?;
    }
    Ok(ids.len())
}

pub fn stats(db_path: &Path) -> Result<(usize, usize, u64), String> {
    let db = Db::open(db_path)?;
    let mut docs = HashSet::new();
    for (id, e) in &db.index {
        if let Ok(chunk) = RetrievedChunk::from_meta(id, 0.0, &e.meta) {
            docs.insert(chunk.doc_id);
        }
    }
    Ok((db.index.len(), docs.len(), db.records))
}

pub fn meta_json(chunk: &Chunk) -> String {
    format!(
        r#"{{"kind":"rag_chunk","doc_id":"{}","source":"{}","chunk_order":{},"start":{},"end":{},"text":"{}","content_with_weight":"{}","doc_type_kwd":"{}","citation":"[ID:{}]"}}"#,
        escape(&chunk.doc_id),
        escape(&chunk.source),
        chunk.order,
        chunk.start,
        chunk.end,
        escape(&chunk.text),
        escape(&chunk.text),
        escape(&chunk.kind),
        escape(&chunk.id)
    )
}

impl RetrievedChunk {
    pub fn citation(&self) -> String {
        format!("[ID:{}]", self.id)
    }

    pub fn from_meta(id: &str, score: f32, meta: &str) -> Result<RetrievedChunk, String> {
        let v = parse(meta).map_err(|e| format!("bad chunk metadata for {id}: {e}"))?;
        Ok(RetrievedChunk {
            id: id.to_string(),
            score,
            doc_id: str_field(&v, "doc_id").unwrap_or_default(),
            source: str_field(&v, "source").unwrap_or_default(),
            order: num_field(&v, "chunk_order").unwrap_or(0),
            start: num_field(&v, "start").unwrap_or(0),
            end: num_field(&v, "end").unwrap_or(0),
            text: str_field(&v, "text").or_else(|| str_field(&v, "content_with_weight")).unwrap_or_default(),
            kind: str_field(&v, "doc_type_kwd").unwrap_or_else(|| "text".into()),
        })
    }
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(String::from)
}

fn num_field(v: &Value, key: &str) -> Option<usize> {
    v.get(key).and_then(Value::as_f64).map(|n| n as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_round_trips() {
        let chunk = Chunk {
            id: "doc:000000".into(),
            doc_id: "doc".into(),
            source: "a.txt".into(),
            order: 0,
            start: 2,
            end: 14,
            text: "hello\nworld".into(),
            kind: "text".into(),
        };
        let meta = meta_json(&chunk);
        let got = RetrievedChunk::from_meta(&chunk.id, 0.9, &meta).unwrap();
        assert_eq!(got.citation(), "[ID:doc:000000]");
        assert_eq!(got.text, "hello\nworld");
        assert_eq!(got.start, 2);
    }
}
