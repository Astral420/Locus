#!/usr/bin/env python3
"""Correctness and speed qualification for the patched llama.cpp engine.

Two modes:

* Real run (L5, Intel Mac with the AMD GPU): launch `llama-server` with the
  document 04 flag sets and run gates Q1-Q7 plus an optional `llama-bench`.
* `--self-test` (L1, any machine): run the same gate code against a built-in
  fake HTTP server, then prove that deliberately broken fakes fail the
  matching gates.

Correctness gates block a release; speed is recorded, never gated.
Python 3.9+, standard library only.
"""

import argparse
import contextlib
import datetime
import json
import math
import os
import platform
import re
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

STARTUP_TIMEOUT = 150.0
CHAT_TIMEOUT = 180.0
EMBED_TIMEOUT = 120.0
CONTEXT_SIZE = 4096
DEFAULT_EMBED_DIM = 384
SIGTERM_GRACE = 5.0

PROMPTS = [
    "What is the capital of France?",
    "Name three colors.",
    "Explain in one sentence why the sky is blue.",
    "Write a short greeting for a coworker.",
    "Describe a rainy morning in one sentence.",
]

# Five triples plus five extra sentences: 20 fixed sentences for Q4/Q5/Q6.
TRIPLES = [
    (
        "A man is playing a guitar on stage.",
        "A musician performs with a guitar.",
        "The stock market closed lower today.",
    ),
    (
        "The cat is sleeping on the sofa.",
        "A kitten rests on the couch.",
        "Quantum computers use qubits.",
    ),
    (
        "She is drinking a cup of coffee.",
        "A woman drinks coffee from a mug.",
        "The bridge was built in 1932.",
    ),
    (
        "The car is parked in the driveway.",
        "A vehicle is sitting in the driveway.",
        "Photosynthesis occurs in plant cells.",
    ),
    (
        "Two dogs are running through the grass.",
        "A pair of dogs runs across a field.",
        "The recipe calls for two eggs.",
    ),
]
EXTRA_SENTENCES = [
    "The train arrives at the station at noon.",
    "A chef is preparing dinner in the kitchen.",
    "The child is reading a book by the window.",
    "Rain is falling on the city streets.",
    "A group of people is waiting at the bus stop.",
]
SENTENCES = [sentence for triple in TRIPLES for sentence in triple] + EXTRA_SENTENCES

# The fake chat replies have nine alphabetic words each, so they pass the Q2
# sanity checks and are deterministic for the Q3 comparison.
FAKE_CHAT_RESPONSES = [
    "The quick brown fox jumps over the lazy dog.",
    "A small green bird sings near the open window.",
    "Fresh coffee smells warm in the quiet kitchen.",
    "The old train rolls slowly past the station.",
    "Bright morning light falls across the wooden table.",
]

# Lexical groups used only by the self-test fake embeddings. Real embeddings
# are never interpreted this way; the fake just needs deterministic vectors
# where the five Q4 triples rank correctly.
FAKE_GROUPS = [
    {"guitar", "musician", "stage", "performs", "playing", "man"},
    {"cat", "kitten", "sofa", "couch", "sleeping", "rests"},
    {"coffee", "cup", "mug", "drinks", "drinking", "woman"},
    {"car", "vehicle", "driveway", "parked", "sitting"},
    {"dogs", "dog", "running", "runs", "grass", "field", "pair"},
]


class GateFailure(Exception):
    pass


class InvalidUtf8(Exception):
    pass


class ProtocolError(Exception):
    pass


class HttpFailure(Exception):
    def __init__(self, message, status=None):
        super().__init__(message)
        self.status = status


class GateResult:
    def __init__(self, gate, status, reason, extra=None):
        self.gate = gate
        self.status = status
        self.reason = reason
        self.extra = extra

    def line(self):
        return "%-16s %-4s %s" % (self.gate, self.status, self.reason)


def status_failed(results):
    return any(result.status == "FAIL" for result in results)


