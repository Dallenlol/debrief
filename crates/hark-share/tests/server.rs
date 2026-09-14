use hark_share::ShareServer;
use hark_store::{Meeting, NewSegment, Store};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

fn http_get(port: u16, path: &str, extra: &str) -> (u16, String, Vec<u8>) {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n{extra}\r\n").unwrap();
    let mut buf = Vec::new();
    s.read_to_end(&mut buf).unwrap();
    let split = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..split]).to_string();
    let status: u16 = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, head, buf[split + 4..].to_vec())
}

#[test]
fn serves_share_page_and_ranged_media() {
    let dir = tempfile::tempdir().unwrap();
    let store = Arc::new(Store::open(&dir.path().join("hark.db")).unwrap());
    let m = Meeting::new_recording("Roadmap sync", None);
    store.create_meeting(&m).unwrap();
    store.replace_segments(&m.id, &[NewSegment { start_ms: 0, end_ms: 1000, speaker: Some("Ann".into()), text: "hello <world>".into() }]).unwrap();
    std::fs::write(store.recordings_dir(&m.id).join("mix.wav"), (0..=255u8).collect::<Vec<_>>()).unwrap();
    let share = store.create_share(&m.id, "meeting", None, None).unwrap();

    let server = ShareServer::start(store.clone(), dir.path().join("clips"), 0).unwrap();
    let port = server.port;

    let (status, _, body) = http_get(port, &format!("/s/{}", share.token), "");
    assert_eq!(status, 200);
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Roadmap sync"));
    assert!(html.contains("hello &lt;world&gt;"));

    let (status, head, body) = http_get(port, &format!("/s/{}/media", share.token), "Range: bytes=10-19\r\n");
    assert_eq!(status, 206, "range requests supported: {head}");
    assert_eq!(body, (10..=19u8).collect::<Vec<_>>());

    let (status, _, _) = http_get(port, "/s/nope", "");
    assert_eq!(status, 404);
    store.set_share_enabled(&share.token, false).unwrap();
    let (status, _, _) = http_get(port, &format!("/s/{}", share.token), "");
    assert_eq!(status, 404, "revoked links stop working");
    server.stop();
}
