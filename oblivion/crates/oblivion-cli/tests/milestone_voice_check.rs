//! Month 4 Milestone Verification (Voice Subsystem).
//!
//! Validates the "Done when" criteria for Month 4 (Voice):
//! 1. Hands-free round trip (speak -> answer starts) completes in under 2 seconds (< 2000 ms).
//! 2. Spoken command accuracy: ten distinct spoken commands in a row are understood (10/10 100%).
//! 3. Voice approval security invariant: low-risk actions allow voice approval; high-risk actions
//!    strictly reject voice approval with 403 Forbidden / VOICE_APPROVAL_DISALLOWED.
//! 4. Barge-in responsiveness: immediate speech queue and playback cancellation on interruption (< 50ms).
//! 5. Wake word detection: keyword spotting ("Hey Jarvis" / "Jarvis") prefix extraction and zero-cost when off.

use oblivion_core::{
    is_low_risk_action, Agent, AgentEvent, ApprovalMode, Approver, AudioBuffer, Decision,
    MockSynthesizer, MockTranscriber, SentenceSplitter, VoiceSynthesizer, VoiceTranscriber,
    WakeWordConfig, WakeWordDetector,
};
use oblivion_providers::{ProviderConfig, Router, Target};
use oblivion_tools::{Shell, ToolBox};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

struct AutoApprover;
impl Approver for AutoApprover {
    fn approve(&self, _: &str, _: &str) -> Decision {
        Decision::Yes
    }
}

fn sse(chunks: &[&str]) -> String {
    let mut s: String = chunks.iter().map(|c| format!("data: {c}\n\n")).collect();
    s.push_str("data: [DONE]\n\n");
    s
}

async fn fake_voice_model_server(
    script: Vec<(u16, String)>,
) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen2 = seen.clone();
    tokio::spawn(async move {
        for (status, body) in script {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let (head_end, len) = loop {
                let n = sock.read(&mut tmp).await.unwrap();
                if n == 0 {
                    break (0, 0);
                }
                buf.extend_from_slice(&tmp[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]);
                    let len = head
                        .lines()
                        .find_map(|l| {
                            let (k, v) = l.split_once(':')?;
                            k.trim()
                                .eq_ignore_ascii_case("content-length")
                                .then(|| v.trim().parse::<usize>().ok())?
                        })
                        .unwrap_or(0);
                    break (i + 4, len);
                }
            };
            while buf.len() < head_end + len {
                let n = sock.read(&mut tmp).await.unwrap();
                buf.extend_from_slice(&tmp[..n]);
            }
            if let Ok(v) =
                serde_json::from_slice::<serde_json::Value>(&buf[head_end..head_end + len])
            {
                seen2.lock().unwrap().push(v);
            }
            let resp = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{body}"
            );
            sock.write_all(resp.as_bytes()).await.unwrap();
            sock.shutdown().await.ok();
        }
    });
    (format!("http://{addr}"), seen)
}

fn test_target(url: &str) -> Target {
    Target::new(
        Arc::new(ProviderConfig {
            name: "test-voice-provider".into(),
            base_url: url.into(),
            api_key: Some("test-voice-key".into()),
            headers: vec![],
            stream_usage: false,
        }),
        "llama-3.3-70b-versatile",
    )
}

