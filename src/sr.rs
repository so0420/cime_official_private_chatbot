use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::app::Shared;
use crate::db::{self, Db};

// ── YouTube 헬퍼 ──

/// YouTube URL에서 video ID 추출
pub fn extract_video_id(url: &str) -> Option<String> {
    if let Ok(parsed) = url::Url::parse(url) {
        let host = parsed.host_str().unwrap_or("");
        if host.contains("youtube.com") {
            // /watch?v=ID
            for (k, v) in parsed.query_pairs() {
                if k == "v" { return Some(v.to_string()); }
            }
            // /embed/ID, /shorts/ID
            let path = parsed.path();
            let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
            if segs.len() >= 2 && (segs[0] == "embed" || segs[0] == "shorts" || segs[0] == "v") {
                return Some(segs[1].to_string());
            }
        } else if host.contains("youtu.be") {
            let id = parsed.path().trim_start_matches('/');
            if !id.is_empty() { return Some(id.to_string()); }
        }
    }
    None
}

/// YouTube 검색 (키워드 → videoId)
pub async fn search_youtube(query: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .build()
        .map_err(|e| format!("{e}"))?;

    let search_url = format!(
        "https://www.youtube.com/results?search_query={}",
        urlencoding(query)
    );
    let body = client.get(&search_url).send().await
        .map_err(|e| format!("YouTube 검색 실패: {e}"))?
        .text().await.unwrap_or_default();

    // "videoId":"XXXXX" 패턴에서 첫 번째 매치
    let re = regex_lite::Regex::new(r#""videoId":"([^"]{11})""#).unwrap();
    re.captures(&body)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| "검색 결과가 없습니다.".into())
}

/// YouTube 메타데이터 조회 (video ID → title, duration)
pub async fn fetch_video_info(video_id: &str) -> Result<VideoInfo, String> {
    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")
        .build()
        .map_err(|e| format!("{e}"))?;

    let url = format!("https://www.youtube.com/watch?v={}", video_id);
    let html = client.get(&url).send().await
        .map_err(|e| format!("YouTube 페이지 요청 실패: {e}"))?
        .text().await.unwrap_or_default();

    // ytInitialPlayerResponse에서 title, lengthSeconds 추출
    let re = regex_lite::Regex::new(r#"ytInitialPlayerResponse\s*=\s*(\{.*?\});"#).unwrap();
    let json_str = re.captures(&html)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str())
        .ok_or("YouTube 메타데이터를 찾을 수 없습니다.")?;

    let data: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|_| "YouTube JSON 파싱 실패")?;

    let details = data.get("videoDetails").ok_or("videoDetails 없음")?;
    let title = details.get("title").and_then(|v| v.as_str()).unwrap_or("제목 없음").to_string();
    let duration = details.get("lengthSeconds").and_then(|v| v.as_str())
        .and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);

    Ok(VideoInfo { video_id: video_id.to_string(), title, duration })
}

/// URL 또는 키워드로 YouTube 영상 정보 가져오기
pub async fn get_video_info(input: &str) -> Result<VideoInfo, String> {
    let video_id = if input.starts_with("http://") || input.starts_with("https://") {
        extract_video_id(input).ok_or("유효한 YouTube URL이 아닙니다.")?
    } else {
        search_youtube(input).await?
    };
    fetch_video_info(&video_id).await
}

#[derive(Debug, Clone)]
pub struct VideoInfo {
    pub video_id: String,
    pub title: String,
    pub duration: i64,
}

fn urlencoding(s: &str) -> String {
    let mut result = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => result.push(b as char),
            b' ' => result.push('+'),
            _ => result.push_str(&format!("%{:02X}", b)),
        }
    }
    result
}

// ── 오버레이 HTML ──

