use crate::doubles::{fails, idle, says, Prompt, Script};
use crate::harness::{fixture, post, service, Env, Options};
use axum::http::StatusCode;
use base64::Engine as _;
use serde_json::{json, Value};
use std::sync::Arc;

const QUESTION: &str = "The image has two panels. LEFT: the Tamil letter as printed in a font.\nRIGHT: the same letter as a learner wrote it with a finger on a blank pad.\nThe letter is \"அ\" (a).\n\nJudge only whether the RIGHT panel reads as the LEFT panel's letter to\nsomeone who reads Tamil. Compare shapes: the loops, the bars, where the\nstems are, how many parts there are. Ignore size, position, line thickness\nand neatness -- a finger is shaky, and shaky is fine. Missing a part,\nadding a part, or a shape that reads as a DIFFERENT Tamil letter is wrong\nhowever tidy it is.\n\nBe lenient: a small slip -- a dot or mark a little off, a stroke short or\nwobbly -- still counts when a reader would recognise the letter. Say\nok=false only when it would be read as something else or a whole part is\nmissing. When you say ok=false, the summary must name the exact part that\nis wrong, missing or extra, so a reviewer can check the objection.\n\nAnswer with JSON and nothing else:\n{\"ok\": true|false, \"score\": 0-100, \"summary\": \"one sentence comparing right to left\",\n \"fix\": \"one sentence on the single thing to change, or an empty string\"}";

