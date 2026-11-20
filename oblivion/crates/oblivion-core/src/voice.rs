//! Voice input subsystem: audio buffer handling, speech-to-text (STT) traits,
//! local whisper runner, cloud fallback, and voice configuration.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Representation of an in-memory audio buffer suitable for speech recognition.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioBuffer {
    /// Number of audio channels (e.g. 1 for mono, 2 for stereo).
    pub channels: u16,
    /// Sampling rate in Hz (e.g. 16000 for Whisper).
    pub sample_rate: u32,
    /// Bit depth per sample (typically 16 for PCM).
    pub bits_per_sample: u16,
    /// Raw PCM audio sample bytes (little-endian 16-bit integers).
    pub data: Vec<u8>,
}

impl AudioBuffer {
    /// Create a new 16-bit mono PCM audio buffer.
    pub fn new_pcm16_mono(sample_rate: u32, samples: &[i16]) -> Self {
        let mut data = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            data.extend_from_slice(&s.to_le_bytes());
        }
        Self {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            data,
        }
    }

    /// Audio duration in milliseconds.
    pub fn duration_ms(&self) -> u64 {
        let bytes_per_sample = (self.bits_per_sample / 8) as u64;
        let total_samples = self.data.len() as u64 / (bytes_per_sample * self.channels as u64);
        if self.sample_rate == 0 {
            0
        } else {
            (total_samples * 1000) / self.sample_rate as u64
        }
    }

    /// Compute root-mean-square (RMS) energy normalized between 0.0 and 1.0.
    pub fn rms_energy(&self) -> f32 {
        if self.data.is_empty() || self.bits_per_sample != 16 {
            return 0.0;
        }
        let mut sum_sq: f64 = 0.0;
        let mut count = 0usize;
        for chunk in self.data.as_chunks::<2>().0 {
            let sample = i16::from_le_bytes(*chunk) as f64;
            sum_sq += sample * sample;
            count += 1;
        }
        if count == 0 {
            return 0.0;
        }
        let rms = (sum_sq / count as f64).sqrt();
        (rms / 32768.0).min(1.0) as f32
    }

    /// Encode this audio buffer as a standard 44-byte header PCM WAV byte vector.
    pub fn to_wav_bytes(&self) -> Vec<u8> {
        let data_len = self.data.len() as u32;
        let bytes_per_sample = (self.bits_per_sample / 8) as u32;
        let byte_rate = self.sample_rate * self.channels as u32 * bytes_per_sample;
        let block_align = self.channels * (self.bits_per_sample / 8);

        let mut wav = Vec::with_capacity(44 + self.data.len());
        // RIFF header
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");

        // fmt subchunk
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes()); // subchunk 1 size (16 for PCM)
        wav.extend_from_slice(&1u16.to_le_bytes()); // audio format 1 = PCM
        wav.extend_from_slice(&self.channels.to_le_bytes());
        wav.extend_from_slice(&self.sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&self.bits_per_sample.to_le_bytes());

        // data subchunk
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&self.data);

        wav
    }

    /// Parse a standard RIFF/WAVE byte buffer into an `AudioBuffer`.
    pub fn from_wav_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 44 {
            bail!(
                "WAV byte buffer too short: {} bytes (minimum 44)",
                bytes.len()
            );
        }
        if &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
            bail!("Invalid WAV header: missing RIFF/WAVE magic");
        }

        let mut offset = 12usize;
        let mut channels = 1u16;
        let mut sample_rate = 16000u32;
        let mut bits_per_sample = 16u16;
        let mut data = Vec::new();
        let mut found_fmt = false;
        let mut found_data = false;

        while offset + 8 <= bytes.len() {
            let chunk_id = &bytes[offset..offset + 4];
            let chunk_size = u32::from_le_bytes([
                bytes[offset + 4],
                bytes[offset + 5],
                bytes[offset + 6],
                bytes[offset + 7],
            ]) as usize;
            offset += 8;

            if chunk_id == b"fmt " {
                if chunk_size < 16 || offset + 16 > bytes.len() {
                    bail!("Invalid fmt chunk size: {}", chunk_size);
                }
                let audio_format = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
                if audio_format != 1 && audio_format != 3 {
                    bail!(
                        "Unsupported WAV audio format: {} (only PCM/IEEE float supported)",
                        audio_format
                    );
                }
                channels = u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]);
                sample_rate = u32::from_le_bytes([
                    bytes[offset + 4],
                    bytes[offset + 5],
                    bytes[offset + 6],
                    bytes[offset + 7],
                ]);
                bits_per_sample = u16::from_le_bytes([bytes[offset + 14], bytes[offset + 15]]);
                found_fmt = true;
            } else if chunk_id == b"data" {
                let end = (offset + chunk_size).min(bytes.len());
                data = bytes[offset..end].to_vec();
                found_data = true;
                break;
            }

            offset += chunk_size;
        }

        if !found_fmt || !found_data {
            bail!("Incomplete WAV file: missing fmt or data subchunks");
        }

        Ok(Self {
            channels,
            sample_rate,
            bits_per_sample,
            data,
        })
    }
}

