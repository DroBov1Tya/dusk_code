"""Local OpenAI-style audio server: one model per process.

    python server.py serve --backend NAME --host 127.0.0.1 --port N
    python server.py download --backend NAME
    python server.py --version

Backends: whisper, kokoro (CPU); mlx-whisper, qwen3-asr, qwen3-tts, voxcpm2
(Apple Silicon GPU through MLX, macOS arm64 only).

Pinned packages per backend (installed by the app into a uv venv):
    common:  starlette==1.10.0 uvicorn==0.54.0 python-multipart==0.0.33
    whisper: faster-whisper==1.2.1 av==18.1.0   (av 19 breaks faster-whisper 1.2.1)
    kokoro:  kokoro==0.9.4 soundfile==0.14.0 and the en_core_web_sm 3.8.0 wheel
             (otherwise misaki tries to pip-download it at runtime)
    mlx-*:   one venv for all four MLX backends:
             mlx-audio==0.6.5 mlx-whisper==0.4.3 soundfile==0.14.0
"""

import os

# The xet downloader stalls on some networks; plain HTTP works.
os.environ.setdefault("HF_HUB_DISABLE_XET", "1")

import argparse
import asyncio
import concurrent.futures
import hmac
import io
import json
import re
import shutil
import sys
import tempfile
import threading
from typing import Any, Callable

PROTOCOL_VERSION = 1

BACKENDS = ("whisper", "kokoro", "mlx-whisper", "qwen3-asr", "qwen3-tts", "voxcpm2")
STT_BACKENDS = ("whisper", "mlx-whisper", "qwen3-asr")
LOOPBACK_HOSTS = ("127.0.0.1", "::1")

WHISPER_REPO = "dropbox-dash/faster-whisper-large-v3-turbo"
KOKORO_REPO = "hexgrad/Kokoro-82M"
KOKORO_SAMPLE_RATE = 24000
KOKORO_DEFAULT_VOICE = "af_heart"
# The pipeline is built for American English ("a"), so only a* voices fit it.
KOKORO_VOICE_PREFIX = "a"
KOKORO_ALLOW_PATTERNS = ["config.json", "kokoro-v1_0.pth", "voices/a*.pt"]

# backend -> (repo, pinned commit or None, files serve needs or None for all).
# A pinned commit makes the offline lookup exact and the weights reproducible.
MODELS: dict[str, tuple[str, str | None, list[str] | None]] = {
    "whisper": (WHISPER_REPO, None, None),
    "kokoro": (KOKORO_REPO, None, KOKORO_ALLOW_PATTERNS),
    "mlx-whisper": (
        "mlx-community/whisper-large-v3-turbo",
        "a4aaeec0636e6fef84abdcbe3544cb2bf7e9f6fb",
        ["config.json", "weights.safetensors"],
    ),
    "qwen3-asr": (
        "mlx-community/Qwen3-ASR-0.6B-8bit",
        "89e96d92ba34aca20b3e29fb10cc284097d1219f",
        ["*.json", "*.safetensors", "merges.txt"],
    ),
    "qwen3-tts": (
        "mlx-community/Qwen3-TTS-12Hz-0.6B-CustomVoice-8bit",
        "049ef77fe8816b536193c0c25f9a214d17921282",
        ["*.json", "*.safetensors", "merges.txt"],  # * also matches speech_tokenizer/
    ),
    "voxcpm2": (
        "mlx-community/VoxCPM2-4bit",
        "dc9e5c187858da5f4a13dc4c247e297339216381",
        ["*.json", "*.safetensors"],
    ),
}

STT_SAMPLE_RATE = 16000
SPEECH_PART_CHARS = 240
# Split after sentence punctuation (Latin, Cyrillic and CJK texts) or a line break.
SENTENCE_END_RE = re.compile(r"(?<=[.!?;\u2026])\s+|(?<=[\u3002\uff01\uff1f])|\n+")
# Bounds decoded memory: 25 MiB of opus can hold many hours of speech.
MAX_AUDIO_SECONDS = 2 * 60 * 60