fn answering(reply: &str) -> Script {
    let reply = reply.to_string();
    Arc::new(move |p: &Prompt| vec![says(&p.session, &reply), idle(&p.session)])
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn given_a_handwritten_letter_when_it_is_marked_then_the_ink_is_drawn_beside_the_printed_letter_for_the_model_and_its_verdict_comes_back_tidied() {
    let env = Env::new(&[]);
    let scratch = env.dir("scratch");
    env.set("TAMIL_CHECK_SCRATCH_DIR", &scratch.to_string_lossy());
    let svc = service(&env, Options::default());
    let oc = &svc.opencode;
    let ink = String::from_utf8(fixture("ink.svg")).unwrap();
    let dot = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"8\" height=\"8\"><circle cx=\"4\" cy=\"4\" r=\"2\"/></svg>";
    let check = |over: Value| {
        let mut body = json!({ "characterId": "a", "glyph": "அ", "transliteration": "a", "svg": ink });
        for (key, value) in over.as_object().unwrap() {
            if value.is_null() {
                body.as_object_mut().unwrap().remove(key);
            } else {
                body[key] = value.clone();
            }
        }
        post("/api/tamil/check").json(body)
    };

    // Given no glyph, no svg or one that is not text, too much ink or a document that is not an svg, then 400 or 413 and opencode
    // is not called.
    for over in [json!({ "glyph": "" }), json!({ "svg": null }), json!({ "svg": 5 })] {
        let refused = svc.send(check(over)).await;
        assert_eq!((refused.status, refused.json()), (StatusCode::BAD_REQUEST, json!({ "error": "glyph and svg are required" })));
    }
    let heavy = svc.send(check(json!({ "svg": format!("<svg>{}</svg>", "x".repeat(2_000_001)) }))).await;
    assert_eq!((heavy.status, heavy.json()), (StatusCode::PAYLOAD_TOO_LARGE, json!({ "error": "that is a lot of ink" })));
    let html = svc.send(check(json!({ "svg": "<html></html>" }))).await;
    assert_eq!((html.status, html.json()), (StatusCode::BAD_REQUEST, json!({ "error": "svg must be an <svg> document" })));
    assert!(oc.requests().is_empty());

    // When a letter is marked, then the model was shown one picture two panels wide (the printed letter, a grey rule, the ink) with
    // the letter and its sound, in an empty directory of its own under the scratch directory, and its verdict comes back.
    oc.on_prompt(answering(&json!({ "ok": true, "score": 88, "summary": "The right panel reads as the left.", "fix": "close the loop" }).to_string()));
    let marked = svc.send(check(json!({}))).await;
    assert_eq!((marked.status, marked.json()), (StatusCode::OK, json!({ "ok": true, "score": 88, "summary": "The right panel reads as the left.", "fix": "close the loop" })));
    let directory = oc.find("GET", "/find/file")[0].directory().unwrap().to_string();
    assert!(directory.starts_with(&format!("{}/tamil-check-", scratch.display())), "{directory}");
    assert_eq!(oc.find("POST", "/session")[0].directory(), Some(directory.as_str()));
    let sent = oc.find("POST", "*/prompt_async")[0].body.clone().unwrap();
    assert_eq!(sent["model"], json!({ "providerID": "opencode-go", "modelID": "gpt-6-luna" }));
    assert_eq!(sent["parts"][0], json!({ "type": "text", "text": QUESTION }));
    assert_eq!((sent["parts"][1]["type"].clone(), sent["parts"][1]["mime"].clone(), sent["parts"][1]["filename"].clone()), (json!("file"), json!("image/png"), json!("a.png")));
    let url = sent["parts"][1]["url"].as_str().unwrap();
    let png = base64::engine::general_purpose::STANDARD.decode(url.strip_prefix("data:image/png;base64,").expect("a PNG data URL")).unwrap();
    let picture = image::load_from_memory(&png).unwrap().to_rgb8();
    assert_eq!(picture.dimensions(), (920, 448));
    let dark = |x0: u32, x1: u32| (0..448).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|&(x, y)| picture.get_pixel(x, y)[0] < 128).count();
    assert!(dark(0, 448) > 500 && dark(472, 920) > 500, "both panels are drawn");
    assert_eq!(picture.get_pixel(459, 200)[0], 153);

    // When the svg has leading whitespace, no id or an id that names a word, then it is still marked (the picture named letter.png
    // without an id) and a word is asked about as a word; an svg that does not parse is a 502 and nothing is asked.
    oc.forget_requests();
    assert_eq!(svc.send(check(json!({ "svg": format!("\n  {ink}"), "characterId": null }))).await.status, StatusCode::OK);
    assert_eq!(oc.find("POST", "*/prompt_async")[0].body.as_ref().unwrap()["parts"][1]["filename"], "letter.png");
    svc.send(check(json!({ "characterId": "word-ilai", "glyph": "இலை", "transliteration": "ilai" }))).await;
    let asked = oc.find("POST", "*/prompt_async")[1].body.clone().unwrap()["parts"][0]["text"].as_str().unwrap().to_string();
    assert!(asked.contains("LEFT: the Tamil word as printed in a font.") && asked.contains("The word is \"இலை\" (ilai).") && asked.contains("LEFT panel's word to"));
    let unparsed = svc.send(check(json!({ "svg": "<svg" }))).await;
    assert_eq!(unparsed.status, StatusCode::BAD_GATEWAY);
    assert!(unparsed.json()["error"].is_string());
    assert_eq!(oc.find("POST", "*/prompt_async").len(), 2);

    // When the model's verdict is untidy, then it is tidied: the JSON is found in prose, the score clamped and rounded, ok follows
    // the score (80 or more passes whatever the flag says), text trimmed, and what is missing or not text is a default or left out.
    let untidy = [
        ("Here is my verdict: {\"ok\":true,\"score\":150,\"summary\":\"  fine  \",\"fix\":\"  move it  \"} -- bye", json!({ "ok": true, "score": 100, "summary": "fine", "fix": "move it" })),
        ("{\"ok\":true,\"score\":20,\"summary\":\"no\",\"fix\":\"\"}", json!({ "ok": false, "score": 20, "summary": "no" })),
        ("{\"ok\":false,\"score\":82,\"summary\":\"close\",\"fix\":\"\"}", json!({ "ok": true, "score": 82, "summary": "close" })),
        ("{\"ok\":false,\"score\":70,\"summary\":\"close\",\"fix\":\"\"}", json!({ "ok": false, "score": 70, "summary": "close" })),
        ("{\"ok\":true,\"score\":\"71.6\"}", json!({ "ok": true, "score": 72, "summary": "No comment came back." })),
        ("{\"ok\":false,\"score\":-5,\"summary\":\"x\",\"fix\":3}", json!({ "ok": false, "score": 0, "summary": "x" })),
        ("{\"ok\":true}", json!({ "ok": false, "score": 0, "summary": "No comment came back." })),
        ("{\"ok\":true,\"score\":\"high\",\"summary\":\"x\"}", json!({ "ok": false, "score": null, "summary": "x" })),
        // a reply cut off mid-summary (seen on the device on 17 Oct) still marks it
        ("{\"ok\": true, \"score\": 85, \"summary\": \"The left loop {a} matches the model, the tail is a little", json!({ "ok": true, "score": 85, "summary": "The left loop {a} matches the model, the tail is a little…" })),
    ];
    for (reply, verdict) in untidy {
        oc.on_prompt(answering(reply));
        let tidied = svc.send(check(json!({ "svg": dot }))).await;
        assert_eq!((tidied.status, tidied.json()), (StatusCode::OK, verdict), "{reply}");
    }

    // When the reply has no JSON or the model fails, then 502 quotes the reply or carries the failure.
    for reply in ["I could not tell.", "} {"] {
        oc.on_prompt(answering(reply));
        let unread = svc.send(check(json!({ "svg": dot }))).await;
        assert_eq!((unread.status, unread.json()), (StatusCode::BAD_GATEWAY, json!({ "error": format!("no JSON in the model's reply: {reply}") })));
    }
    oc.on_prompt(Arc::new(|p: &Prompt| vec![fails(&p.session, "overloaded")]));
    let failed = svc.send(check(json!({ "svg": dot }))).await;
    assert_eq!((failed.status, failed.json()), (StatusCode::BAD_GATEWAY, json!({ "error": "opencode session error: overloaded" })));
}