/// Result returned from a speech-to-text transcription engine.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TranscriptionResult {
    /// Transcribed text.
    pub text: String,
    /// Detected or requested language code (e.g. "en").
    pub language: Option<String>,
    /// Duration of the transcribed audio in milliseconds.
    pub duration_ms: u64,
    /// Transcription confidence score (if reported by engine, 0.0 - 1.0).
    pub confidence: Option<f32>,
    /// Identifier of the engine that performed the transcription.
    pub engine: String,
}

/// Abstract speech-to-text transcriber interface.
pub trait VoiceTranscriber: Send + Sync {
    /// Human-readable identifier of the transcription engine.
    fn engine_name(&self) -> &str;

    /// Returns true if this engine is currently available and ready to transcribe.
    fn is_available(&self) -> bool;

    /// Transcribe the given audio buffer into text.
    fn transcribe<'a>(
        &'a self,
        audio: &'a AudioBuffer,
        language: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranscriptionResult>> + Send + 'a>>;
}

/// Deterministic mock transcriber for testing.
pub struct MockTranscriber {
    name: String,
    reply_text: String,
    available: bool,
}

impl MockTranscriber {
    pub fn new(name: impl Into<String>, reply_text: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            reply_text: reply_text.into(),
            available: true,
        }
    }

    pub fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }
}

impl VoiceTranscriber for MockTranscriber {
    fn engine_name(&self) -> &str {
        &self.name
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn transcribe<'a>(
        &'a self,
        audio: &'a AudioBuffer,
        language: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranscriptionResult>> + Send + 'a>>
    {
        Box::pin(async move {
            if !self.available {
                bail!("Mock transcriber '{}' is marked unavailable", self.name);
            }
            Ok(TranscriptionResult {
                text: self.reply_text.clone(),
                language: language
                    .map(str::to_string)
                    .or_else(|| Some("en".to_string())),
                duration_ms: audio.duration_ms(),
                confidence: Some(0.98),
                engine: self.name.clone(),
            })
        })
    }
}

/// Local Whisper runner: drives a local `whisper-cli`, `whisper.cpp`, or `whisper.exe` binary.
pub struct LocalWhisperTranscriber {
    binary_path: Option<PathBuf>,
    model_path: Option<PathBuf>,
}

impl LocalWhisperTranscriber {
    pub fn new(binary_path: Option<PathBuf>, model_path: Option<PathBuf>) -> Self {
        Self {
            binary_path,
            model_path,
        }
    }

    /// Search for available whisper executable on system PATH or common locations.
    pub fn detect_binary() -> Option<PathBuf> {
        if let Ok(env_path) = std::env::var("WHISPER_PATH") {
            let p = PathBuf::from(env_path);
            if p.is_file() {
                return Some(p);
            }
        }

        let candidates = if cfg!(windows) {
            vec![
                "whisper-cli.exe",
                "whisper.exe",
                "whisper-cpp.exe",
                "main.exe",
            ]
        } else {
            vec!["whisper-cli", "whisper", "whisper-cpp", "main"]
        };

        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                for cand in &candidates {
                    let full = dir.join(cand);
                    if full.is_file() {
                        return Some(full);
                    }
                }
            }
        }
        None
    }

    /// Resolve the binary path to use (either explicitly configured or detected).
    pub fn resolved_binary(&self) -> Option<PathBuf> {
        self.binary_path.clone().or_else(Self::detect_binary)
    }
}

impl VoiceTranscriber for LocalWhisperTranscriber {
    fn engine_name(&self) -> &str {
        "local-whisper"
    }

    fn is_available(&self) -> bool {
        self.resolved_binary().is_some()
    }