def post_json(url, payload, timeout):
    data = json.dumps(payload).encode("utf-8")
    request = urllib.request.Request(
        url,
        data=data,
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            body = response.read()
    except urllib.error.HTTPError as exc:
        body = exc.read()
        snippet = body.decode("utf-8", "replace")[:200]
        raise HttpFailure("HTTP %d: %s" % (exc.code, snippet), status=exc.code)
    except urllib.error.URLError as exc:
        raise HttpFailure("request failed: %s" % (exc.reason,))
    try:
        text = body.decode("utf-8")
    except UnicodeDecodeError as exc:
        raise InvalidUtf8(str(exc))
    try:
        return json.loads(text)
    except json.JSONDecodeError as exc:
        raise ProtocolError("invalid JSON: %s" % (exc,))


def get_json(url, timeout):
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            body = response.read()
    except urllib.error.HTTPError as exc:
        raise HttpFailure("HTTP %d" % exc.code, status=exc.code)
    except urllib.error.URLError as exc:
        raise HttpFailure("request failed: %s" % (exc.reason,))
    try:
        return json.loads(body.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise ProtocolError("invalid health response: %s" % (exc,))


def chat_once(base_url, prompt, timeout=CHAT_TIMEOUT):
    payload = {
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0,
        "max_tokens": 24,
        "stream": False,
    }
    data = post_json(base_url + "/v1/chat/completions", payload, timeout)
    if not isinstance(data, dict):
        raise ProtocolError("chat response is not an object")
    choices = data.get("choices")
    if not isinstance(choices, list) or not choices or not isinstance(choices[0], dict):
        raise ProtocolError("chat response has no choices")
    content = choices[0].get("message", {}).get("content")
    if content is None:
        content = choices[0].get("text")
    if not isinstance(content, str):
        raise ProtocolError("chat response has no text content")
    return content


def _extract_embedding(data):
    if not isinstance(data, dict):
        raise ProtocolError("embedding response is not an object")
    embedding = None
    items = data.get("data")
    if isinstance(items, list) and items and isinstance(items[0], dict):
        embedding = items[0].get("embedding")
    if embedding is None:
        embedding = data.get("embedding")
    if isinstance(embedding, list) and embedding and isinstance(embedding[0], list):
        embedding = embedding[0]
    if not isinstance(embedding, list):
        raise ProtocolError("embedding response has no vector")
    return embedding


def embed_once(base_url, sentence, timeout=EMBED_TIMEOUT):
    try:
        data = post_json(
            base_url + "/v1/embeddings",
            {"input": sentence, "model": "embedding"},
            timeout,
        )
    except HttpFailure as exc:
        if exc.status != 404:
            raise
        data = post_json(base_url + "/embedding", {"content": sentence}, timeout)
    return _extract_embedding(data)


def alphabetic_words(text):
    return re.findall(r"[^\W\d_]+", text, re.UNICODE)


def first_words(text, count):
    return [word.lower() for word in alphabetic_words(text)[:count]]


def chat_problem(text):
    if not isinstance(text, str):
        return "response content is not text"
    stripped = text.strip()
    if not stripped:
        return "empty response"
    words = alphabetic_words(stripped)
    if len({word.lower() for word in words}) < 3:
        return "fewer than 3 distinct alphabetic words"
    counts = {}
    for word in words:
        key = word.lower()
        counts[key] = counts.get(key, 0) + 1
    top_word, top_count = max(counts.items(), key=lambda item: item[1])
    if top_count > 0.60 * len(words):
        return "more than 60%% of words are %r (%d/%d)" % (
            top_word,
            top_count,
            len(words),
        )
    return None


def vector_problem(vector, expected_dim):
    if not isinstance(vector, list):
        return "embedding is not a list"
    if len(vector) != expected_dim:
        return "dimension %d != %d" % (len(vector), expected_dim)
    for value in vector:
        if isinstance(value, bool) or not isinstance(value, (int, float)):
            return "embedding contains a non-numeric value"
        if not math.isfinite(value):
            return "embedding contains a non-finite value"
    return None


def cosine(left, right):
    if len(left) != len(right):
        raise ValueError("dimension mismatch")
    dot = sum(a * b for a, b in zip(left, right))
    norm_left = math.sqrt(sum(a * a for a in left))
    norm_right = math.sqrt(sum(b * b for b in right))
    if norm_left == 0.0 or norm_right == 0.0:
        return 0.0
    return dot / (norm_left * norm_right)


def percentile(values, percent):
    ordered = sorted(values)
    if not ordered:
        return 0.0
    index = math.ceil(percent / 100.0 * len(ordered)) - 1
    index = max(0, min(index, len(ordered) - 1))
    return ordered[index]


def free_port():
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class LlamaServer:
    """A real llama-server child process."""

    def __init__(self, binary, model, role, device, log_path):
        self.binary = binary
        self.model = model
        self.role = role
        self.device = device
        self.log_path = log_path
        self.port = free_port()
        self.process = None
        self.log_handle = None
        self.last_health_seconds = None

    @property
    def base_url(self):
        return "http://127.0.0.1:%d" % self.port

    def _args(self):
        args = [
            self.binary,
            "-m",
            self.model,
            "--host",
            "127.0.0.1",
            "--port",
            str(self.port),
        ]
        if self.role == "embedding":
            args.append("--embedding")
        if self.device == "gpu":
            args.extend(["-ngl", "99", "-fa", "auto", "-c", str(CONTEXT_SIZE)])
        else:
            args.extend(["-ngl", "0"])
        return args

    def _env(self):
        env = {
            key: value
            for key, value in os.environ.items()
            if not key.startswith("TOSH_") and not key.startswith("GGML_METAL_")
        }
        if self.device == "gpu":
            env["TOSH_FA_AMD"] = "1"
        return env

    def start(self):
        self.log_handle = open(self.log_path, "wb")
        self.process = subprocess.Popen(
            self._args(),
            stdout=self.log_handle,
            stderr=subprocess.STDOUT,
            env=self._env(),
            cwd=os.path.dirname(os.path.abspath(self.binary)),
        )

    def wait_health(self, timeout):
        begin = time.monotonic()
        while time.monotonic() - begin < timeout:
            if self.process is not None and self.process.poll() is not None:
                self.last_health_seconds = time.monotonic() - begin
                return (
                    False,
                    "server exited with code %s before /health"
                    % self.process.returncode,
                )
            try:
                get_json(self.base_url + "/health", timeout=2.0)
                self.last_health_seconds = time.monotonic() - begin
                return True, ""
            except Exception:
                pass
            time.sleep(0.25)
        self.last_health_seconds = time.monotonic() - begin
        return False, "no /health within %.1f s" % timeout

    def log_text(self):
        try:
            with open(self.log_path, "rb") as handle:
                return handle.read().decode("utf-8", "replace")
        except OSError:
            return ""

    def alive(self):
        return self.process is not None and self.process.poll() is None

    def terminate(self, timeout):
        if not self.alive():
            return True
        try:
            self.process.send_signal(signal.SIGTERM)
        except ProcessLookupError:
            return True
        try:
            self.process.wait(timeout)
            return True
        except subprocess.TimeoutExpired:
            return False

    def stop(self):
        if self.alive():
            try:
                self.process.terminate()
                self.process.wait(2.0)
            except (ProcessLookupError, subprocess.TimeoutExpired):
                try:
                    self.process.kill()
                    self.process.wait(2.0)
                except (ProcessLookupError, subprocess.TimeoutExpired):
                    pass
        if self.log_handle is not None:
            try:
                self.log_handle.close()
            except OSError:
                pass
            self.log_handle = None


class FakeState:
    def __init__(self, mode, embed_dim, gpu_name, device):
        self.mode = mode
        self.embed_dim = embed_dim
        self.gpu_name = gpu_name
        self.device = device
        self.release = threading.Event()
        self.delay = 0.01
        self.log_text = (
            "fake llama-server\n"
            "main: using device %s\n"
            "main: server is listening\n" % gpu_name
        )


class FakeHandler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, format_string, *args):
        pass

    def _write_bytes(self, status, body, content_type="application/json"):
        try:
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (BrokenPipeError, ConnectionResetError):
            self.close_connection = True

    def _write_json(self, status, payload):
        self._write_bytes(status, json.dumps(payload).encode("utf-8"))

    def do_GET(self):
        state = self.server.state
        path = self.path.split("?", 1)[0]
        if path == "/health":
            if state.mode == "hung":
                state.release.wait()
            self._write_json(200, {"status": "ok"})
        else:
            self._write_json(404, {"error": "not found"})

    def do_POST(self):
        state = self.server.state
        path = self.path.split("?", 1)[0]
        length = int(self.headers.get("Content-Length") or 0)
        raw_body = self.rfile.read(length) if length else b""
        try:
            payload = json.loads(raw_body.decode("utf-8")) if raw_body else {}
        except (UnicodeDecodeError, json.JSONDecodeError):
            payload = {}
        time.sleep(state.delay)

        if path in ("/v1/chat/completions", "/chat/completions"):
            if state.mode == "garbage":
                self._write_bytes(200, b"\xff\xfe\xfa not utf-8")
                return
            if state.mode == "empty":
                content = ""
            else:
                messages = payload.get("messages") or []
                prompt = ""
                if messages and isinstance(messages[-1], dict):
                    prompt = str(messages[-1].get("content", ""))
                total = sum(ord(char) for char in prompt)
                content = FAKE_CHAT_RESPONSES[total % len(FAKE_CHAT_RESPONSES)]
            self._write_json(
                200,
                {
                    "choices": [
                        {
                            "index": 0,
                            "message": {"role": "assistant", "content": content},
                            "finish_reason": "stop",
                        }
                    ]
                },
            )
            return

        if path in ("/v1/embeddings", "/embedding"):
            source = payload.get("input", payload.get("content", ""))
            texts = source if isinstance(source, list) else [source]
            data = []
            for index, text in enumerate(texts):
                vector = fake_embedding(
                    str(text), state.embed_dim, state.mode, state.device
                )
                data.append(
                    {"object": "embedding", "index": index, "embedding": vector}
                )
            if path == "/embedding" and len(data) == 1:
                self._write_json(200, {"embedding": data[0]["embedding"]})
            else:
                self._write_json(
                    200, {"object": "list", "data": data, "model": "fake"}
                )
            return

        self._write_json(404, {"error": "not found"})


def fake_embedding(text, embed_dim, mode, device):
    dim = embed_dim
    if mode == "wrong_dim":
        dim = max(1, embed_dim - 1)
    vector = [0.0] * dim
    words = set(re.findall(r"[a-z]+", text.lower()))
    group = 9
    for index, keywords in enumerate(FAKE_GROUPS):
        if words & keywords:
            group = index
            break
    if group >= dim:
        group = dim - 1
    vector[group] = 1.0
    if mode == "drifting" and device == "gpu" and dim > 1:
        drift = (group + 1) % dim
        vector[drift] += 0.1
    return vector


class FakeServer:
    """An in-process HTTP server that stands in for llama-server in --self-test."""

    def __init__(self, state):
        self.state = state
        self.httpd = None
        self.thread = None
        self.port = None
        self._alive = False
        self.last_health_seconds = None

    @property
    def base_url(self):
        return "http://127.0.0.1:%d" % self.port

    def start(self):
        self.httpd = ThreadingHTTPServer(("127.0.0.1", 0), FakeHandler)
        self.httpd.state = self.state
        self.httpd.daemon_threads = True
        self.port = self.httpd.server_address[1]
        self.thread = threading.Thread(
            target=self.httpd.serve_forever, daemon=True
        )
        self.thread.start()
        self._alive = True

    def wait_health(self, timeout):
        begin = time.monotonic()
        while time.monotonic() - begin < timeout:
            try:
                get_json(self.base_url + "/health", timeout=0.5)
                self.last_health_seconds = time.monotonic() - begin
                return True, ""
            except Exception:
                pass
            time.sleep(0.05)
        self.last_health_seconds = time.monotonic() - begin
        return False, "no /health within %.1f s" % timeout

    def log_text(self):
        return self.state.log_text

    def alive(self):
        return self._alive

    def terminate(self, timeout):
        self.stop()
        return True

    def stop(self):
        if self.httpd is not None:
            self.state.release.set()
            try:
                self.httpd.shutdown()
            except Exception:
                pass
            try:
                self.httpd.server_close()
            except Exception:
                pass
            self.httpd = None
        self._alive = False


class RealBackend:
    def __init__(self, binary, chat_model, embed_model, log_dir):
        self.binary = binary
        self.chat_model = chat_model
        self.embed_model = embed_model
        self.log_dir = log_dir
        self.counter = 0

    def _server(self, role, device, model):
        self.counter += 1
        log_path = os.path.join(
            self.log_dir, "%s-%s-%02d.log" % (role, device, self.counter)
        )
        return LlamaServer(self.binary, model, role, device, log_path)

    def chat(self, device):
        return self._server("chat", device, self.chat_model)

    def embed(self, device):
        return self._server("embedding", device, self.embed_model)


class FakeBackend:
    def __init__(self, modes=None, embed_dim=DEFAULT_EMBED_DIM, gpu_name="RX 6600"):
        self.modes = modes or {}
        self.embed_dim = embed_dim
        self.gpu_name = gpu_name
        self.counter = 0

    def _mode(self, role, device):
        return self.modes.get(
            (role, device),
            self.modes.get(role, self.modes.get("all", "healthy")),
        )

    def _server(self, role, device):
        self.counter += 1
        mode = self._mode(role, device)
        state = FakeState(mode, self.embed_dim, self.gpu_name, device)
        return FakeServer(state)

    def chat(self, device):
        return self._server("chat", device)

    def embed(self, device):
        return self._server("embed", device)


@contextlib.contextmanager
def running(server, timeout=STARTUP_TIMEOUT):
    server.start()
    try:
        ok, reason = server.wait_health(timeout)
        if not ok:
            raise GateFailure(reason)
        yield server
    finally:
        server.stop()


def gate_q1(backend, gpu_name, timeout=STARTUP_TIMEOUT):
    name = "Q1 start"
    try:
        server = backend.chat("gpu")
        try:
            server.start()
            ok, reason = server.wait_health(timeout)
            if not ok:
                return GateResult(name, "FAIL", reason)
            seconds = server.last_health_seconds or 0.0
            log = server.log_text()
            if not gpu_name:
                return GateResult(
                    name,
                    "WARN",
                    "health in %.1f s; no --gpu-name supplied to confirm the device"
                    % seconds,
                )
            if gpu_name.lower() in log.lower():
                return GateResult(
                    name,
                    "PASS",
                    "health in %.1f s; log names %s" % (seconds, gpu_name),
                )
            return GateResult(
                name,
                "WARN",
                "health in %.1f s; log does not mention %s" % (seconds, gpu_name),
            )
        finally:
            server.stop()
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q2(backend):
    name = "Q2 chat sane"
    try:
        with running(backend.chat("gpu")) as server:
            for index, prompt in enumerate(PROMPTS, 1):
                try:
                    text = chat_once(server.base_url, prompt)
                except InvalidUtf8:
                    return GateResult(
                        name, "FAIL", "prompt %d: invalid UTF-8 in response" % index
                    )
                except Exception as exc:
                    return GateResult(
                        name, "FAIL", "prompt %d: %s" % (index, exc)
                    )
                problem = chat_problem(text)
                if problem is not None:
                    return GateResult(
                        name, "FAIL", "prompt %d: %s" % (index, problem)
                    )
        return GateResult(
            name, "PASS", "5 prompts, temperature 0, max_tokens 24, GPU"
        )
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q3(backend):
    name = "Q3 chat vs CPU"
    try:
        with running(backend.chat("gpu")) as gpu:
            gpu_texts = [chat_once(gpu.base_url, prompt) for prompt in PROMPTS]
        with running(backend.chat("cpu")) as cpu:
            cpu_texts = [chat_once(cpu.base_url, prompt) for prompt in PROMPTS]
        matches = 0
        for gpu_text, cpu_text in zip(gpu_texts, cpu_texts):
            gpu_words = first_words(gpu_text, 6)
            cpu_words = first_words(cpu_text, 6)
            if gpu_words and gpu_words == cpu_words:
                matches += 1
        if matches < 3:
            return GateResult(
                name,
                "FAIL",
                "%d/5 prompts share the first 6 words (need 3)" % matches,
            )
        return GateResult(
            name, "PASS", "%d/5 prompts share the first 6 words" % matches
        )
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q4(backend, embed_dim):
    name = "Q4 embeddings"
    try:
        with running(backend.embed("cpu")) as server:
            vectors = {}
            for index, sentence in enumerate(SENTENCES, 1):
                try:
                    vector = embed_once(server.base_url, sentence)
                except Exception as exc:
                    return GateResult(
                        name, "FAIL", "sentence %d: %s" % (index, exc)
                    )
                problem = vector_problem(vector, embed_dim)
                if problem is not None:
                    return GateResult(
                        name, "FAIL", "sentence %d: %s" % (index, problem)
                    )
                vectors[sentence] = vector
        for anchor, similar, unrelated in TRIPLES:
            close = cosine(vectors[anchor], vectors[similar])
            far = cosine(vectors[anchor], vectors[unrelated])
            if close <= far:
                return GateResult(
                    name,
                    "FAIL",
                    "triple misranked: similar %.6f <= unrelated %.6f (%r)"
                    % (close, far, anchor),
                )
        return GateResult(
            name,
            "PASS",
            "20 sentences, dimension %d; 5 triples ranked correctly" % embed_dim,
        )
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q5(backend, embed_dim):
    name = "Q5 embeddings GPU"
    try:
        with running(backend.embed("cpu")) as cpu:
            cpu_vectors = [embed_once(cpu.base_url, s) for s in SENTENCES]
        with running(backend.embed("gpu")) as gpu:
            gpu_vectors = [embed_once(gpu.base_url, s) for s in SENTENCES]
        worst = 1.0
        worst_sentence = ""
        for index, sentence in enumerate(SENTENCES):
            problem = vector_problem(cpu_vectors[index], embed_dim)
            if problem is not None:
                return GateResult(
                    name, "FAIL", "CPU sentence %d: %s" % (index + 1, problem)
                )
            problem = vector_problem(gpu_vectors[index], embed_dim)
            if problem is not None:
                return GateResult(
                    name, "FAIL", "GPU sentence %d: %s" % (index + 1, problem)
                )
            similarity = cosine(cpu_vectors[index], gpu_vectors[index])
            if similarity < worst:
                worst = similarity
                worst_sentence = sentence
        if worst < 0.999:
            return GateResult(
                name,
                "FAIL",
                "cosine %.6f < 0.999 for %r" % (worst, worst_sentence),
            )
        return GateResult(
            name, "PASS", "20 sentences, minimum cosine %.6f" % worst
        )
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q6(backend, embed_dim):
    name = "Q6 soak"
    try:
        with running(backend.chat("gpu")) as chat:
            with running(backend.embed("gpu")) as embed:
                latencies = []
                for index in range(50):
                    prompt = PROMPTS[index % len(PROMPTS)]
                    begin = time.perf_counter()
                    try:
                        text = chat_once(chat.base_url, prompt)
                    except Exception as exc:
                        return GateResult(
                            name, "FAIL", "chat %d failed: %s" % (index + 1, exc)
                        )
                    latencies.append(time.perf_counter() - begin)
                    problem = chat_problem(text)
                    if problem is not None:
                        return GateResult(
                            name,
                            "FAIL",
                            "chat %d: %s" % (index + 1, problem),
                        )
                for index in range(200):
                    sentence = SENTENCES[index % len(SENTENCES)]
                    begin = time.perf_counter()
                    try:
                        vector = embed_once(embed.base_url, sentence)
                    except Exception as exc:
                        return GateResult(
                            name,
                            "FAIL",
                            "embedding %d failed: %s" % (index + 1, exc),
                        )
                    latencies.append(time.perf_counter() - begin)
                    problem = vector_problem(vector, embed_dim)
                    if problem is not None:
                        return GateResult(
                            name,
                            "FAIL",
                            "embedding %d: %s" % (index + 1, problem),
                        )
        median = statistics.median(latencies)
        p95 = percentile(latencies, 95)
        detail = "250 requests; median %.3f s, p95 %.3f s" % (median, p95)
        if p95 > 3.0 * median:
            return GateResult(name, "WARN", detail + " (p95 > 3x median)")
        return GateResult(name, "PASS", detail)
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def gate_q7(backend):
    name = "Q7 stop"
    try:
        server = backend.chat("gpu")
        try:
            with running(server):
                if not server.alive():
                    return GateResult(name, "FAIL", "server was not running")
                stopped = server.terminate(SIGTERM_GRACE)
                if not stopped or server.alive():
                    return GateResult(
                        name,
                        "FAIL",
                        "still alive %.0f s after SIGTERM" % SIGTERM_GRACE,
                    )
                return GateResult(name, "PASS", "process gone after SIGTERM")
        finally:
            server.stop()
    except Exception as exc:
        return GateResult(name, "FAIL", "error: %s" % (exc,))


def parse_bench(output):
    rows = []
    for line in output.splitlines():
        if "pp512" not in line and "tg128" not in line:
            continue
        cells = [cell.strip() for cell in line.strip().strip("|").split("|")]
        if len(cells) < 2 or cells[-2] not in ("pp512", "tg128"):
            continue
        match = re.search(r"([0-9]+(?:\.[0-9]+)?)", cells[-1])
        if match:
            rows.append((cells[-2], float(match.group(1))))
    return rows


def gate_bench(bench_binary, chat_model, timeout=900):
    name = "Bench"
    if not bench_binary:
        return GateResult(name, "SKIP", "no --bench supplied")
    if not os.path.isfile(bench_binary):
        return GateResult(name, "WARN", "no llama-bench at %s" % bench_binary)
    entries = []
    warnings = []
    for label, device_args in (("GPU", ["-ngl", "99"]), ("CPU", ["-ngl", "0"])):
        command = [
            bench_binary,
            "-m",
            chat_model,
            "-p",
            "512",
            "-n",
            "128",
        ] + device_args
        try:
            process = subprocess.run(
                command,
                stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT,
                timeout=timeout,
            )
            output = process.stdout.decode("utf-8", "replace")
        except Exception as exc:
            warnings.append("%s run failed: %s" % (label, exc))
            continue
        rows = parse_bench(output)
        entries.append({"label": label, "rows": rows, "output": output})
        if process.returncode != 0:
            warnings.append("%s run exited with %d" % (label, process.returncode))
        if not rows:
            warnings.append("%s run produced no pp512/tg128 numbers" % label)
    parts = []
    for entry in entries:
        for test, rate in entry["rows"]:
            parts.append("%s %s %.2f t/s" % (entry["label"], test, rate))
    detail = "; ".join(parts + warnings) if (parts or warnings) else "no numbers"
    if warnings:
        return GateResult(name, "WARN", detail, extra=entries)
    return GateResult(name, "PASS", detail, extra=entries)


def run_gates(backend, embed_dim, bench_binary, chat_model, gpu_name):
    return [
        gate_q1(backend, gpu_name),
        gate_q2(backend),
        gate_q3(backend),
        gate_q4(backend, embed_dim),
        gate_q5(backend, embed_dim),
        gate_q6(backend, embed_dim),
        gate_q7(backend),
        gate_bench(bench_binary, chat_model),
    ]


def engine_pin(binary):
    path = os.path.join(os.path.dirname(os.path.abspath(binary)), "engine.json")
    try:
        with open(path, "r", encoding="utf-8") as handle:
            data = json.load(handle)
    except (OSError, ValueError):
        return "unknown"
    return "toshllm %s (%s), llama.cpp %s" % (
        data.get("toshllm_tag", "?"),
        data.get("toshllm_commit", "?")[:12],
        data.get("llama_commit", "?")[:12],
    )


def write_report(path, binary, gpu_name, results, backend):
    lines = [
        "# Intel Mac + AMD llama.cpp qualification report",
        "",
        "- Machine: %s" % platform.node(),
        "- macOS: %s" % (platform.mac_ver()[0] or platform.platform()),
        "- GPU: %s" % (gpu_name or "unknown"),
        "- Engine pin: %s" % engine_pin(binary),
        "- Date: %s"
        % datetime.datetime.now(datetime.timezone.utc).strftime(
            "%Y-%m-%d %H:%M UTC"
        ),
        "",
        "Correctness gates block a release; speed is recorded, never gated.",
        "",
        "## Gates",
        "",
        "| Gate | Status | Detail |",
        "|---|---|---|",
    ]
    for result in results:
        lines.append(
            "| %s | %s | %s |"
            % (result.gate, result.status, result.reason.replace("|", "\\|"))
        )
    lines.extend(
        [
            "",
            "## Bench",
            "",
            "Tokens per second from `llama-bench -p 512 -n 128` (optional, never gated).",
            "",
            "| Run | Test | Tokens/s |",
            "|---|---|---|",
        ]
    )
    benches = [result for result in results if result.gate == "Bench"]
    for result in benches:
        if result.status == "SKIP":
            lines.append("| - | - | not run |")
        for entry in result.extra or []:
            for test, rate in entry["rows"]:
                lines.append("| %s | %s | %.2f |" % (entry["label"], test, rate))
    lines.extend(
        [
            "",
            "## Logs",
            "",
            "Server stderr logs: `%s`" % getattr(backend, "log_dir", "unknown"),
            "",
            "Paste this report into `docs/release/LLAMA_METAL_AMD.md` under Results.",
            "",
        ]
    )
    directory = os.path.dirname(os.path.abspath(path))
    if directory and not os.path.isdir(directory):
        os.makedirs(directory)
    with open(path, "w", encoding="utf-8") as handle:
        handle.write("\n".join(lines))


def expect_fail(label, result):
    print(
        "  negative %-18s %-16s -> %-4s %s"
        % (label, result.gate, result.status, result.reason)
    )
    return result.status == "FAIL"


def run_self_test(args):
    embed_dim = args.embed_dim
    print("self-test: healthy fake server")
    backend = FakeBackend(embed_dim=embed_dim)
    results = run_gates(backend, embed_dim, None, None, backend.gpu_name)
    for result in results:
        print("  " + result.line())
    healthy = all(
        result.status == "PASS" for result in results if result.gate != "Bench"
    )

    print("self-test: broken fakes (each must FAIL the matching gate)")
    checks = [
        (
            "empty replies",
            gate_q2(FakeBackend(modes={("chat", "gpu"): "empty"}, embed_dim=embed_dim)),
        ),
        (
            "garbage text",
            gate_q2(FakeBackend(modes={("chat", "gpu"): "garbage"}, embed_dim=embed_dim)),
        ),
        (
            "wrong dimension",
            gate_q4(
                FakeBackend(modes={("embed", "cpu"): "wrong_dim"}, embed_dim=embed_dim),
                embed_dim,
            ),
        ),
        (
            "drifting vectors",
            gate_q5(
                FakeBackend(modes={("embed", "gpu"): "drifting"}, embed_dim=embed_dim),
                embed_dim,
            ),
        ),
        (
            "hung process",
            gate_q1(
                FakeBackend(modes={("chat", "gpu"): "hung"}, embed_dim=embed_dim),
                "RX 6600",
                timeout=1.0,
            ),
        ),
    ]
    negatives = [expect_fail(label, result) for label, result in checks]

    if healthy and all(negatives):
        print("self-test: PASS (Q1-Q7 pass on the healthy fake; all negative checks failed)")
        return 0
    print("self-test: FAIL")
    return 1


def parse_args(argv):
    parser = argparse.ArgumentParser(
        description=(
            "Qualify the patched llama.cpp engine on Intel Mac + AMD. "
            "Correctness gates block a release; speed is recorded."
        )
    )
    parser.add_argument("--binary", help="path to llama-server")
    parser.add_argument("--chat-model", help="path to the chat GGUF")
    parser.add_argument("--embed-model", help="path to the embedding GGUF")
    parser.add_argument(
        "--embed-dim",
        type=int,
        default=DEFAULT_EMBED_DIM,
        help="expected embedding dimension (default: %d)" % DEFAULT_EMBED_DIM,
    )
    parser.add_argument(
        "--gpu-name",
        default="",
        help="GPU name expected in the server log, e.g. \"RX 6600\"",
    )
    parser.add_argument("--bench", help="optional path to llama-bench")
    parser.add_argument("--out", help="write a Markdown report to this path")
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run against the built-in fake server (L1; no model or GPU)",
    )
    return parser.parse_args(argv)


