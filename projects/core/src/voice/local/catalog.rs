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
    Model {
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
    },
];

pub fn find(id: &str) -> Option<&'static Model> {
    MODELS.iter().find(|model| model.id == id)
}