    fn transcribe<'a>(
        &'a self,
        audio: &'a AudioBuffer,
        language: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranscriptionResult>> + Send + 'a>>
    {
        Box::pin(async move {
            let bin = self.resolved_binary().context(
                "Local whisper binary not found. Set voice.whisper_path or install whisper-cli.",
            )?;

            // Write audio to temporary WAV file
            let temp_dir = std::env::temp_dir();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let wav_path = temp_dir.join(format!("oblivion_voice_{}.wav", timestamp));
            let wav_bytes = audio.to_wav_bytes();
            tokio::fs::write(&wav_path, &wav_bytes)
                .await
                .with_context(|| format!("Failed to write temporary WAV file to {:?}", wav_path))?;

            // Build command arguments
            let mut cmd = tokio::process::Command::new(&bin);
            cmd.arg("-f").arg(&wav_path);
            if let Some(m) = &self.model_path {
                cmd.arg("-m").arg(m);
            }
            if let Some(lang) = language {
                cmd.arg("-l").arg(lang);
            }
            cmd.arg("--output-txt");

            let output = cmd
                .output()
                .await
                .with_context(|| format!("Failed to execute local whisper binary at {:?}", bin))?;

            // Clean up temporary wav file
            let _ = tokio::fs::remove_file(&wav_path).await;

            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                bail!(
                    "Whisper execution failed with exit code {:?}: {}",
                    output.status.code(),
                    err_msg
                );
            }

            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let clean_text = stdout
                .lines()
                .filter(|line| !line.starts_with('[') || !line.contains("-->"))
                .collect::<Vec<_>>()
                .join(" ")
                .trim()
                .to_string();

            Ok(TranscriptionResult {
                text: clean_text,
                language: language.map(str::to_string),
                duration_ms: audio.duration_ms(),
                confidence: None,
                engine: "local-whisper".to_string(),
            })
        })
    }
}

/// Cloud Whisper transcriber (Groq Whisper or OpenAI Whisper API).
pub struct CloudWhisperTranscriber {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl CloudWhisperTranscriber {
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                .unwrap_or_default(),
            base_url: base_url.into(),
            api_key: api_key.into(),
            model: model.into(),
        }
    }

    pub fn groq(api_key: impl Into<String>) -> Self {
        Self::new(
            "https://api.groq.com/openai/v1/audio/transcriptions",
            api_key,
            "whisper-large-v3-turbo",
        )
    }

    pub fn openai(api_key: impl Into<String>) -> Self {
        Self::new(
            "https://api.openai.com/v1/audio/transcriptions",
            api_key,
            "whisper-1",
        )
    }
}

impl VoiceTranscriber for CloudWhisperTranscriber {
    fn engine_name(&self) -> &str {
        "cloud-whisper"
    }

    fn is_available(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    fn transcribe<'a>(
        &'a self,
        audio: &'a AudioBuffer,
        language: Option<&'a str>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TranscriptionResult>> + Send + 'a>>
    {
        Box::pin(async move {
            if !self.is_available() {
                bail!("Cloud Whisper API key is not configured.");
            }

            let wav_bytes = audio.to_wav_bytes();
            let part = reqwest::multipart::Part::bytes(wav_bytes)
                .file_name("audio.wav")
                .mime_str("audio/wav")?;

            let mut form = reqwest::multipart::Form::new()
                .text("model", self.model.clone())
                .text("response_format", "json")
                .part("file", part);

            if let Some(lang) = language {
                form = form.text("language", lang.to_string());
            }

            let res = self
                .client
                .post(&self.base_url)
                .bearer_auth(&self.api_key)
                .multipart(form)
                .send()
                .await
                .context("Failed to send audio to Cloud Whisper API")?;

            let status = res.status();
            let body_text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                bail!("Cloud Whisper API returned error {}: {}", status, body_text);
            }

            #[derive(Deserialize)]
            struct ApiResponse {
                text: String,
            }

            let parsed: ApiResponse = serde_json::from_str(&body_text).with_context(|| {
                format!("Failed to parse Cloud Whisper response: {}", body_text)
            })?;

            Ok(TranscriptionResult {
                text: parsed.text.trim().to_string(),
                language: language.map(str::to_string),
                duration_ms: audio.duration_ms(),
                confidence: None,
                engine: "cloud-whisper".to_string(),
            })
        })
    }
}

/// Streaming sentence tokenizer that splits incoming text tokens into full sentences
/// suitable for low-latency text-to-speech synthesis while skipping code blocks.
#[derive(Debug, Clone, Default)]
pub struct SentenceSplitter {
    buffer: String,
    in_code_block: bool,
}