def main(argv):
    args = parse_args(argv)
    if args.embed_dim <= 0:
        print("error: --embed-dim must be positive", file=sys.stderr)
        return 2
    if args.self_test:
        return run_self_test(args)

    missing = [
        flag
        for flag, value in (
            ("--binary", args.binary),
            ("--chat-model", args.chat_model),
            ("--embed-model", args.embed_model),
        )
        if not value
    ]
    if missing:
        print("error: missing %s" % ", ".join(missing), file=sys.stderr)
        return 2
    args.binary = os.path.abspath(os.path.expanduser(args.binary))
    args.chat_model = os.path.abspath(os.path.expanduser(args.chat_model))
    args.embed_model = os.path.abspath(os.path.expanduser(args.embed_model))
    if args.bench:
        args.bench = os.path.abspath(os.path.expanduser(args.bench))
    if args.out:
        args.out = os.path.abspath(os.path.expanduser(args.out))

    for label, path in (
        ("llama-server", args.binary),
        ("chat model", args.chat_model),
        ("embedding model", args.embed_model),
    ):
        if not os.path.isfile(path):
            print("error: no %s at %s" % (label, path), file=sys.stderr)
            return 2
    if not os.access(args.binary, os.X_OK):
        print("error: %s is not executable" % args.binary, file=sys.stderr)
        return 2

    log_dir = tempfile.mkdtemp(prefix="locus-qualify-logs-")
    backend = RealBackend(
        args.binary, args.chat_model, args.embed_model, log_dir
    )
    print("engine: %s" % args.binary)
    print("logs:   %s" % log_dir)
    results = run_gates(
        backend, args.embed_dim, args.bench, args.chat_model, args.gpu_name
    )
    print("")
    for result in results:
        print(result.line())
    if args.out:
        write_report(args.out, args.binary, args.gpu_name, results, backend)
        print("")
        print("report: %s" % args.out)
    return 1 if status_failed(results) else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
