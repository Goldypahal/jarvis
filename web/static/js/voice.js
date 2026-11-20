// ============================================================================
// Jarvis Voice & Push-to-Talk (PTT) Subsystem
// Handles push-to-talk hotkeys (default: Ctrl+Space), mic buttons in menubar
// and Oblivion panel, audio capture, local speech-to-text, and visual states.
// ============================================================================

(function () {
    'use strict';

    let isListening = false;
    let isProcessing = false;
    let recognition = null;
    let mediaRecorder = null;
    let audioChunks = [];
    let audioStream = null;
    let voiceStatus = {
        enabled: true,
        stt_engine: 'auto',
        hotkey: 'Ctrl+Space',
        language: 'en',
        local_whisper_available: false,
        cloud_whisper_available: false,
        web_speech_available: true
    };

    // DOM Elements
    let menubarMicBtn = null;
    let menubarMicIcon = null;
    let menubarMicLabel = null;
    let obMicBtn = null;
    let obInput = null;
    let obSend = null;

    function getJarvisToken() {
        return window.__JARVIS_TOKEN__ || (window.oblivion ? window.oblivion.token : '');
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

    async function startListening() {
        if (isListening || isProcessing) return;

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

    function initEventListeners() {
        menubarMicBtn = document.getElementById('menubar-mic-btn');
        menubarMicIcon = document.getElementById('menubar-mic-icon');
        menubarMicLabel = document.getElementById('menubar-mic-label');
        obMicBtn = document.getElementById('ob-mic-btn');
        obInput = document.getElementById('ob-input') || document.getElementById('input');
        obSend = document.getElementById('ob-send') || document.getElementById('send');

        if (menubarMicBtn) {
            menubarMicBtn.addEventListener('click', function (e) {
                e.preventDefault();
                toggleListening();
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
        status: () => Object.assign({}, voiceStatus),
        isListening: () => isListening
    };
})();