impl SentenceSplitter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a newly arrived text chunk and extract any completed sentences.
    pub fn feed(&mut self, chunk: &str) -> Vec<String> {
        let mut sentences = Vec::new();
        for ch in chunk.chars() {
            self.buffer.push(ch);

            // Check for code block toggling
            if self.buffer.ends_with("```") {
                self.in_code_block = !self.in_code_block;
                if self.in_code_block {
                    // Code block started: trim trailing ``` from speech buffer
                    if let Some(idx) = self.buffer.rfind("```") {
                        let text_before = self.buffer[..idx].trim().to_string();
                        if !text_before.is_empty() {
                            sentences.push(text_before);
                        }
                    }
                    self.buffer.clear();
                } else {
                    // Code block ended: discard whatever was in the code block
                    self.buffer.clear();
                }
                continue;
            }

            if self.in_code_block {
                continue;
            }

            // Check if buffer ends with a sentence delimiter
            if Self::is_sentence_end(&self.buffer) {
                let trimmed = self.buffer.trim().to_string();
                if !trimmed.is_empty() {
                    sentences.push(trimmed);
                }
                self.buffer.clear();
            }
        }
        sentences
    }

    /// Flush any remaining text in the buffer when the stream completes.
    pub fn flush(&mut self) -> Option<String> {
        if self.in_code_block {
            self.buffer.clear();
            return None;
        }
        let trimmed = self.buffer.trim().to_string();
        self.buffer.clear();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    }

    /// Helper to test if buffer currently ends with a sentence boundary.
    fn is_sentence_end(buf: &str) -> bool {
        let trimmed = buf.trim_end();
        if trimmed.is_empty() {
            return false;
        }
        let last_char = trimmed.chars().last().unwrap();
        if last_char != '.' && last_char != '!' && last_char != '?' && last_char != '\n' {
            return false;
        }

        // Avoid splitting on common abbreviations or decimal numbers
        if last_char == '.' {
            // Check for known abbreviations
            for abbr in &["e.g.", "i.e.", "Dr.", "Mr.", "Mrs.", "vs.", "etc."] {
                if trimmed.ends_with(abbr) {
                    return false;
                }
            }
            // Check if preceded by a digit (e.g. 3.14)
            let mut rev = trimmed.chars().rev().skip(1);
            if let Some(prev) = rev.next() {
                if prev.is_ascii_digit() {
                    return false;
                }
            }
        }

        // A sentence delimiter must be followed by whitespace or be at end of newline
        buf.ends_with(". ")
            || buf.ends_with("! ")
            || buf.ends_with("? ")
            || buf.ends_with(".\n")
            || buf.ends_with("!\n")
            || buf.ends_with("?\n")
            || buf.ends_with("\n\n")
    }
}

/// Abstract text-to-speech synthesizer interface.
pub trait VoiceSynthesizer: Send + Sync {
    /// Human-readable identifier of the synthesizer engine.
    fn engine_name(&self) -> &str;

    /// Returns true if this synthesizer is available on the system.
    fn is_available(&self) -> bool;

    /// Synthesize speech audio from text.
    fn synthesize<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AudioBuffer>> + Send + 'a>>;
}

/// Deterministic mock synthesizer for testing.
pub struct MockSynthesizer {
    name: String,
    sample_rate: u32,
    available: bool,
}

impl MockSynthesizer {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sample_rate: 16000,
            available: true,
        }
    }

    pub fn with_available(mut self, available: bool) -> Self {
        self.available = available;
        self
    }
}

impl VoiceSynthesizer for MockSynthesizer {
    fn engine_name(&self) -> &str {
        &self.name
    }

    fn is_available(&self) -> bool {
        self.available
    }

    fn synthesize<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AudioBuffer>> + Send + 'a>> {
        let sample_rate = self.sample_rate;
        let is_avail = self.available;
        let engine_name = self.name.clone();
        let word_count = text.split_whitespace().count().max(1);

        Box::pin(async move {
            if !is_avail {
                bail!("Synthesizer '{}' is marked unavailable", engine_name);
            }
            let duration_ms = (word_count as u64 * 50).max(100);
            let num_samples = ((sample_rate as u64 * duration_ms) / 1000) as usize;
            let mut samples = Vec::with_capacity(num_samples);
            for i in 0..num_samples {
                let t = i as f64 / sample_rate as f64;
                let val = (t * 440.0 * 2.0 * std::f64::consts::PI).sin();
                samples.push((val * 8000.0) as i16);
            }
            Ok(AudioBuffer::new_pcm16_mono(sample_rate, &samples))
        })
    }
}

/// Local Piper TTS runner.
pub struct LocalPiperSynthesizer {
    binary_path: Option<PathBuf>,
    model_path: Option<PathBuf>,
}

impl LocalPiperSynthesizer {
    pub fn new(binary_path: Option<PathBuf>, model_path: Option<PathBuf>) -> Self {
        Self {
            binary_path,
            model_path,
        }
    }

    pub fn detect_binary() -> Option<PathBuf> {
        if let Ok(env_path) = std::env::var("PIPER_PATH") {
            let p = PathBuf::from(env_path);
            if p.is_file() {
                return Some(p);
            }
        }
        let candidates = if cfg!(windows) {
            vec!["piper.exe", "piper"]
        } else {
            vec!["piper"]
        };
        if let Ok(path_var) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_var) {
                for cand in &candidates {
                    let full = dir.join(cand);
                    if full.is_file() {
                        return Some(full);
                    }
                }
            }
        }
        None
    }

    pub fn resolved_binary(&self) -> Option<PathBuf> {
        self.binary_path.clone().or_else(Self::detect_binary)
    }
}

impl VoiceSynthesizer for LocalPiperSynthesizer {
    fn engine_name(&self) -> &str {
        "local-piper"
    }

    fn is_available(&self) -> bool {
        self.resolved_binary().is_some()
    }