# Qwen3-ASR takes language names; the API takes ISO 639 codes.
QWEN_ASR_LANGUAGES = {
    "zh": "Chinese", "en": "English", "yue": "Cantonese", "ar": "Arabic",
    "de": "German", "fr": "French", "es": "Spanish", "pt": "Portuguese",
    "id": "Indonesian", "it": "Italian", "ko": "Korean", "ru": "Russian",
    "th": "Thai", "vi": "Vietnamese", "ja": "Japanese", "tr": "Turkish",
    "hi": "Hindi", "ms": "Malay", "nl": "Dutch", "sv": "Swedish",
    "da": "Danish", "fi": "Finnish", "pl": "Polish", "cs": "Czech",
    "fil": "Filipino", "fa": "Persian", "el": "Greek", "ro": "Romanian",
    "hu": "Hungarian", "mk": "Macedonian",
}  # fmt: skip

MIN_TOKEN_LEN = 16
MAX_UPLOAD_BYTES = 25 * 1024 * 1024
# Multipart framing and the small text fields on top of the file itself.
MAX_MULTIPART_OVERHEAD = 64 * 1024
MAX_JSON_BYTES = 64 * 1024
MAX_SPEECH_CHARS = 4096
LANGUAGE_RE = re.compile(r"[a-z]{2,3}")
# espeak-ng aborts the process on data paths longer than about 160 chars.
ESPEAK_MAX_PATH = 120


# The app reads stage lines from stdout; serve() sends library chatter to stderr.
PROTOCOL_OUT = sys.stdout


def log_json(**fields: Any) -> None:
    print(json.dumps(fields), file=PROTOCOL_OUT, flush=True)


def fail(message: str, code: int = 1) -> "NoReturn":  # noqa: F821
    print(message, file=sys.stderr, flush=True)
    sys.exit(code)


# ---------------------------------------------------------------- download


def download(backend: str) -> None:
    from huggingface_hub import snapshot_download

    repo, revision, patterns = MODELS[backend]
    log_json(stage="download", repo=repo)
    snapshot_download(
        repo,
        revision=revision,
        allow_patterns=patterns,
        token=os.environ.get("HF_TOKEN") or None,
    )
    log_json(stage="done")


def local_snapshot(backend: str) -> str:
    """Path of the cached snapshot; never touches the network.

    The patterns must match the download, or hub 2.x calls the partial snapshot incomplete.
    """
    from huggingface_hub import snapshot_download

    repo, revision, patterns = MODELS[backend]
    return snapshot_download(
        repo, revision=revision, allow_patterns=patterns, local_files_only=True
    )


# ---------------------------------------------------------------- backends


class WhisperEngine:
    name = "whisper"

    def __init__(self) -> None:
        from faster_whisper import WhisperModel

        self._model = WhisperModel(
            local_snapshot("whisper"), device="cpu", compute_type="float32"
        )
        self._lock = threading.Lock()  # one model instance, one caller at a time
        self.languages = frozenset(getattr(self._model, "supported_languages", ()))

    def transcribe(self, path: str, language: str | None) -> str:
        with self._lock:
            # segments is a lazy generator: decoding happens while joining.
            segments, _info = self._model.transcribe(
                path, language=language, beam_size=1
            )
            return " ".join(s.text.strip() for s in segments).strip()


class KokoroEngine:
    name = "kokoro"

    def __init__(self) -> None:
        ensure_short_espeak_path()
        from kokoro import KPipeline

        snapshot = local_snapshot("kokoro")
        self.voices = frozenset(
            f[:-3]
            for f in os.listdir(os.path.join(snapshot, "voices"))
            if f.endswith(".pt") and f.startswith(KOKORO_VOICE_PREFIX)
        )
        if KOKORO_DEFAULT_VOICE not in self.voices:
            raise RuntimeError("default voice missing from the local model snapshot")
        self._pipe = KPipeline(lang_code="a", repo_id=KOKORO_REPO, device="cpu")
        self._lock = threading.Lock()
        self.default_voice = KOKORO_DEFAULT_VOICE

    def speak(self, text: str, voice: str) -> bytes | None:
        with self._lock:
            chunks = [
                audio.numpy()
                for _, _, audio in self._pipe(text, voice=voice)
                if audio is not None
            ]
        return wav_bytes(chunks, KOKORO_SAMPLE_RATE)