fn sr_overlay_html(port: u16) -> String {
    format!(r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<style>
*{{margin:0;padding:0}}
body{{background:transparent;overflow:hidden}}
#wrap{{width:100vw;height:100vh;display:none}}
#wrap.show{{display:block}}
#player{{width:100%;height:100%}}
#info{{position:fixed;bottom:10px;left:10px;background:rgba(0,0,0,0.7);color:#fff;padding:8px 16px;border-radius:8px;font:14px 'Malgun Gothic',sans-serif;display:none;z-index:10}}
#info.show{{display:block}}
</style>
</head><body>
<div id="info"><span id="title"></span> <span id="req" style="color:#aaa;font-size:12px"></span></div>
<div id="wrap"><div id="player"></div></div>
<script>
let player=null, playerReady=false, currentId=null, playing=false;
let ignoreEndEvent=false, loadTime=0, everPlayed=false;
const PORT={port};

function skipToNext(){{
  if(!currentId)return;
  currentId=null;
  fetch('http://127.0.0.1:'+PORT+'/api/sr/ended',{{method:'POST'}}).catch(()=>{{}});
  document.getElementById('wrap').classList.remove('show');
  document.getElementById('info').classList.remove('show');
  playing=false;
}}

function onYouTubeIframeAPIReady(){{
  player=new YT.Player('player',{{
    host:'https://www.youtube-nocookie.com',
    width:'100%',height:'100%',
    videoId:'',
    playerVars:{{autoplay:1,controls:0,rel:0,modestbranding:1,origin:window.location.origin}},
    events:{{
      onReady:()=>{{ playerReady=true; }},
      onStateChange:e=>{{
        if(e.data===YT.PlayerState.PLAYING){{ everPlayed=true; ignoreEndEvent=false; }}
        if(e.data===YT.PlayerState.ENDED){{
          if(ignoreEndEvent)return;
          skipToNext();
        }}
      }},
      onError:e=>{{ skipToNext(); }}
    }}
  }});
}}

function loadVideo(videoId){{
  if(!player||!playerReady)return;
  ignoreEndEvent=true;
  setTimeout(()=>{{ ignoreEndEvent=false; }},3000);
  everPlayed=false;
  loadTime=Date.now();
  if(typeof player.stopVideo==='function')player.stopVideo();
  currentId=videoId;
  player.loadVideoById(videoId);
  document.getElementById('wrap').classList.add('show');
  playing=true;
}}

function hidePlayer(){{
  if(player&&typeof player.stopVideo==='function')player.stopVideo();
  document.getElementById('wrap').classList.remove('show');
  document.getElementById('info').classList.remove('show');
  currentId=null;playing=false;
}}

async function poll(){{
  try{{
    const r=await fetch('http://127.0.0.1:'+PORT+'/api/sr/state');
    const d=await r.json();
    if(!playerReady)return;

    if(d.command==='play'&&d.video_id){{
      if(d.video_id!==currentId){{
        loadVideo(d.video_id);
      }}
      document.getElementById('title').textContent=d.title||'';
      document.getElementById('req').textContent=d.requester?'- '+d.requester:'';
      document.getElementById('info').classList.add('show');
    }}else if(d.command==='pause'&&player){{
      player.pauseVideo();
    }}else if(d.command==='resume'){{
      if(currentId&&player){{player.playVideo()}}
      else if(d.video_id&&d.video_id!==currentId){{
        loadVideo(d.video_id);
        document.getElementById('title').textContent=d.title||'';
        document.getElementById('req').textContent=d.requester?'- '+d.requester:'';
        document.getElementById('info').classList.add('show');
      }}
    }}else if(d.command==='stop'||d.command==='idle'){{
      if(playing)hidePlayer();
    }}
    if(player&&typeof player.setVolume==='function'&&d.volume!==undefined&&d.volume!==null)player.setVolume(d.volume);
    // 워치독: 10초 내 재생 안 되면 자동 스킵
    if(playing&&loadTime&&!everPlayed&&(Date.now()-loadTime>10000)){{
      loadTime=0;
      skipToNext();
    }}
  }}catch(e){{}}
}}
setInterval(poll,1000);
</script>
<script src="https://www.youtube.com/iframe_api"></script>
</body></html>"#, port=port)
}

/// 오버레이 HTML 파일을 디스크에 기록
pub fn write_sr_overlay(port: u16) -> std::path::PathBuf {
    let exe = std::env::current_exe().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let dir = exe.parent().unwrap_or(std::path::Path::new("."));
    let path = dir.join("sr_overlay.html");
    let html = sr_overlay_html(port);
    let _ = std::fs::write(&path, html.as_bytes());
    path
}

pub fn sr_overlay_file_url(port: u16) -> String {
    let path = write_sr_overlay(port);
    let abs = path.to_string_lossy().replace('\\', "/");
    format!("file:///{}", abs)
}

// ── 오버레이 API 서버 ──

pub async fn start_sr_server(shared: Shared, db: Db, port: u16) {
    let listener = match tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port)).await {
        Ok(l) => l,
        Err(e) => {
            let mut st = shared.lock().unwrap();
            st.log(&format!("[SR] 서버 시작 실패: {e}"));
            return;
        }
    };

    {
        let mut st = shared.lock().unwrap();
        st.log(&format!("[SR] 오버레이 서버 시작 (포트 {})", port));
    }

    loop {
        {
            let st = shared.lock().unwrap();
            if st.bot_should_stop { break; }
        }
        match tokio::time::timeout(std::time::Duration::from_secs(2), listener.accept()).await {
            Ok(Ok((stream, _))) => {
                let shared = shared.clone();
                let db = db.clone();
                tokio::spawn(async move { handle_sr_request(stream, &shared, &db, port).await; });
            }
            Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }
}