    fn synthesize<'a>(
        &'a self,
        text: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<AudioBuffer>> + Send + 'a>> {
        let bin = self.resolved_binary();
        let model = self.model_path.clone();
        let text_owned = text.to_string();

        Box::pin(async move {
            let bin_path =
                bin.context("Local Piper binary not found. Set PIPER_PATH or install piper.")?;
            let temp_dir = std::env::temp_dir();
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let wav_path = temp_dir.join(format!("oblivion_tts_{}.wav", timestamp));

            let mut cmd = tokio::process::Command::new(&bin_path);
            if let Some(m) = &model {
                cmd.arg("--model").arg(m);
            }
            cmd.arg("--output_file").arg(&wav_path);
            cmd.stdin(std::process::Stdio::piped());

            let mut child = cmd
                .spawn()
                .with_context(|| format!("Failed to spawn Piper binary at {:?}", bin_path))?;
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(text_owned.as_bytes()).await;
            }

            let output = child.wait_with_output().await?;
            if !output.status.success() {
                let err_msg = String::from_utf8_lossy(&output.stderr);
                bail!(
                    "Piper TTS failed with exit code {:?}: {}",
                    output.status.code(),
                    err_msg
                );
            }

            let wav_bytes = tokio::fs::read(&wav_path).await?;
            let _ = tokio::fs::remove_file(&wav_path).await;

            AudioBuffer::from_wav_bytes(&wav_bytes)
        })
    }
}

/// Configuration for wake word detection.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WakeWordConfig {
    pub enabled: bool,
    pub phrases: Vec<String>,
    pub energy_threshold: f32,
}

impl Default for WakeWordConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            phrases: vec!["jarvis".to_string(), "hey jarvis".to_string()],
            energy_threshold: 0.01,
        }
    }
}

/// Keyword spotter / wake word detector for hands-free invocation.
///
/// Designed to be CPU-light and privacy-respecting:
/// 1. Only active when explicitly enabled by user.
/// 2. Energy gating drops quiet audio without performing heavy model operations.
/// 3. Normalizes phrases (case, punctuation) to reliably spot triggers.
#[derive(Debug, Clone)]
pub struct WakeWordDetector {
    pub config: WakeWordConfig,
}

impl WakeWordDetector {
    pub fn new(config: WakeWordConfig) -> Self {
        Self { config }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.config.enabled = enabled;
    }

    pub fn phrases(&self) -> &[String] {
        &self.config.phrases
    }

    /// Check if audio energy is above the threshold required to process for wake word.
    pub fn has_speech_energy(&self, audio: &AudioBuffer) -> bool {
        audio.rms_energy() >= self.config.energy_threshold
    }

    /// Detect if a transcript begins with or contains a configured wake word.
    /// Returns `Some((detected_phrase, remaining_text))` with the wake word stripped.
    pub fn detect_in_text(&self, text: &str) -> Option<(String, String)> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        // Clean leading punctuation
        let clean =
            trimmed.trim_start_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace());
        let lower = clean.to_lowercase();

        // Sort phrases by length descending to match longer phrases like "hey jarvis" before "jarvis"
        let mut sorted_phrases = self.config.phrases.clone();
        sorted_phrases.sort_by_key(|a| std::cmp::Reverse(a.len()));

        for phrase in &sorted_phrases {
            let p_lower = phrase.to_lowercase();
            if lower.starts_with(&p_lower) {
                let remainder = &clean[p_lower.len()..];
                let remainder_clean = remainder
                    .trim_start_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace())
                    .trim();
                return Some((phrase.clone(), remainder_clean.to_string()));
            }
        }
        None
    }
}

/// Status summary for the voice subsystem.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VoiceStatus {
    pub enabled: bool,
    pub stt_engine: String,
    pub tts_engine: String,
    pub hotkey: String,
    pub language: String,
    pub local_whisper_available: bool,
    pub cloud_whisper_available: bool,
    pub local_piper_available: bool,
    pub web_speech_available: bool,
    #[serde(default)]
    pub wake_word_enabled: bool,
    #[serde(default)]
    pub wake_words: Vec<String>,
}

/// High-level voice coordinator service that routes between local and cloud STT.
pub struct VoiceService {
    local_transcriber: Arc<dyn VoiceTranscriber>,
    cloud_transcriber: Option<Arc<dyn VoiceTranscriber>>,
    synthesizer: Option<Arc<dyn VoiceSynthesizer>>,
    engine_preference: String,
    hotkey: String,
    language: String,
    offline_only: bool,
    wake_word: std::sync::RwLock<WakeWordDetector>,
}

