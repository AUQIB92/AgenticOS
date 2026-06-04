# Gemini API Key Management

## Overview

AgenticOS uses Google Gemini as an optional LLM provider for workload classification
recommendations. The Gemini API key is a sensitive credential that must never be
committed to git, stored in SQLite, logged, serialized in config files, or exposed
in any output.

This document describes how to obtain, store, and use a Gemini API key securely
with AgenticOS.

---

## Obtaining a Gemini API Key

1. Go to [Google AI Studio](https://aistudio.google.com/app/apikey)
2. Click **Get API Key**
3. Select or create a Google Cloud project
4. Copy the key — it starts with `AIza` and is ~39 characters

---

## Setup Instructions

### Linux / WSL (Recommended)

Create a secrets file with restricted permissions:

```bash
mkdir -p ~/.config/agenticos
chmod 700 ~/.config/agenticos
echo 'GEMINI_API_KEY=YOUR_KEY_HERE' >> ~/.config/agenticos/secrets.env
chmod 600 ~/.config/agenticos/secrets.env
```

Source the file before running the daemon or CLI:

```bash
source ~/.config/agenticos/secrets.env
cargo run --bin agenticos-daemon
```

Or add the source line to your `~/.bashrc`:

```bash
echo 'source ~/.config/agenticos/secrets.env' >> ~/.bashrc
```

### Development Setup (`.env` file)

For local development, create a `.env` file in the workspace root:

```bash
echo 'GEMINI_API_KEY=YOUR_KEY_HERE' >> .env
```

AgenticOS uses `dotenvy` to automatically load `.env` at startup. The file is
already in `.gitignore` and will never be committed.

### Production Setup

In production, set the environment variable directly:

```bash
export GEMINI_API_KEY='YOUR_KEY_HERE'
```

For systemd services, use `EnvironmentFile`:

```ini
[Service]
EnvironmentFile=/etc/agenticos/secrets.env
```

---

## Configuration

In your `configs/dev.toml` (or equivalent), set the provider to `gemini`:

```toml
[intelligence]
provider = "gemini"
model = "gemini-2.5-flash"
api_key_env = "GEMINI_API_KEY"
timeout_seconds = 10
```

The `api_key_env` field specifies the **name** of the environment variable, not
the key itself. The actual key is read only from `std::env` at runtime.

---

## How It Works

```
┌──────────────────────────────────────────────────────────┐
│  IntelligenceConfig::create_provider()                    │
│                                                          │
│  1. Read provider_name from config ("gemini")            │
│  2. Read GEMINI_API_KEY from std::env                    │
│  3. If missing/empty → return Err with clear message     │
│  4. If present → create GeminiProvider                   │
│  5. Wrap in CachedLlmProvider (if cache available)       │
│  6. Return Box<dyn LlmProvider>                          │
└──────────────────────────────────────────────────────────┘
```

## Security Protections

| Layer | Protection |
|-------|-----------|
| **Environment only** | API key is never in TOML, SQLite, or TraceStore |
| **`#[derive(Debug)]`** | Custom `Debug` impl redacts the key — shows `AIza****…` |
| **`#[derive(Serialize)]`** | `GeminiProvider` has no `Serialize` derive; the `api_key` field is private |
| **Error messages** | All reqwest errors are sanitized — URL with `?key=` is never included |
| **`redact_secret()`** | Utility function that replaces `AIza…` with `AIza****…` in any string |
| **Logging** | `eprintln!` only receives sanitized error strings |
| **Runtime validation** | Missing or empty key returns a clear error — no silent fallback |
| **`dotenvy`** | Automatically loads `.env` at startup; `.env` is gitignored |
| **Secrets file** | `~/.config/agenticos/secrets.env` with `chmod 600` |

---

## Threat Model

### Assets Protected

- Gemini API key (enables usage of the Gemini API)

### Threats

| Threat | Mitigation |
|--------|-----------|
| Key committed to git | `.env` in `.gitignore`; `secrets.env` is outside the repo |
| Key leaked in logs | `call_gemini` redacts errors; `Debug` redacts the key |
| Key stored in SQLite | Config stores only the env var **name**, never the key value |
| Key in serialized output | `GeminiProvider` has no `Serialize` impl |
| Key in TraceStore replay | Recommendation events contain no API key data |
| Key in error output | `request failed (secret redacted)` — no URL or key in errors |
| Accidental `println!` | All fields are private; custom `Debug` is the only string output |

### Out of Scope

- Network-level interception (use HTTPS — Gemini API is TLS-only)
- Compromise of the runtime process memory (OS-level protection required)
- Key rotation (manual — update the env var and restart)

---

## Best Practices

1. **Never hardcode keys** — always use environment variables
2. **Restrict permissions** — `chmod 600` for secrets files
3. **Use separate keys** — one for development, one for production
4. **Rotate keys** — revoke and regenerate if a key is exposed
5. **Monitor usage** — check Google AI Studio dashboard for unexpected API calls
6. **Principle of least privilege** — the key only needs `generateContent` access

---

## Commands Cheat Sheet

```bash
# Set key for current shell session
export GEMINI_API_KEY='YOUR_KEY_HERE'

# Create and secure a secrets file
mkdir -p ~/.config/agenticos && chmod 700 ~/.config/agenticos
echo 'GEMINI_API_KEY=YOUR_KEY_HERE' >> ~/.config/agenticos/secrets.env
chmod 600 ~/.config/agenticos/secrets.env

# Load secrets file
source ~/.config/agenticos/secrets.env

# Verify the key is set
echo ${GEMINI_API_KEY:0:4}  # should print "AIza"

# Run the daemon with the key
source ~/.config/agenticos/secrets.env && cargo run --bin agenticos-daemon

# Run CLI commands with the key
source ~/.config/agenticos/secrets.env && cargo run --bin agenticos recommendations
```
