use crate::doubles::{FakeCommands, Run};
use crate::harness::{fixture, post, service, Env, Form, Options};
use axum::http::StatusCode;
use serde_json::json;

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `pdftotext` that reads a real PDF as two pages and refuses anything else.
fn document_tools(commands: &FakeCommands) {
    commands.answer(|run: &Run| match run.program.as_str() {
        "pdftotext" => {
            let pdf = std::fs::read(&run.args[2]).unwrap_or_default();
            if pdf.starts_with(b"%PDF") {
                run.ok("Quarterly report: remote-chat-agent migration\n\x0cPage two, line 8: café — naïve\n\x0c")
            } else {
                run.fails("", "Syntax Error: Couldn't find trailer dictionary\nSyntax Error: Couldn't read xref table\n")
            }
        }
        _ => None,
    });
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_documents_when_they_are_converted_then_their_text_comes_back_and_bad_uploads_are_refused() {
    let env = Env::new(&[("OPENCODE_DEFAULT_MODEL", "opencode-go/deepseek-v4.1-flash"), ("OPENCODE_CWD", "/tmp")]);
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    document_tools(&svc.commands);
    let (pdf, docx) = (fixture("sample.pdf"), fixture("sample.docx"));

    // Given an upload, when it is converted, then a PDF's pages come back joined by a blank line, a Word document's paragraphs in
    // order, whatever field carries the file; no file is a 400, another type a 415, and a file that is not what it says a 422.
    let converted = svc.send(post("/api/convert").form(Form::default().file("file", "sample.pdf", "application/pdf", &pdf))).await;
    assert_eq!(converted.status, StatusCode::OK);
    assert_eq!(converted.json(), json!({ "text": "Quarterly report: remote-chat-agent migration\n\nPage two, line 8: café — naïve" }));
    let pdftotext = &svc.commands.of("pdftotext")[0].args;
    assert_eq!((pdftotext.len(), &pdftotext[..2], pdftotext[3].as_str()), (4, &["-enc".to_string(), "UTF-8".to_string()][..], "-"));
    let other_field = svc.send(post("/api/convert").form(Form::default().file("upload", "x.pdf", "application/pdf", &pdf))).await;
    assert!(other_field.json()["text"].as_str().unwrap().starts_with("Quarterly report"));
    let word = svc.send(post("/api/convert").form(Form::default().file("file", "sample.docx", DOCX, &docx))).await;
    let text = words(word.json()["text"].as_str().unwrap());
    for said in ["Migration notes", "The first paragraph explains the goal in plain words.", "first item", "second item", "convert", "pinned", "A closing paragraph."] {
        assert!(text.contains(said), "{said} in {text}");
    }
    assert!(text.find("Migration notes") < text.find("A closing paragraph."));
    let no_file = svc.send(post("/api/convert").form(Form::default().field("note", "x"))).await;
    assert_eq!((no_file.status, no_file.json()), (StatusCode::BAD_REQUEST, json!({ "error": "No file uploaded" })));
    let plain = svc.send(post("/api/convert").form(Form::default().file("file", "doc.bin", "text/plain", b"not a document"))).await;
    assert_eq!((plain.status, plain.json()), (StatusCode::UNSUPPORTED_MEDIA_TYPE, json!({ "error": "Unsupported file type" })));
    let broken_pdf = svc.send(post("/api/convert").form(Form::default().file("file", "doc.bin", "application/pdf", b"not a document"))).await;
    assert_eq!((broken_pdf.status, broken_pdf.json()), (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": "Syntax Error: Couldn't find trailer dictionary" })));
    let broken_docx = svc.send(post("/api/convert").form(Form::default().file("file", "doc.bin", DOCX, b"not a document"))).await;
    assert_eq!(broken_docx.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!broken_docx.json()["error"].as_str().unwrap().is_empty());
    let mut partless = Vec::new();
    {
        let mut archive = zip::ZipWriter::new(std::io::Cursor::new(&mut partless));
        archive.start_file::<_, ()>("other.xml", zip::write::SimpleFileOptions::default()).unwrap();
        archive.finish().unwrap();
    }
    let no_main_part = svc.send(post("/api/convert").form(Form::default().file("file", "doc.docx", DOCX, &partless))).await;
    assert_eq!(no_main_part.json(), json!({ "error": "Could not find main document part. Are you sure this is a valid .docx file?" }));
    assert!(oc.requests().is_empty());
}