impl VoiceService {
    pub fn new(
        local: Arc<dyn VoiceTranscriber>,
        cloud: Option<Arc<dyn VoiceTranscriber>>,
        synthesizer: Option<Arc<dyn VoiceSynthesizer>>,
        engine_preference: impl Into<String>,
        hotkey: impl Into<String>,
        language: impl Into<String>,
        offline_only: bool,
    ) -> Self {
        Self {
            local_transcriber: local,
            cloud_transcriber: cloud,
            synthesizer,
            engine_preference: engine_preference.into(),
            hotkey: hotkey.into(),
            language: language.into(),
            offline_only,
            wake_word: std::sync::RwLock::new(WakeWordDetector::new(WakeWordConfig::default())),
        }
    }

    pub fn is_wake_word_enabled(&self) -> bool {
        self.wake_word.read().unwrap().is_enabled()
    }

    pub fn set_wake_word_enabled(&self, enabled: bool) {
        self.wake_word.write().unwrap().set_enabled(enabled);
    }

    pub fn wake_word_detector(&self) -> WakeWordDetector {
        self.wake_word.read().unwrap().clone()
    }

    /// Inspect current status of voice capabilities.
    pub fn status(&self) -> VoiceStatus {
        let local_avail = self.local_transcriber.is_available();
        let cloud_avail = !self.offline_only
            && self
                .cloud_transcriber
                .as_ref()
                .map(|c| c.is_available())
                .unwrap_or(false);
        let piper_avail = self
            .synthesizer
            .as_ref()
            .map(|s| s.is_available())
            .unwrap_or(false);

        VoiceStatus {
            enabled: true,
            stt_engine: self.engine_preference.clone(),
            tts_engine: "auto".to_string(),
            hotkey: self.hotkey.clone(),
            language: self.language.clone(),
            local_whisper_available: local_avail,
            cloud_whisper_available: cloud_avail,
            local_piper_available: piper_avail,
            web_speech_available: true, // Supported in modern desktop browsers
            wake_word_enabled: self.is_wake_word_enabled(),
            wake_words: self.wake_word.read().unwrap().phrases().to_vec(),
        }
    }

    /// Synthesize speech audio from text using configured synthesizer.
    pub async fn synthesize(&self, text: &str) -> Result<AudioBuffer> {
        if let Some(synth) = &self.synthesizer {
            if synth.is_available() {
                return synth.synthesize(text).await;
            }
        }
        bail!(
            "No local text-to-speech synthesizer is available. Use browser Web Speech Synthesis or install Piper TTS."
        );
    }

    /// Transcribe an audio buffer using the configured preference and fallback logic.
    pub async fn transcribe(&self, audio: &AudioBuffer) -> Result<TranscriptionResult> {
        let pref = self.engine_preference.as_str();

        match pref {
            "local-whisper" => {
                self.local_transcriber
                    .transcribe(audio, Some(&self.language))
                    .await
            }
            "cloud" | "groq" | "openai" => {
                if self.offline_only {
                    bail!("Offline mode is enabled: cloud STT is disallowed");
                }
                if let Some(cloud) = &self.cloud_transcriber {
                    cloud.transcribe(audio, Some(&self.language)).await
                } else {
                    bail!("Cloud STT is not configured");
                }
            }
            _ => {
                // "auto" mode:
                // 1. Try local whisper if available.
                if self.local_transcriber.is_available() {
                    return self
                        .local_transcriber
                        .transcribe(audio, Some(&self.language))
                        .await;
                }

                // 2. Try cloud whisper if allowed and available.
                if !self.offline_only {
                    if let Some(cloud) = &self.cloud_transcriber {
                        if cloud.is_available() {
                            return cloud.transcribe(audio, Some(&self.language)).await;
                        }
                    }
                }

                // 3. Fallback information for user.
                bail!(
                    "No speech-to-text backend is ready. Install whisper-cli for local speech recognition, or provide a cloud API key, or use browser Web Speech API."
                );
            }
        }
    }
}