def ensure_short_espeak_path() -> None:
    """Point espeak at a short copy of its data when the venv path is too long."""
    import espeakng_loader

    data = espeakng_loader.get_data_path()
    if len(data) <= ESPEAK_MAX_PATH:
        return
    short = os.path.join(tempfile.gettempdir(), f"dusk-espeak-{os.getuid()}")
    if not os.path.isdir(os.path.join(short, "espeak-ng-data")):
        staging = tempfile.mkdtemp(prefix="dusk-espeak-", dir=tempfile.gettempdir())
        shutil.copytree(data, os.path.join(staging, "espeak-ng-data"))
        try:
            os.rename(staging, short)
        except OSError:  # another process won the race
            shutil.rmtree(staging, ignore_errors=True)
    from misaki import espeak

    espeak.EspeakWrapper.set_data_path(os.path.join(short, "espeak-ng-data"))


class MlxWhisperEngine:
    name = "mlx-whisper"

    def __init__(self) -> None:
        import mlx.core as mx
        from mlx_whisper.tokenizer import LANGUAGES
        from mlx_whisper.transcribe import ModelHolder

        self._path = local_snapshot(self.name)
        # transcribe() reuses this cached instance when given the same path.
        ModelHolder.get_model(self._path, mx.float16)
        self.languages = frozenset(LANGUAGES)
        self._lock = threading.Lock()

    def transcribe(self, path: str, language: str | None) -> str:
        import mlx_whisper

        audio = decode_audio(path)
        with self._lock:
            # verbose=None: no progress bar and no transcript on the console.
            result = mlx_whisper.transcribe(
                audio, path_or_hf_repo=self._path, language=language, verbose=None
            )
        return result["text"].strip()


class QwenAsrEngine:
    name = "qwen3-asr"

    def __init__(self) -> None:
        from mlx_audio.stt.utils import load_model

        self._model = load_model(local_snapshot(self.name))
        supported = set(self._model.config.support_languages or ())
        self._names = {k: v for k, v in QWEN_ASR_LANGUAGES.items() if v in supported}
        if not self._names:
            raise RuntimeError("the local model lists no known languages")
        self.languages = frozenset(self._names)
        self._lock = threading.Lock()

    def transcribe(self, path: str, language: str | None) -> str:
        audio = decode_audio(path)
        name = self._names[language] if language else None  # None: auto-detect
        with self._lock:
            result = self._model.generate(audio, language=name, verbose=False)
        return result.text.strip()


class QwenTtsEngine:
    name = "qwen3-tts"

    def __init__(self) -> None:
        from mlx_audio.tts.utils import load_model

        self._model = load_model(local_snapshot(self.name))
        self.voices = frozenset(s.lower() for s in self._model.get_supported_speakers())
        self.default_voice = "ryan"
        if self.default_voice not in self.voices:
            raise RuntimeError("default voice missing from the local model")
        self._lock = threading.Lock()

    def speak(self, text: str, voice: str) -> bytes | None:
        with self._lock:
            # lang_code "auto" lets the model read the script; it covers en and ru.
            chunks = [
                to_numpy(r.audio)
                for part in speech_parts(text)
                for r in self._model.generate(part, voice=voice, lang_code="auto")
            ]
        return wav_bytes(chunks, self._model.sample_rate)


# VoxCPM2 has no stock speakers; a fixed voice description stands in for one.
VOXCPM_VOICES = {
    "female": "A young woman, warm and clear voice, calm pace",
    "male": "A middle-aged man, deep and clear voice, calm pace",
}


