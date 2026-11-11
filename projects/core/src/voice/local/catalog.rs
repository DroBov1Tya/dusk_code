//! The models that can be installed here. Each is a backend of `resources/voice/server.py`,
//! the Python packages it needs, and the Hugging Face weights that script downloads.
//! Versions are exact: they are the ones that ran together.

use dusk_protocol::config::SpeechKind;

pub struct Model {
    pub id: &'static str,
    pub title: &'static str,
    pub about: &'static str,
    pub kind: SpeechKind,
    /// The name `server.py` knows the model by.
    pub backend: &'static str,
    pub packages: &'static [&'static str],
    pub size: &'static str,
    pub languages: &'static str,
    /// The voice a synthesis model starts with.
    pub voice: Option<&'static str>,
}

/// What every model's server needs besides its own packages.
pub const SERVER_PACKAGES: &[&str] = &[
    "starlette==1.10.0",
    "uvicorn==0.54.0",
    "python-multipart==0.0.33",
];

/// What the MLX models share: they run on the Apple GPU through `mlx-audio`, all in the
/// same pinned set.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
const MLX_PACKAGES: &[&str] = &[
    "soundfile==0.14.0",
    "mlx-audio==0.6.5",
    "mlx-whisper==0.4.3",
];

/// On Apple Silicon: the models run on the GPU with MLX, several times faster than the CPU.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
pub const MODELS: &[Model] = &[
    Model {
        id: "qwen3-asr",
        title: "Qwen3-ASR 0.6B",
        about: "Fast, accurate speech recognition on the Apple GPU",
        kind: SpeechKind::Recognition,
        backend: "qwen3-asr",
        packages: MLX_PACKAGES,
        size: "2.2 GB",
        languages: "English, Russian and 28 more",
        voice: None,
    },
    Model {
        id: "whisper-large-v3-turbo",
        title: "Whisper large-v3-turbo",
        about: "Accurate speech recognition on the Apple GPU",
        kind: SpeechKind::Recognition,
        backend: "mlx-whisper",
        packages: MLX_PACKAGES,
        size: "2.7 GB",
        languages: "English, Russian and about 90 more",
        voice: None,
    },
    Model {
        id: "qwen3-tts",
        title: "Qwen3-TTS 0.6B",
        about: "Natural reading aloud with nine built-in voices; faster than real time",
        kind: SpeechKind::Synthesis,
        backend: "qwen3-tts",
        packages: MLX_PACKAGES,
        size: "3.1 GB",
        languages: "English, Russian and 8 more",
        voice: Some("ryan"),
    },
    Model {
        id: "voxcpm2",
        title: "VoxCPM2",
        about: "Expressive reading aloud at 48 kHz; a female and a male voice",
        kind: SpeechKind::Synthesis,
        backend: "voxcpm2",
        packages: MLX_PACKAGES,
        size: "3.4 GB",
        languages: "English, Russian and 28 more",
        voice: Some("female"),
    },
    KOKORO,
];

/// Elsewhere the CPU runs them.
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
pub const MODELS: &[Model] = &[
    Model {
        id: "whisper-large-v3-turbo",
        title: "Whisper large-v3-turbo",
        about: "Accurate speech recognition on the CPU; runs offline",
        kind: SpeechKind::Recognition,
        backend: "whisper",
        // PyAV 19 breaks faster-whisper 1.2.1.
        packages: &["faster-whisper==1.2.1", "av==18.1.0"],
        size: "1.6 GB",
        languages: "English, Russian and about 90 more",
        voice: None,
    },
    KOKORO,
];

const KOKORO: Model = Model {
    id: "kokoro-82m",
    title: "Kokoro 82M",
    about: "Small, fast and natural reading aloud; English only",
    kind: SpeechKind::Synthesis,
    backend: "kokoro",
    // misaki would download the spaCy model at run time, which fails in a uv venv.
    packages: &[
        "kokoro==0.9.4",
        "soundfile==0.14.0",
        "en-core-web-sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.8.0/en_core_web_sm-3.8.0-py3-none-any.whl",
    ],
    size: "1 GB",
    languages: "English",
    voice: Some("af_heart"),
};

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}