/// Determines whether an action requiring user approval is "low-risk"
/// and eligible for approval via voice ("yes" / "no").
///
/// Security rule: Risky actions (file modifications, shell commands, destructive operations)
/// MUST NEVER be approved by voice, to prevent accidental execution from ambient noise,
/// misrecognized speech, or prompt injections. They require explicit manual click confirmation.
pub fn is_low_risk_action(tool: &str, summary: &str) -> bool {
    // 1. Tool-level exclusion: editing files, writing files, and running shell commands are high-risk.
    if matches!(tool, "write_file" | "edit_file" | "shell") {
        return false;
    }

    // 2. Destructive keyword scanning: any action referencing deletion or system disruption is high-risk.
    let s_lower = summary.to_lowercase();
    let dangerous_keywords = [
        "delete",
        "remove",
        "rm ",
        "rmdir",
        "drop",
        "truncate",
        "kill",
        "format",
        "wipe",
        "reset",
        "shutdown",
        "reboot",
        "unlink",
        "overwrite",
    ];
    for kw in dangerous_keywords {
        if s_lower.contains(kw) {
            return false;
        }
    }

    // 3. Known safe/low-risk tools (read-only outside workspace, memory storage, web fetch, sub-agent queries)
    matches!(
        tool,
        "read_file" | "glob" | "grep" | "web_fetch" | "remember" | "forget" | "recall" | "task"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pcm16_mono_and_wav_roundtrip() {
        // Generate a 400Hz test tone for 100ms at 16000Hz (1600 samples)
        let sample_rate = 16000u32;
        let num_samples = 1600usize;
        let mut samples = Vec::with_capacity(num_samples);
        for i in 0..num_samples {
            let t = i as f64 / sample_rate as f64;
            let val = (t * 400.0 * 2.0 * std::f64::consts::PI).sin();
            samples.push((val * 16000.0) as i16);
        }

        let audio = AudioBuffer::new_pcm16_mono(sample_rate, &samples);
        assert_eq!(audio.channels, 1);
        assert_eq!(audio.sample_rate, 16000);
        assert_eq!(audio.bits_per_sample, 16);
        assert_eq!(audio.duration_ms(), 100);

        let energy = audio.rms_energy();
        assert!(
            energy > 0.1 && energy < 0.6,
            "RMS energy should be moderate: {}",
            energy
        );

        let wav_bytes = audio.to_wav_bytes();
        assert_eq!(wav_bytes.len(), 44 + 3200);

        let parsed = AudioBuffer::from_wav_bytes(&wav_bytes).expect("Failed to parse WAV");
        assert_eq!(parsed.channels, 1);
        assert_eq!(parsed.sample_rate, 16000);
        assert_eq!(parsed.bits_per_sample, 16);
        assert_eq!(parsed.data, audio.data);
        assert_eq!(parsed.duration_ms(), 100);
    }

    #[test]
    fn test_invalid_wav_bytes() {
        let too_short = vec![0u8; 20];
        assert!(AudioBuffer::from_wav_bytes(&too_short).is_err());

        let bad_magic = b"RIFF1234BADWfmt 16......data....".to_vec();
        assert!(AudioBuffer::from_wav_bytes(&bad_magic).is_err());
    }

    #[tokio::test]
    async fn test_mock_transcriber() {
        let mock = MockTranscriber::new("mock-engine", "Hello Jarvis!");
        assert_eq!(mock.engine_name(), "mock-engine");
        assert!(mock.is_available());

        let samples = vec![0i16; 1600];
        let audio = AudioBuffer::new_pcm16_mono(16000, &samples);

        let res = mock
            .transcribe(&audio, Some("en"))
            .await
            .expect("Transcribe failed");
        assert_eq!(res.text, "Hello Jarvis!");
        assert_eq!(res.engine, "mock-engine");
        assert_eq!(res.duration_ms, 100);
        assert_eq!(res.confidence, Some(0.98));
    }

    #[tokio::test]
    async fn test_voice_service_auto_and_status() {
        let local_mock = Arc::new(MockTranscriber::new("local-whisper", "Turn on lights"));
        let cloud_mock = Arc::new(MockTranscriber::new("cloud-whisper", "Turn off lights"));
        let synth_mock = Arc::new(MockSynthesizer::new("mock-piper"));

        let service = VoiceService::new(
            local_mock,
            Some(cloud_mock),
            Some(synth_mock),
            "auto",
            "Ctrl+Space",
            "en",
            false,
        );

        let status = service.status();
        assert!(status.enabled);
        assert_eq!(status.stt_engine, "auto");
        assert_eq!(status.hotkey, "Ctrl+Space");
        assert!(status.local_whisper_available);
        assert!(status.cloud_whisper_available);
        assert!(status.local_piper_available);

        let audio = AudioBuffer::new_pcm16_mono(16000, &[100i16; 800]);
        let res = service
            .transcribe(&audio)
            .await
            .expect("Auto transcribe failed");
        // In auto mode with local available, it uses local
        assert_eq!(res.text, "Turn on lights");
        assert_eq!(res.engine, "local-whisper");

        // Test synthesis
        let spoken = service.synthesize("Turn on lights").await.unwrap();
        assert_eq!(spoken.sample_rate, 16000);
        assert!(spoken.duration_ms() >= 100);
    }

    #[tokio::test]
    async fn test_voice_service_offline_disallows_cloud() {
        let local_unavailable =
            Arc::new(MockTranscriber::new("local-whisper", "").with_available(false));
        let cloud_mock = Arc::new(MockTranscriber::new("cloud-whisper", "Cloud response"));

        let service = VoiceService::new(
            local_unavailable,
            Some(cloud_mock),
            None,
            "cloud",
            "Ctrl+Space",
            "en",
            true, // Offline only!
        );

        let status = service.status();
        assert!(
            !status.cloud_whisper_available,
            "Cloud must be unavailable when offline_only is true"
        );

        let audio = AudioBuffer::new_pcm16_mono(16000, &[0i16; 800]);
        let err = service.transcribe(&audio).await;
        assert!(err.is_err());
        assert!(err
            .unwrap_err()
            .to_string()
            .contains("Offline mode is enabled"));
    }

    #[test]
    fn test_sentence_splitter_streaming_and_code_omission() {
        let mut splitter = SentenceSplitter::new();

        // 1. Streaming sentence chunks
        let s1 = splitter.feed("Hello world! ");
        assert_eq!(s1, vec!["Hello world!"]);

        let s2 = splitter.feed("This is a ");
        assert!(s2.is_empty(), "Sentence is not finished yet");

        let s3 = splitter.feed("test. Here is another one? ");
        assert_eq!(s3, vec!["This is a test.", "Here is another one?"]);

        // 2. Abbreviations and numbers should not split prematurely
        let s4 = splitter.feed("Dr. Smith bought 3.14 units e.g. apples. ");
        assert_eq!(s4, vec!["Dr. Smith bought 3.14 units e.g. apples."]);

        // 3. Code blocks should be completely omitted from speech output
        let s5 =
            splitter.feed("Check out this code:\n```python\nprint('hello world')\n```\nAll done! ");
        assert_eq!(s5, vec!["Check out this code:", "All done!"]);

        // 4. Flush remaining buffer
        splitter.feed("Trailing sentence without period");
        let flushed = splitter.flush();
        assert_eq!(
            flushed,
            Some("Trailing sentence without period".to_string())
        );
    }

    #[test]
    fn test_is_low_risk_action() {
        // High-risk tools requiring manual click confirmation
        assert!(!is_low_risk_action("write_file", "write main.rs"));
        assert!(!is_low_risk_action("edit_file", "edit src/lib.rs"));
        assert!(!is_low_risk_action("shell", "run: cargo test"));
        assert!(!is_low_risk_action("shell", "run: ls"));

        // Destructive keyword actions requiring manual click
        assert!(!is_low_risk_action(
            "task",
            "delete temporary test directories"
        ));
        assert!(!is_low_risk_action(
            "forget",
            "forget and drop database tables"
        ));
        assert!(!is_low_risk_action("read_file", "format and wipe disk"));

        // Low-risk read-only and memory actions eligible for voice approval
        assert!(is_low_risk_action(
            "read_file",
            "read C:\\secrets\\doc.txt (outside the workspace)"
        ));
        assert!(is_low_risk_action(
            "glob",
            "search ../outside (outside the workspace)"
        ));
        assert!(is_low_risk_action(
            "grep",
            "search ../outside (outside the workspace)"
        ));
        assert!(is_low_risk_action("web_fetch", "fetch https://docs.rs"));
        assert!(is_low_risk_action(
            "remember",
            "remember: user prefers rust"
        ));
        assert!(is_low_risk_action("forget", "forget memory id 42"));
        assert!(is_low_risk_action(
            "task",
            "research rust async best practices"
        ));

        // Unknown custom tool defaults to requiring manual click
        assert!(!is_low_risk_action(
            "custom_mcp_exec",
            "run arbitrary action"
        ));
    }

    #[test]
    fn test_wake_word_detector() {
        let mut detector = WakeWordDetector::new(WakeWordConfig::default());
        assert!(!detector.is_enabled());

        // 1. Detection of 'hey jarvis'
        let (phrase, cmd) = detector
            .detect_in_text("Hey Jarvis, what time is it?")
            .expect("should match hey jarvis");
        assert_eq!(phrase, "hey jarvis");
        assert_eq!(cmd, "what time is it?");

        // 2. Detection of 'jarvis' with colon and punctuation
        let (phrase2, cmd2) = detector
            .detect_in_text("Jarvis: explain this codebase.")
            .expect("should match jarvis");
        assert_eq!(phrase2, "jarvis");
        assert_eq!(cmd2, "explain this codebase.");

        // 3. Case-insensitivity and whitespace handling
        let (phrase3, cmd3) = detector
            .detect_in_text("  HEY JARVIS   run the tests!  ")
            .expect("should match uppercase");
        assert_eq!(phrase3, "hey jarvis");
        assert_eq!(cmd3, "run the tests!");

        // 4. Non-matching queries
        assert!(detector.detect_in_text("What is the weather?").is_none());
        assert!(detector.detect_in_text("Alexa play music").is_none());
        assert!(detector.detect_in_text("").is_none());

        // 5. Energy gating
        let silent = AudioBuffer::new_pcm16_mono(16000, &[0i16; 800]);
        assert!(!detector.has_speech_energy(&silent));

        let loud = AudioBuffer::new_pcm16_mono(16000, &[5000i16; 800]);
        assert!(detector.has_speech_energy(&loud));

        // 6. Enable toggle
        detector.set_enabled(true);
        assert!(detector.is_enabled());
    }
}