class VoxCpmEngine:
    name = "voxcpm2"

    def __init__(self) -> None:
        from mlx_audio.tts.utils import load_model

        self._model = load_model(local_snapshot(self.name))
        self.voices = frozenset(VOXCPM_VOICES)
        self.default_voice = "female"
        self._lock = threading.Lock()

    def speak(self, text: str, voice: str) -> bytes | None:
        with self._lock:
            chunks = [
                to_numpy(r.audio)
                for part in speech_parts(text)
                for r in self._model.generate(part, instruct=VOXCPM_VOICES[voice])
            ]
        return wav_bytes(chunks, self._model.sample_rate)


def speech_parts(text: str) -> list[str]:
    """Sentences packed into parts of at most SPEECH_PART_CHARS.

    Both MLX TTS models drift into repetition or mumbling on long inputs, so
    each part is synthesized on its own.
    """
    words: list[str] = []
    for sentence in SENTENCE_END_RE.split(text):
        words.extend(sentence.split())
        words.append("\n")  # sentence boundary marker
    parts: list[str] = []
    current = ""
    for word in words:
        if word == "\n":
            if len(current) >= SPEECH_PART_CHARS // 2:
                parts.append(current)
                current = ""
            continue
        if current and len(current) + 1 + len(word) > SPEECH_PART_CHARS:
            parts.append(current)
            current = ""
        current = f"{current} {word}" if current else word
    if current:
        parts.append(current)
    return parts


class BadAudio(Exception):
    """The upload could not be decoded; the caller's fault, not ours."""


def decode_audio(path: str) -> Any:
    """Mono float32 at the STT rate, decoded without ffmpeg for wav, mp3 and flac."""
    from mlx_audio.audio_io import read

    try:
        audio, _ = read(
            path,
            dtype="float32",
            sample_rate=STT_SAMPLE_RATE,
            nchannels=1,
            duration=MAX_AUDIO_SECONDS + 1,
        )
    except Exception as exc:  # any decoder failure means the bytes are not audio
        raise BadAudio(f"{type(exc).__name__}: {exc}") from exc
    if audio.size == 0:
        raise BadAudio("no audio samples")
    if audio.size > MAX_AUDIO_SECONDS * STT_SAMPLE_RATE:
        raise BadAudio("audio longer than the limit")
    return audio


def to_numpy(audio: Any) -> Any:
    import numpy as np

    return np.asarray(audio, dtype=np.float32).reshape(-1)


def wav_bytes(chunks: list, rate: int) -> bytes | None:
    """16-bit mono PCM WAV, or None when nothing was produced."""
    import numpy as np
    import soundfile

    chunks = [c for c in chunks if c.size]
    if not chunks:
        return None
    buf = io.BytesIO()
    samples = np.clip(np.concatenate(chunks), -1.0, 1.0)
    soundfile.write(buf, samples, rate, format="WAV", subtype="PCM_16")
    return buf.getvalue()


ENGINES: dict[str, Callable[[], Any]] = {
    "whisper": WhisperEngine,
    "kokoro": KokoroEngine,
    "mlx-whisper": MlxWhisperEngine,
    "qwen3-asr": QwenAsrEngine,
    "qwen3-tts": QwenTtsEngine,
    "voxcpm2": VoxCpmEngine,
}


# ------------------------------------------------------------------- HTTP