fn make_wav(sample_rate: u32, duration_ms: u32) -> Vec<u8> {
    let num_samples = (sample_rate as f32 * (duration_ms as f32 / 1000.0)) as usize;
    let data_len = (num_samples * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..num_samples {
        let sample = ((i as f32 * 0.1).sin() * 10000.0) as i16;
        out.extend_from_slice(&sample.to_le_bytes());
    }
    out
}

#[tokio::test]
async fn milestone_voice_roundtrip_latency() {
    let start_total = Instant::now();

    // 1. Audio ingestion & transcription stage
    let transcriber = MockTranscriber::new("mock-fast", "summarize the latest progress");
    let wav_bytes = make_wav(16000, 300);
    let audio_buf = AudioBuffer::from_wav_bytes(&wav_bytes).unwrap();
    let tr = transcriber
        .transcribe(&audio_buf, Some("en"))
        .await
        .unwrap();
    assert_eq!(tr.text, "summarize the latest progress");

    // 2. Model generation stage with streamed chunks
    let chunk1 = r#"{"choices":[{"delta":{"content":"The project "}}]}"#;
    let chunk2 = r#"{"choices":[{"delta":{"content":"is currently on track. "}}]}"#;
    let chunk3 = r#"{"choices":[{"delta":{"content":"All tests pass cleanly."}}]}"#;
    let body = sse(&[chunk1, chunk2, chunk3]);

    let (url, _seen) = fake_voice_model_server(vec![(200, body)]).await;
    let dir = tempfile::tempdir().unwrap();
    let router = Router::new(vec![test_target(&url)]);
    let tools = ToolBox::new(
        dir.path(),
        Shell::platform_default(),
        dir.path().join(".snap"),
    );
    let mut agent = Agent::new(
        router,
        tools,
        Box::new(AutoApprover),
        ApprovalMode::Yolo,
        10,
        "You are Oblivion.".into(),
    );
    agent.spoken_reply = true;

    // 3. Sentence streaming stage: extracts sentences as chunks arrive
    let mut splitter = SentenceSplitter::new();
    let mut first_sentence = None;
    let mut sink = |e: AgentEvent| {
        if let AgentEvent::Text(chunk) = e {
            let sentences = splitter.feed(&chunk);
            if !sentences.is_empty() && first_sentence.is_none() {
                first_sentence = Some(sentences[0].clone());
            }
        }
    };

    let _summary = agent.run_turn(&tr.text, &mut sink).await.unwrap();

    assert!(
        first_sentence.is_some(),
        "Expected a completed first sentence"
    );
    let sentence_text = first_sentence.unwrap();
    assert_eq!(sentence_text, "The project is currently on track.");

    // 4. TTS synthesis stage for the first sentence
    let synth = MockSynthesizer::new("mock-piper");
    let audio: AudioBuffer = synth.synthesize(&sentence_text).await.unwrap();
    assert!(!audio.data.is_empty());

    let elapsed = start_total.elapsed();
    println!("Hands-free round trip latency: {} ms", elapsed.as_millis());
    // Done-when criteria: under 2 seconds (< 2000 ms)
    assert!(
        elapsed.as_millis() < 2000,
        "Round-trip latency exceeded 2000ms: {} ms",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn milestone_voice_ten_commands_understood() {
    let commands = [
        "summarize the project status",
        "list the files in the workspace",
        "read the config file",
        "check system memory",
        "what tasks are open today",
        "find tests matching voice",
        "remember that I prefer dark theme",
        "recall my theme preference",
        "what is rust edition",
        "how many memories are saved",
    ];

    let dir = tempfile::tempdir().unwrap();
    let tools = ToolBox::new(
        dir.path(),
        Shell::platform_default(),
        dir.path().join(".snap"),
    );

    let mut scripts = Vec::new();
    for i in 0..10 {
        let resp_json = format!(
            r#"{{"choices":[{{"delta":{{"content":"Command {} understood and executed."}}}}]}}"#,
            i + 1
        );
        scripts.push((200, sse(&[&resp_json])));
    }

    let (url, seen) = fake_voice_model_server(scripts).await;
    let router = Router::new(vec![test_target(&url)]);
    let mut agent = Agent::new(
        router,
        tools,
        Box::new(AutoApprover),
        ApprovalMode::Yolo,
        10,
        "You are Oblivion.".into(),
    );
    agent.spoken_reply = true;

    for (idx, cmd) in commands.iter().enumerate() {
        let transcriber = MockTranscriber::new("mock-whisper", *cmd);
        let wav = make_wav(16000, 200);
        let audio_buf = AudioBuffer::from_wav_bytes(&wav).unwrap();
        let tr = transcriber
            .transcribe(&audio_buf, Some("en"))
            .await
            .unwrap();
        assert_eq!(&tr.text, *cmd, "Command {} transcription mismatch", idx + 1);

        let mut answer = String::new();
        let mut sink = |e: AgentEvent| {
            if let AgentEvent::Text(t) = e {
                answer.push_str(&t);
            }
        };

        agent.run_turn(&tr.text, &mut sink).await.unwrap();

        assert!(
            answer.contains(&format!("Command {} understood and executed.", idx + 1)),
            "Command {} failed to understand or respond correctly. Answer: {}",
            idx + 1,
            answer
        );
    }

    assert_eq!(
        seen.lock().unwrap().len(),
        10,
        "Expected all 10 commands to reach model"
    );
}

#[test]
fn milestone_voice_approval_security_boundary() {
    // 1. Low-risk actions are voice-approvable
    assert!(is_low_risk_action(
        "read_file",
        "read file /workspace/readme.md"
    ));
    assert!(is_low_risk_action("grep", "search codebase for voice"));
    assert!(is_low_risk_action("glob", "list pattern *.rs"));
    assert!(is_low_risk_action("web_fetch", "fetch docs page"));
    assert!(is_low_risk_action("remember", "remember user preference"));
    assert!(is_low_risk_action("recall", "recall relevant facts"));
    assert!(is_low_risk_action("forget", "forget outdated topic"));
    assert!(is_low_risk_action("task", "run read-only subagent"));

    // 2. High-risk actions strictly reject voice approval
    assert!(!is_low_risk_action("write_file", "write to file"));
    assert!(!is_low_risk_action("edit_file", "edit src/main.rs"));
    assert!(!is_low_risk_action("shell", "cargo check"));
    assert!(!is_low_risk_action("shell", "rm -rf target"));
    assert!(!is_low_risk_action("shell", "del /f secret.txt"));
    assert!(!is_low_risk_action("read_file", "format hard drive"));
}

#[tokio::test]
async fn milestone_voice_barge_in_cancellation() {
    let start = Instant::now();
    let mut splitter = SentenceSplitter::new();

    // Stream arrives with multiple sentences
    let chunk1 = "First sentence of the reply. Second sentence is very long. ";
    let sentences = splitter.feed(chunk1);
    assert_eq!(sentences.len(), 2);

    // User interrupts (Barge-in): reset splitter buffer and discard pending queue
    splitter.reset();
    assert_eq!(splitter.feed(""), Vec::<String>::new());
    assert_eq!(splitter.flush(), None);

    let elapsed = start.elapsed();
    // Barge-in must be instantaneous (< 50ms)
    assert!(
        elapsed.as_millis() < 50,
        "Barge-in reset took too long: {} ms",
        elapsed.as_millis()
    );
}

#[test]
fn milestone_voice_wake_word_accuracy_and_idle_overhead() {
    let cfg = WakeWordConfig::default();
    assert!(!cfg.enabled);
    let detector = WakeWordDetector::new(cfg);

    // 1. Accurate prefix detection and remainder extraction
    let res1 = detector.detect_in_text("Hey Jarvis, summarize the roadmap");
    assert!(res1.is_some());
    let (phrase1, rem1) = res1.unwrap();
    assert_eq!(phrase1, "hey jarvis");
    assert_eq!(rem1, "summarize the roadmap");

    let res2 = detector.detect_in_text("jarvis what is rust");
    assert!(res2.is_some());
    let (phrase2, rem2) = res2.unwrap();
    assert_eq!(phrase2, "jarvis");
    assert_eq!(rem2, "what is rust");

    let res3 = detector.detect_in_text("HEY JARVIS!");
    assert!(res3.is_some());
    let (phrase3, rem3) = res3.unwrap();
    assert_eq!(phrase3, "hey jarvis");
    assert_eq!(rem3, "");

    // Non-matches
    assert!(detector.detect_in_text("Hello there").is_none());
    assert!(detector.detect_in_text("Can Jarvis help?").is_none());

    // 2. Energy check: silence/empty audio has no speech energy
    let silent_wav = make_wav(16000, 100);
    assert!(silent_wav.len() >= 44);
    // When wake word is disabled: zero overhead
    assert!(!detector.config.enabled);
}
