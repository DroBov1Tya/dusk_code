"""Local OpenAI-style audio server: one model per process.

    python server.py serve --backend whisper|kokoro --host 127.0.0.1 --port N
    python server.py download --backend whisper|kokoro
    python server.py --version

Pinned packages per backend (installed by the app into a uv venv):
    common:  starlette==1.10.0 uvicorn==0.54.0 python-multipart==0.0.33
    whisper: faster-whisper==1.2.1 av==18.1.0   (av 19 breaks faster-whisper 1.2.1)
    kokoro:  kokoro==0.9.4 soundfile==0.14.0 and the en_core_web_sm 3.8.0 wheel
             (otherwise misaki tries to pip-download it at runtime)
"""

import os

# The xet downloader stalls on some networks; plain HTTP works.
os.environ.setdefault("HF_HUB_DISABLE_XET", "1")

import argparse
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

BACKENDS = ("whisper", "kokoro")
LOOPBACK_HOSTS = ("127.0.0.1", "::1")

WHISPER_REPO = "dropbox-dash/faster-whisper-large-v3-turbo"
KOKORO_REPO = "hexgrad/Kokoro-82M"
KOKORO_SAMPLE_RATE = 24000
KOKORO_DEFAULT_VOICE = "af_heart"
# The pipeline is built for American English ("a"), so only a* voices fit it.
KOKORO_VOICE_PREFIX = "a"
KOKORO_ALLOW_PATTERNS = ["config.json", "kokoro-v1_0.pth", "voices/a*.pt"]

MIN_TOKEN_LEN = 16
MAX_UPLOAD_BYTES = 25 * 1024 * 1024
# Multipart framing and the small text fields on top of the file itself.
MAX_MULTIPART_OVERHEAD = 64 * 1024
MAX_JSON_BYTES = 64 * 1024
MAX_SPEECH_CHARS = 4096
LANGUAGE_RE = re.compile(r"[a-z]{2,3}")
# espeak-ng aborts the process on data paths longer than about 160 chars.
ESPEAK_MAX_PATH = 120


def log_json(**fields: Any) -> None:
    print(json.dumps(fields), flush=True)


def fail(message: str, code: int = 1) -> "NoReturn":  # noqa: F821
    print(message, file=sys.stderr, flush=True)
    sys.exit(code)


# ---------------------------------------------------------------- download


def download(backend: str) -> None:
    from huggingface_hub import snapshot_download

    repo = WHISPER_REPO if backend == "whisper" else KOKORO_REPO
    patterns = KOKORO_ALLOW_PATTERNS if backend == "kokoro" else None
    log_json(stage="download", repo=repo)
    snapshot_download(
        repo, allow_patterns=patterns, token=os.environ.get("HF_TOKEN") or None
    )
    log_json(stage="done")


def local_snapshot(repo: str, patterns: list[str] | None = None) -> str:
    """Path of the cached snapshot; never touches the network.

    The patterns must match the download, or hub 2.x calls the partial snapshot incomplete.
    """
    from huggingface_hub import snapshot_download

    return snapshot_download(repo, allow_patterns=patterns, local_files_only=True)


# ---------------------------------------------------------------- backends


class WhisperEngine:
    name = "whisper"

    def __init__(self) -> None:
        from faster_whisper import WhisperModel

        self._model = WhisperModel(
            local_snapshot(WHISPER_REPO), device="cpu", compute_type="float32"
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

        snapshot = local_snapshot(KOKORO_REPO, KOKORO_ALLOW_PATTERNS)
        self.voices = frozenset(
            f[:-3]
            for f in os.listdir(os.path.join(snapshot, "voices"))
            if f.endswith(".pt") and f.startswith(KOKORO_VOICE_PREFIX)
        )
        if KOKORO_DEFAULT_VOICE not in self.voices:
            raise RuntimeError("default voice missing from the local model snapshot")
        self._pipe = KPipeline(lang_code="a", repo_id=KOKORO_REPO, device="cpu")
        self._lock = threading.Lock()

    def speak(self, text: str, voice: str) -> bytes | None:
        import numpy as np
        import soundfile

        with self._lock:
            chunks = [
                audio.numpy()
                for _, _, audio in self._pipe(text, voice=voice)
                if audio is not None
            ]
        if not chunks:
            return None
        buf = io.BytesIO()
        soundfile.write(
            buf, np.concatenate(chunks), KOKORO_SAMPLE_RATE, format="WAV"
        )
        return buf.getvalue()


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


# ------------------------------------------------------------------- HTTP


def make_app(engine: Any, token: str):
    import anyio.to_thread
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
                text = await anyio.to_thread.run_sync(
                    engine.transcribe, tmp.name, language
                )
            except Exception as exc:
                # av raises on undecodable audio; that is the caller's fault.
                if type(exc).__module__.split(".")[0] == "av":
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
        voice = data.get("voice", KOKORO_DEFAULT_VOICE)
        if not isinstance(voice, str) or voice not in engine.voices:
            return error(400, "unknown voice")
        fmt = data.get("response_format", "wav")
        if fmt != "wav":
            return error(400, "only response_format 'wav' is supported")
        if "model" in data and not isinstance(data["model"], str):
            return error(400, "'model' must be a string")

        audio = await anyio.to_thread.run_sync(engine.speak, text, voice)
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
    if engine.name == "whisper":
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

    # Load before binding so a listening port always means a ready model.
    engine: Any = WhisperEngine() if backend == "whisper" else KokoroEngine()
    app = make_app(engine, token)

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