def make_app(engine: Any, token: str, worker: concurrent.futures.Executor):
    from starlette.applications import Starlette
    from starlette.datastructures import Headers
    from starlette.exceptions import HTTPException
    from starlette.formparsers import MultiPartParser
    from starlette.requests import Request
    from starlette.responses import JSONResponse, PlainTextResponse, Response
    from starlette.routing import Route

    token_bytes = token.encode()

    def error(status: int, message: str) -> JSONResponse:
        return JSONResponse({"error": {"message": message}}, status_code=status)

    class BodyTooLarge(Exception):
        pass

    async def limited_stream(request: Request, limit: int):
        total = 0
        async for chunk in request.stream():
            total += len(chunk)
            if total > limit:
                raise BodyTooLarge
            yield chunk

    def declared_length_ok(request: Request, limit: int) -> bool:
        raw = request.headers.get("content-length")
        return raw is None or (raw.isdigit() and int(raw) <= limit)

    async def run_model(fn: Callable, *args: Any) -> Any:
        return await asyncio.wrap_future(worker.submit(fn, *args))

    async def health(_: Request) -> Response:
        return JSONResponse({"ok": True, "backend": engine.name})

    async def transcriptions(request: Request) -> Response:
        limit = MAX_UPLOAD_BYTES + MAX_MULTIPART_OVERHEAD
        if not declared_length_ok(request, limit):
            return error(413, "upload too large (max 25 MiB)")
        if not request.headers.get("content-type", "").startswith(
            "multipart/form-data"
        ):
            return error(400, "expected multipart/form-data")
        parser = MultiPartParser(
            request.headers,
            limited_stream(request, limit),
            max_files=1,
            max_fields=8,
            max_part_size=4096,
        )
        try:
            form = await parser.parse()
        except BodyTooLarge:
            return error(413, "upload too large (max 25 MiB)")
        except Exception:
            return error(400, "malformed multipart body")
        try:
            return await transcribe_form(form)
        finally:
            await form.close()

    async def transcribe_form(form) -> Response:
        upload = form.get("file")
        if upload is None or isinstance(upload, str):
            return error(400, "field 'file' must be a file upload")
        if upload.size is None or upload.size > MAX_UPLOAD_BYTES:
            return error(413, "upload too large (max 25 MiB)")
        if upload.size == 0:
            return error(400, "empty file")
        model = form.get("model")
        if model is not None and not isinstance(model, str):
            return error(400, "field 'model' must be a string")
        language = form.get("language")
        if language in ("", None):
            language = None
        elif (
            not isinstance(language, str)
            or not LANGUAGE_RE.fullmatch(language)
            or (engine.languages and language not in engine.languages)
        ):
            return error(400, "unsupported language code")
        fmt = form.get("response_format") or "json"
        if fmt not in ("json", "text"):
            return error(400, "response_format must be 'json' or 'text'")

        await upload.seek(0)
        with tempfile.NamedTemporaryFile(prefix="dusk-stt-", suffix=".audio") as tmp:
            shutil.copyfileobj(upload.file, tmp, length=1024 * 1024)
            tmp.flush()
            try:
                text = await run_model(engine.transcribe, tmp.name, language)
            except Exception as exc:
                # av raises on undecodable audio; that is the caller's fault.
                from_av = type(exc).__module__.split(".")[0] == "av"
                if from_av or isinstance(exc, BadAudio):
                    print(f"decode error: {exc}", file=sys.stderr, flush=True)
                    return error(400, "could not decode the audio")
                raise
        if fmt == "text":
            return PlainTextResponse(text)
        return JSONResponse({"text": text})

    async def speech(request: Request) -> Response:
        if not declared_length_ok(request, MAX_JSON_BYTES):
            return error(413, "request body too large")
        body = bytearray()
        try:
            async for chunk in limited_stream(request, MAX_JSON_BYTES):
                body += chunk
        except BodyTooLarge:
            return error(413, "request body too large")
        try:
            data = json.loads(bytes(body))
        except ValueError:
            return error(400, "body must be valid JSON")
        if not isinstance(data, dict):
            return error(400, "body must be a JSON object")

        text = data.get("input")
        if not isinstance(text, str) or not text.strip():
            return error(400, "'input' must be a non-empty string")
        if len(text) > MAX_SPEECH_CHARS:
            return error(400, f"'input' is longer than {MAX_SPEECH_CHARS} characters")
        voice = data.get("voice", engine.default_voice)
        if not isinstance(voice, str) or voice not in engine.voices:
            return error(400, "unknown voice")
        fmt = data.get("response_format", "wav")
        if fmt != "wav":
            return error(400, "only response_format 'wav' is supported")
        if "model" in data and not isinstance(data["model"], str):
            return error(400, "'model' must be a string")

        audio = await run_model(engine.speak, text, voice)
        if audio is None:
            return error(400, "nothing speakable in 'input'")
        return Response(audio, media_type="audio/wav")

    async def http_error(_: Request, exc: HTTPException) -> Response:
        return error(exc.status_code, {404: "not found", 405: "method not allowed"}.get(
            exc.status_code, "request failed"))

    async def internal_error(_: Request, exc: Exception) -> Response:
        # Details go to stderr only; never echo internals or request content.
        print(f"unhandled {type(exc).__name__}: {exc}", file=sys.stderr, flush=True)
        return error(500, "internal error")

    routes = [Route("/health", health, methods=["GET"])]
    if engine.name in STT_BACKENDS:
        routes.append(
            Route("/v1/audio/transcriptions", transcriptions, methods=["POST"])
        )
    else:
        routes.append(Route("/v1/audio/speech", speech, methods=["POST"]))

    inner = Starlette(
        routes=routes,
        exception_handlers={HTTPException: http_error, Exception: internal_error},
    )

    async def app(scope, receive, send):
        """Pure ASGI bearer-token gate in front of every route but /health."""
        if scope["type"] == "http" and scope["path"] != "/health":
            header = Headers(scope=scope).get("authorization", "")
            scheme, _, supplied = header.partition(" ")
            if scheme.lower() != "bearer" or not hmac.compare_digest(
                supplied.strip().encode(), token_bytes
            ):
                response = JSONResponse(
                    {"error": {"message": "missing or invalid bearer token"}},
                    status_code=401,
                    headers={"WWW-Authenticate": "Bearer"},
                )
                await response(scope, receive, send)
                return
        await inner(scope, receive, send)

    return app


