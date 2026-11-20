// ============================================================================
// Jarvis Voice & Push-to-Talk (PTT) Subsystem
// Handles push-to-talk hotkeys (default: Ctrl+Space), mic buttons in menubar
// and Oblivion panel, audio capture, local speech-to-text, sentence-by-sentence
// streamed text-to-speech, and barge-in (stopping speech when user talks).
// ============================================================================

(function () {
    'use strict';

    let isListening = false;
    let isProcessing = false;
    let recognition = null;
    let mediaRecorder = null;
    let audioChunks = [];
    let audioStream = null;
    let voiceOutputEnabled = true;
    let isSpeaking = false;
    let speechQueue = [];
    let currentAudio = null;

    let voiceStatus = {
        enabled: true,
        stt_engine: 'auto',
        tts_engine: 'auto',
        hotkey: 'Ctrl+Space',
        language: 'en',
        local_whisper_available: false,
        cloud_whisper_available: false,
        local_piper_available: false,
        web_speech_available: true
    };

    // DOM Elements
    let menubarMicBtn = null;
    let menubarMicIcon = null;
    let menubarMicLabel = null;
    let menubarSpeakerBtn = null;
    let menubarSpeakerIcon = null;
    let menubarSpeakerLabel = null;
    let obMicBtn = null;
    let obInput = null;
    let obSend = null;

    function getJarvisToken() {
        return window.__JARVIS_TOKEN__ || (window.oblivion ? window.oblivion.token : '');
    }

    // ========================================================================
    // Sentence Streamer: Buffers streamed text and extracts complete sentences
    // ========================================================================
    class SentenceStreamer {
        constructor() {
            this.buffer = '';
            this.inCodeBlock = false;
        }

        reset() {
            this.buffer = '';
            this.inCodeBlock = false;
        }

        feed(chunk) {
            const sentences = [];
            for (let i = 0; i < chunk.length; i++) {
                this.buffer += chunk[i];

                // Code block toggle (```)
                if (this.buffer.endsWith('```')) {
                    this.inCodeBlock = !this.inCodeBlock;
                    if (this.inCodeBlock) {
                        // Exclude code block from speech
                        const idx = this.buffer.lastIndexOf('```');
                        const before = this.buffer.substring(0, idx).trim();
                        if (before.length > 0) {
                            const clean = cleanTextForSpeech(before);
                            if (clean) sentences.push(clean);
                        }
                    }
                    this.buffer = '';
                    continue;
                }

                if (this.inCodeBlock) continue;

                if (this.isSentenceEnd()) {
                    const clean = cleanTextForSpeech(this.buffer);
                    if (clean) sentences.push(clean);
                    this.buffer = '';
                }
            }
            return sentences;
        }

        flush() {
            if (this.inCodeBlock) {
                this.buffer = '';
                return null;
            }
            const clean = cleanTextForSpeech(this.buffer);
            this.buffer = '';
            return clean || null;
        }

        isSentenceEnd() {
            const trimmed = this.buffer.trimEnd();
            if (trimmed.length === 0) return false;
            const last = trimmed[trimmed.length - 1];
            if (last !== '.' && last !== '!' && last !== '?' && last !== '\n') return false;

            // Avoid splitting on common abbreviations or decimal numbers
            if (last === '.') {
                const lower = trimmed.toLowerCase();
                const abbrs = ['e.g.', 'i.e.', 'dr.', 'mr.', 'mrs.', 'ms.', 'vs.', 'etc.', 'v1.0', 'v2.0'];
                for (let a of abbrs) {
                    if (lower.endsWith(a)) return false;
                }
                // Preceded by digit? (e.g. 3.14)
                if (trimmed.length >= 2 && /\d/.test(trimmed[trimmed.length - 2])) {
                    return false;
                }
            }

            return this.buffer.endsWith('. ') ||
                this.buffer.endsWith('! ') ||
                this.buffer.endsWith('? ') ||
                this.buffer.endsWith('.\n') ||
                this.buffer.endsWith('!\n') ||
                this.buffer.endsWith('?\n') ||
                this.buffer.endsWith('\n\n');
        }
    }

    const sentenceStreamer = new SentenceStreamer();

    function cleanTextForSpeech(text) {
        if (!text) return '';
        let s = text
            // Remove markdown code blocks and inline code
            .replace(/```[\s\S]*?```/g, '')
            .replace(/`([^`]+)`/g, '$1')
            // Remove markdown links [text](url) -> text
            .replace(/\[([^\]]+)\]\([^\)]+\)/g, '$1')
            // Remove markdown images ![alt](url) -> ''
            .replace(/!\[([^\]]*)\]\([^\)]+\)/g, '')
            // Remove headers #, ##, etc.
            .replace(/#{1,6}\s+/g, '')
            // Remove bold/italics
            .replace(/\*\*([^*]+)\*\*/g, '$1')
            .replace(/\*([^*]+)\*/g, '$1')
            .replace(/__([^_]+)__/g, '$1')
            .replace(/_([^_]+)_/g, '$1')
            // Remove bullet points and blockquotes
            .replace(/^[\s*>-]+\s+/gm, '')
            .trim();

        // Collapse multiple spaces
        s = s.replace(/\s+/g, ' ');
        return s;
    }

    // ========================================================================
    // Barge-in: immediately halt and discard any active or pending speech
    // ========================================================================
    function bargeIn() {
        speechQueue = [];
        sentenceStreamer.reset();
        isSpeaking = false;

        if (window.speechSynthesis) {
            try {
                window.speechSynthesis.cancel();
            } catch (e) {
                // ignore
            }
        }

        if (currentAudio) {
            try {
                currentAudio.pause();
                currentAudio.currentTime = 0;
            } catch (e) {
                // ignore
            }
            currentAudio = null;
        }

        updateSpeakerVisualState();
    }

    // ========================================================================
    // Speech Synthesis Queue
    // ========================================================================
    function speakSentence(sentence) {
        if (!voiceOutputEnabled || !sentence || sentence.trim().length === 0) return;
        speechQueue.push(sentence);
        if (!isSpeaking) {
            playNextSentence();
        }
    }

    async function playNextSentence() {
        if (speechQueue.length === 0) {
            isSpeaking = false;
            updateSpeakerVisualState();
            return;
        }

        if (!voiceOutputEnabled) {
            speechQueue = [];
            isSpeaking = false;
            updateSpeakerVisualState();
            return;
        }

        isSpeaking = true;
        updateSpeakerVisualState();
        const sentence = speechQueue.shift();

        // Prefer window.speechSynthesis for instant zero-latency speech
        if ('speechSynthesis' in window) {
            try {
                const utterance = new SpeechSynthesisUtterance(sentence);
                utterance.lang = voiceStatus.language === 'en' ? 'en-US' : voiceStatus.language;

                // Pick a natural voice if available
                const voices = window.speechSynthesis.getVoices();
                if (voices && voices.length > 0) {
                    const preferred = voices.find(v =>
                        (v.lang.startsWith('en') && (v.name.includes('Natural') || v.name.includes('David') || v.name.includes('Jenny') || v.name.includes('Aria') || v.name.includes('Google')))
                    );
                    if (preferred) {
                        utterance.voice = preferred;
                    }
                }

                utterance.onend = function () {
                    playNextSentence();
                };

                utterance.onerror = function (e) {
                    console.warn('[Voice] Utterance error:', e);
                    playNextSentence();
                };

                window.speechSynthesis.speak(utterance);
                return;
            } catch (e) {
                console.warn('[Voice] speechSynthesis speak failed, trying backend TTS:', e);
            }
        }

        // Backend fallback: POST /api/oblivion/voice/tts/speak
        try {
            const token = getJarvisToken();
            const res = await fetch('/api/oblivion/voice/tts/speak', {
                method: 'POST',
                headers: {
                    'Content-Type': 'application/json',
                    ...(token ? { 'X-Jarvis-Token': token } : {})
                },
                body: JSON.stringify({ text: sentence })
            });

            if (res.ok) {
                const data = await res.json();
                if (data.audio_base64) {
                    const audio = new Audio('data:audio/wav;base64,' + data.audio_base64);
                    currentAudio = audio;
                    audio.onended = function () {
                        currentAudio = null;
                        playNextSentence();
                    };
                    audio.onerror = function () {
                        currentAudio = null;
                        playNextSentence();
                    };
                    await audio.play();
                    return;
                }
            }
        } catch (e) {
            console.warn('[Voice] Backend TTS failed:', e);
        }

        // If synthesis failed, continue queue
        playNextSentence();
    }

    // Callbacks for live SSE text events
    function onTextChunk(text) {
        if (!voiceOutputEnabled) return;
        const sentences = sentenceStreamer.feed(text);
        for (let s of sentences) {
            speakSentence(s);
        }
    }

    function onTurnEnd() {
        if (!voiceOutputEnabled) return;
        const finalSentence = sentenceStreamer.flush();
        if (finalSentence) {
            speakSentence(finalSentence);
        }
    }

    async function fetchVoiceStatus() {
        try {
            const token = getJarvisToken();
            const res = await fetch('/api/oblivion/voice/status', {
                headers: token ? { 'X-Jarvis-Token': token } : {}
            });
            if (res.ok) {
                const data = await res.json();
                voiceStatus = Object.assign(voiceStatus, data);
                updateHotkeyHint();
            }
        } catch (e) {
            console.warn('[Voice] Status fetch error:', e);
        }
    }

    function updateHotkeyHint() {
        const hint = voiceStatus.hotkey || 'Ctrl+Space';
        if (menubarMicBtn) {
            menubarMicBtn.title = `Push-to-talk: Hold ${hint} or click to speak`;
        }
        if (obMicBtn) {
            obMicBtn.title = `Push-to-talk: Hold ${hint} or click to speak`;
        }
    }

    function initSpeechRecognition() {
        const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
        if (SpeechRecognition) {
            recognition = new SpeechRecognition();
            recognition.continuous = false;
            recognition.interimResults = true;
            recognition.lang = voiceStatus.language || 'en-US';

            recognition.onstart = function () {
                isListening = true;
                updateVisualState(true);
            };

            recognition.onresult = function (event) {
                let interimTranscript = '';
                let finalTranscript = '';
                for (let i = event.resultIndex; i < event.results.length; ++i) {
                    if (event.results[i].isFinal) {
                        finalTranscript += event.results[i][0].transcript;
                    } else {
                        interimTranscript += event.results[i][0].transcript;
                    }
                }
                const text = (finalTranscript || interimTranscript).trim();
                if (text && obInput) {
                    obInput.value = text;
                    if (obSend) obSend.disabled = false;
                }
            };

            recognition.onerror = function (event) {
                console.warn('[Voice] SpeechRecognition error:', event.error);
                if (event.error !== 'no-speech') {
                    stopListening(false);
                }
            };

            recognition.onend = function () {
                if (isListening) {
                    stopListening(true);
                }
            };
        }
    }

    function updateVisualState(listening, processing) {
        if (listening) {
            if (menubarMicBtn) {
                menubarMicBtn.classList.add('recording');
                if (menubarMicLabel) menubarMicLabel.textContent = 'Listening...';
                if (menubarMicIcon) menubarMicIcon.textContent = '🔴';
            }
            if (obMicBtn) {
                obMicBtn.classList.add('recording');
                obMicBtn.textContent = '🔴';
            }
        } else if (processing) {
            if (menubarMicBtn) {
                menubarMicBtn.classList.remove('recording');
                if (menubarMicLabel) menubarMicLabel.textContent = 'Transcribing...';
                if (menubarMicIcon) menubarMicIcon.textContent = '⏳';
            }
            if (obMicBtn) {
                obMicBtn.classList.remove('recording');
                obMicBtn.textContent = '⏳';
            }
        } else {
            if (menubarMicBtn) {
                menubarMicBtn.classList.remove('recording');
                if (menubarMicLabel) menubarMicLabel.textContent = 'Voice';
                if (menubarMicIcon) menubarMicIcon.textContent = '🎙️';
            }
            if (obMicBtn) {
                obMicBtn.classList.remove('recording');
                obMicBtn.textContent = '🎙️';
            }
        }
    }

    function updateSpeakerVisualState() {
        if (!menubarSpeakerBtn) return;
        if (!voiceOutputEnabled) {
            menubarSpeakerBtn.classList.add('muted');
            if (menubarSpeakerIcon) menubarSpeakerIcon.textContent = '🔇';
            if (menubarSpeakerLabel) menubarSpeakerLabel.textContent = 'Muted';
            menubarSpeakerBtn.title = 'Voice Output: Muted (Click to unmute)';
        } else if (isSpeaking) {
            menubarSpeakerBtn.classList.remove('muted');
            if (menubarSpeakerIcon) menubarSpeakerIcon.textContent = '🗣️';
            if (menubarSpeakerLabel) menubarSpeakerLabel.textContent = 'Speaking...';
            menubarSpeakerBtn.title = 'Voice Output: Speaking (Click to mute or barge in)';
        } else {
            menubarSpeakerBtn.classList.remove('muted');
            if (menubarSpeakerIcon) menubarSpeakerIcon.textContent = '🔊';
            if (menubarSpeakerLabel) menubarSpeakerLabel.textContent = 'Speech';
            menubarSpeakerBtn.title = 'Voice Output: Enabled (Click to mute)';
        }
    }

    async function startListening() {
        if (isListening || isProcessing) return;

        // Barge-in: halt any active text-to-speech output immediately
        bargeIn();

        // Ensure Oblivion drawer is visible if on desktop
        if (typeof window.openOblivionPanel === 'function') {
            window.openOblivionPanel();
        } else {
            const panel = document.getElementById('ob-drawer');
            if (panel && panel.classList.contains('closed')) {
                panel.classList.remove('closed');
            }
        }

        isListening = true;
        updateVisualState(true, false);

        // Try Web Speech API first if supported
        if (recognition) {
            try {
                recognition.start();
                return;
            } catch (e) {
                console.warn('[Voice] Web Speech start failed, falling back to MediaRecorder:', e);
            }
        }

        // Fallback: MediaRecorder audio capture -> POST /api/oblivion/voice/transcribe
        if (navigator.mediaDevices && navigator.mediaDevices.getUserMedia) {
            try {
                audioStream = await navigator.mediaDevices.getUserMedia({ audio: true });
                audioChunks = [];
                mediaRecorder = new MediaRecorder(audioStream);
                mediaRecorder.ondataavailable = function (e) {
                    if (e.data.size > 0) {
                        audioChunks.push(e.data);
                    }
                };
                mediaRecorder.start();
            } catch (err) {
                console.warn('[Voice] getUserMedia error:', err);
                isListening = false;
                updateVisualState(false, false);
            }
        } else {
            console.warn('[Voice] Neither SpeechRecognition nor getUserMedia is available');
            isListening = false;
            updateVisualState(false, false);
        }
    }

    async function stopListening(submitOnStop) {
        if (!isListening) return;
        isListening = false;
        isProcessing = true;
        updateVisualState(false, true);

        if (recognition) {
            try {
                recognition.stop();
            } catch (e) {
                // ignore
            }
        }

        if (mediaRecorder && mediaRecorder.state !== 'inactive') {
            mediaRecorder.onstop = async function () {
                if (audioStream) {
                    audioStream.getTracks().forEach(track => track.stop());
                }
                const audioBlob = new Blob(audioChunks, { type: 'audio/wav' });
                await sendAudioForTranscription(audioBlob);
                isProcessing = false;
                updateVisualState(false, false);
                checkAutoSubmit(submitOnStop);
            };
            mediaRecorder.stop();
        } else {
            isProcessing = false;
            updateVisualState(false, false);
            checkAutoSubmit(submitOnStop);
        }
    }

    async function sendAudioForTranscription(blob) {
        try {
            const token = getJarvisToken();
            const res = await fetch('/api/oblivion/voice/transcribe', {
                method: 'POST',
                headers: {
                    'Content-Type': 'audio/wav',
                    ...(token ? { 'X-Jarvis-Token': token } : {})
                },
                body: blob
            });
            if (res.ok) {
                const data = await res.json();
                if (data.text && obInput) {
                    obInput.value = data.text;
                    if (obSend) obSend.disabled = false;
                }
            } else {
                console.warn('[Voice] Transcription request failed with status:', res.status);
            }
        } catch (e) {
            console.warn('[Voice] Error sending audio for transcription:', e);
        }
    }

    function checkAutoSubmit(submit) {
        if (submit && obInput && obInput.value.trim().length > 0) {
            if (obSend && !obSend.disabled) {
                bargeIn();
                obSend.click();
            }
        }
    }

    function toggleListening() {
        if (isListening) {
            stopListening(false);
        } else {
            startListening();
        }
    }

    function toggleVoiceOutput() {
        voiceOutputEnabled = !voiceOutputEnabled;
        if (!voiceOutputEnabled) {
            bargeIn();
        }
        localStorage.setItem('jarvis_voice_output', voiceOutputEnabled ? 'true' : 'false');
        updateSpeakerVisualState();
    }

    function initEventListeners() {
        menubarMicBtn = document.getElementById('menubar-mic-btn');
        menubarMicIcon = document.getElementById('menubar-mic-icon');
        menubarMicLabel = document.getElementById('menubar-mic-label');
        menubarSpeakerBtn = document.getElementById('menubar-speaker-btn');
        menubarSpeakerIcon = document.getElementById('menubar-speaker-icon');
        menubarSpeakerLabel = document.getElementById('menubar-speaker-label');
        obMicBtn = document.getElementById('ob-mic-btn');
        obInput = document.getElementById('ob-input') || document.getElementById('input');
        obSend = document.getElementById('ob-send') || document.getElementById('send');

        const savedOutput = localStorage.getItem('jarvis_voice_output');
        if (savedOutput !== null) {
            voiceOutputEnabled = savedOutput === 'true';
        }
        updateSpeakerVisualState();

        if (menubarMicBtn) {
            menubarMicBtn.addEventListener('click', function (e) {
                e.preventDefault();
                toggleListening();
            });
        }

        if (menubarSpeakerBtn) {
            menubarSpeakerBtn.addEventListener('click', function (e) {
                e.preventDefault();
                toggleVoiceOutput();
            });
        }

        if (obMicBtn) {
            obMicBtn.addEventListener('click', function (e) {
                e.preventDefault();
                toggleListening();
            });
        }

        // Global Push-To-Talk hotkey: default Ctrl+Space
        window.addEventListener('keydown', function (e) {
            // Check for Ctrl+Space
            if (e.ctrlKey && (e.code === 'Space' || e.keyCode === 32)) {
                // If not currently listening, start push-to-talk
                if (!isListening && !isProcessing) {
                    e.preventDefault();
                    startListening();
                }
            } else if (e.key === 'Escape') {
                // Escape key acts as a barge-in stop
                bargeIn();
            }
        });

        window.addEventListener('keyup', function (e) {
            // Releasing space or ctrl while listening stops push-to-talk
            if (isListening && (e.code === 'Space' || e.keyCode === 32 || e.key === 'Control')) {
                e.preventDefault();
                stopListening(true);
            }
        });
    }

    // Initialize when DOM is ready
    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', function () {
            initEventListeners();
            initSpeechRecognition();
            fetchVoiceStatus();
        });
    } else {
        initEventListeners();
        initSpeechRecognition();
        fetchVoiceStatus();
    }

    // Expose on window for programmatic control and testing
    window.JarvisVoice = {
        start: startListening,
        stop: stopListening,
        toggle: toggleListening,
        bargeIn: bargeIn,
        speakSentence: speakSentence,
        onTextChunk: onTextChunk,
        onTurnEnd: onTurnEnd,
        toggleOutput: toggleVoiceOutput,
        status: () => Object.assign({}, voiceStatus),
        isListening: () => isListening,
        isSpeaking: () => isSpeaking,
        getQueue: () => speechQueue.slice(),
        cleanText: cleanTextForSpeech,
        SentenceStreamer: SentenceStreamer
    };
})();
