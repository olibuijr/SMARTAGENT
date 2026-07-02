use std::path::PathBuf;

use semdb::storage::Db;

fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-scratch")
        .join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn ingests_and_retrieves_cited_chunks() {
    let root = scratch("rag-e2e");
    let db = root.join("docs.semdb");
    let doc = root.join("guide.txt");
    std::fs::write(
        &doc,
        "Golf swing tempo helps putting accuracy.\nPasta sauce uses basil tomatoes.",
    )
    .unwrap();

    let out = rag::cli::run(&vec![
        "ingest".into(),
        db.display().to_string(),
        doc.display().to_string(),
        "--doc-id".into(),
        "guide".into(),
        "--chunk-tokens".into(),
        "64".into(),
        "--vector".into(),
        "1,0".into(),
    ])
    .unwrap();
    assert!(out.contains("ingested 1 chunks"), "{out}");

    let semdb = Db::open(&db).unwrap();
    let entry = semdb.get("guide:000000").unwrap();
    assert_eq!(entry.vector, vec![1.0, 0.0]);
    assert!(entry.meta.contains("\"doc_id\":\"guide\""));

    let got = rag::cli::run(&vec![
        "retrieve".into(),
        db.display().to_string(),
        "--vector".into(),
        "1,0".into(),
        "--k".into(),
        "1".into(),
        "--exact".into(),
    ])
    .unwrap();
    assert!(got.contains("[ID:guide:000000]"), "{got}");
    assert!(got.contains("Golf swing tempo"), "{got}");
}

#[test]
fn reingest_replaces_stale_chunks() {
    let root = scratch("rag-reingest");
    let db = root.join("docs.semdb");
    let doc = root.join("note.txt");
    let ingest = |content: &str| {
        std::fs::write(&doc, content).unwrap();
        rag::cli::run(&vec![
            "ingest".into(),
            db.display().to_string(),
            doc.display().to_string(),
            "--doc-id".into(),
            "note".into(),
            "--chunk-tokens".into(),
            "8".into(),
            "--vector".into(),
            "1,0".into(),
        ])
        .unwrap()
    };
    // Long doc → several chunks; edited doc → one chunk. Old chunks must go.
    ingest("one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty");
    let out = ingest("short now");
    assert!(out.contains("replaced"), "{out}");
    let semdb = Db::open(&db).unwrap();
    assert!(semdb.get("note:000000").is_some());
    assert!(
        semdb.get("note:000001").is_none(),
        "stale chunk survived re-ingest"
    );
}

#[test]
fn chunks_pdf_text() {
    let root = scratch("rag-pdf");
    let pdf = root.join("plain.pdf");
    std::fs::write(
        &pdf,
        b"%PDF-1.1\n1 0 obj\n<<>>stream\nBT (Alpha PDF text) Tj <42657461> Tj ET\nendstream\n%%EOF",
    )
    .unwrap();
    let out = rag::cli::run(&vec![
        "chunk".into(),
        pdf.display().to_string(),
        "--doc-id".into(),
        "pdfdoc".into(),
        "--kind".into(),
        "pdf".into(),
    ])
    .unwrap();
    assert!(out.contains("Alpha PDF text"), "{out}");
    assert!(out.contains("Beta"), "{out}");
}