# -------------------------------------------------------------------- CLI


def serve(backend: str, host: str, port: int) -> None:
    if host not in LOOPBACK_HOSTS:
        fail("--host must be 127.0.0.1 or ::1", 2)
    token = os.environ.get("DUSK_VOICE_TOKEN", "")
    if len(token) < MIN_TOKEN_LEN:
        fail(f"DUSK_VOICE_TOKEN must be set to at least {MIN_TOKEN_LEN} characters", 2)

    import uvicorn

    sys.stdout = sys.stderr  # mlx-audio prints load progress with print()
    # MLX streams belong to the thread that made them, so one thread loads the
    # model and runs every call. Load before binding so a listening port always
    # means a ready model.
    worker = concurrent.futures.ThreadPoolExecutor(1, thread_name_prefix="model")
    engine: Any = worker.submit(ENGINES[backend]).result()
    app = make_app(engine, token, worker)

    class ReadyServer(uvicorn.Server):
        async def startup(self, sockets=None):
            await super().startup(sockets)
            if self.started:
                bound = self.servers[0].sockets[0].getsockname()[1]
                log_json(stage="ready", port=bound)

    config = uvicorn.Config(
        app,
        host=host,
        port=port,
        log_level="warning",  # access logs would carry request paths
        access_log=False,
        server_header=False,
        date_header=False,
        h11_max_incomplete_event_size=16 * 1024,
    )
    ReadyServer(config).run()


def main() -> None:
    if sys.argv[1:] == ["--version"]:
        print(PROTOCOL_VERSION)
        return
    parser = argparse.ArgumentParser(prog="server.py")
    sub = parser.add_subparsers(dest="cmd", required=True)
    p_serve = sub.add_parser("serve")
    p_serve.add_argument("--backend", choices=BACKENDS, required=True)
    p_serve.add_argument("--host", default="127.0.0.1")
    p_serve.add_argument("--port", type=int, required=True)
    p_down = sub.add_parser("download")
    p_down.add_argument("--backend", choices=BACKENDS, required=True)
    args = parser.parse_args()

    try:
        if args.cmd == "serve":
            serve(args.backend, args.host, args.port)
        else:
            download(args.backend)
    except SystemExit:
        raise
    except Exception as exc:
        fail(f"{args.cmd} failed: {type(exc).__name__}: {exc}")


if __name__ == "__main__":
    main()