async fn handle_sr_request(mut stream: tokio::net::TcpStream, shared: &Shared, db: &Db, port: u16) {
    let mut buf = vec![0u8; 4096];
    let n = match stream.read(&mut buf).await { Ok(n) if n > 0 => n, _ => return };
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let method = first_line.split_whitespace().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");

    if method == "GET" && (path == "/" || path == "/index.html") {
        // 오버레이 HTML 서빙
        let html = sr_overlay_html(port);
        let resp = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            html.len(), html
        );
        let _ = stream.write_all(resp.as_bytes()).await;
        return;
    }

    let (status, body) = match (method, path) {
        ("GET", "/api/sr/state") => {
            let st = shared.lock().unwrap();
            let json = format!(
                r#"{{"command":"{}","video_id":{},"title":{},"requester":{},"volume":{}}}"#,
                st.sr_command.as_deref().unwrap_or("idle"),
                opt_json_str(&st.sr_current_video_id),
                opt_json_str(&st.sr_current_title),
                opt_json_str(&st.sr_current_requester),
                st.sr_volume,
            );
            ("200 OK", json)
        }
        ("POST", "/api/sr/ended") => {
            // 현재 곡 종료 → 다음 곡 재생
            play_next(shared, db);
            ("200 OK", r#"{"ok":true}"#.to_string())
        }
        _ => ("404 Not Found", r#"{"error":"not found"}"#.to_string()),
    };

    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: application/json\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status, body.len(), body
    );
    let _ = stream.write_all(resp.as_bytes()).await;
}

fn opt_json_str(opt: &Option<String>) -> String {
    match opt {
        Some(s) => format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")),
        None => "null".to_string(),
    }
}

// ── 큐 관리 ──

/// 다음 곡 재생 (큐에서 꺼내서 SharedState에 설정)
pub fn play_next(shared: &Shared, db: &Db) {
    // 현재 곡 삭제
    db::sr_remove_current(db);

    // 다음 곡 조회
    if let Some(song) = db::sr_peek_next(db) {
        db::sr_set_playing(db, song.id);
        let mut st = shared.lock().unwrap();
        st.sr_command = Some("play".to_string());
        st.sr_current_video_id = Some(song.video_id.clone());
        st.sr_current_title = Some(song.video_title.clone());
        st.sr_current_requester = Some(song.requester.clone());
        st.sr_queue_changed = true;
        st.log(&format!("[SR] 재생: {} ({})", song.video_title, song.requester));
    } else {
        let mut st = shared.lock().unwrap();
        st.sr_command = Some("stop".to_string());
        st.sr_current_video_id = None;
        st.sr_current_title = None;
        st.sr_current_requester = None;
        st.sr_queue_changed = true;
        st.log("[SR] 대기열이 비어있습니다.");
    }
}

/// 곡 추가 후 아무것도 재생 중이 아니면 바로 재생
pub fn add_and_maybe_play(shared: &Shared, db: &Db, video_id: &str, title: &str, duration: i64, requester: &str) {
    db::sr_add(db, video_id, title, duration, requester);
    { shared.lock().unwrap().sr_queue_changed = true; }
    let is_idle = {
        let st = shared.lock().unwrap();
        st.sr_command.as_deref() != Some("play") && st.sr_command.as_deref() != Some("pause")
    };
    if is_idle {
        play_next(shared, db);
    }
}

/// 앱 시작 시 DB에서 대기열 상태 복원 (재생하지 않음, 정보만 표시)
pub fn restore_queue(shared: &Shared, db: &Db) {
    let conn = db.lock().unwrap();
    let current = conn.query_row(
        "SELECT video_id, video_title, requester FROM sr_queue WHERE status='playing' LIMIT 1",
        [],
        |r| Ok((r.get::<_,String>(0)?, r.get::<_,String>(1)?, r.get::<_,String>(2)?)),
    );
    drop(conn);

    if let Ok((vid, title, req)) = current {
        let mut st = shared.lock().unwrap();
        // 중지 상태로 복원 (자동 재생 안 함)
        st.sr_command = Some("stop".to_string());
        st.sr_current_video_id = Some(vid);
        st.sr_current_title = Some(title.clone());
        st.sr_current_requester = Some(req.clone());
        st.log(&format!("[SR] 대기열 복원 (중지 상태): {} ({})", title, req));
    }
}

pub fn format_duration(secs: i64) -> String {
    let m = secs / 60;
    let s = secs % 60;
    format!("{}:{:02}", m, s)
}
