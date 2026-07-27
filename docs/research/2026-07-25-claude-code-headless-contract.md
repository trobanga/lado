# The Claude Code headless contract

Research for `diff-93a.2`, on the map *Map: agent-task subsystem in lado, with C4 diff diagrams
as first consumer* (`diff-93a`).

**Subject:** what lado can rely on when it spawns `claude -p` as a subprocess.
**Version tested:** `claude` 2.1.220, `/home/trobanga/.local/bin/claude`, Linux.
**Date:** 2026-07-25.

Everything under "Verified" was observed by running the binary on this machine. Everything under
"Unverified" is explicitly *not* established — do not design against it without testing first.
Facts sourced only from `--help` text are marked _(help only)_.

> **Caveat on generality.** All probes ran with this machine's ambient config: a large global
> `CLAUDE.md`, several plugins, MCP servers, and SessionStart hooks. Token and latency figures
> are therefore an upper bound for *this* setup, not an intrinsic floor. See §7.

---

## 1. Invocation

**Verified**

- `-p` / `--print` selects non-interactive mode. Claude also enters it automatically when stdout
  is not a TTY _(help only)_.
- **The prompt can be delivered on stdin.** `echo 'prompt' | claude -p ...` works with no
  positional argument. (An earlier reading of `--help` concluded there was "no documented way" to
  do this; that was wrong.)
- `--input-format` accepts `text` (default) or `stream-json`, only with `--print`.
- Exit code is `0` for `--help`, `1` for an unrecognised flag.

**Help only**

- `--model` takes aliases (`fable`, `opus`, `sonnet`) or full ids (`claude-opus-5`).
- `--fallback-model` takes a comma-separated retry list for overload/unavailability.
- `--system-prompt[-file]` replaces the default system prompt;
  `--append-system-prompt[-file]` appends to it.
- `--bare` strips hooks, LSP, plugin sync, auto-memory and `CLAUDE.md` auto-discovery.
  **Important:** `--bare` authenticates *strictly* via `ANTHROPIC_API_KEY` or `apiKeyHelper` and
  never reads OAuth/keychain. On a subscription-authenticated machine (§6) `--bare` will not
  authenticate at all unless an API key is supplied.
- The working directory determines project context.

---

## 2. Output — `--output-format json`

**Verified.** A single JSON object. Full observed key set, success case:

```json
{
  "type": "result",
  "subtype": "success",
  "is_error": false,
  "terminal_reason": "completed",
  "result": "OK",
  "api_error_status": null,
  "num_turns": 1,
  "stop_reason": "end_turn",
  "session_id": "df494f00-...",
  "uuid": "7bbc3f9a-...",
  "total_cost_usd": 0.1447105,
  "usage": { "input_tokens": 0, "cache_creation_input_tokens": 0,
             "cache_read_input_tokens": 0, "output_tokens": 0,
             "service_tier": "standard", "...": "..." },
  "modelUsage": { "claude-opus-5[1m]": {
      "inputTokens": 2, "outputTokens": 4,
      "cacheCreationInputTokens": 13499, "cacheReadInputTokens": 19221,
      "costUSD": 0.1447105, "contextWindow": 1000000,
      "maxOutputTokens": 64000, "canonicalModel": "claude-opus-5",
      "provider": "firstParty" } },
  "permission_denials": [],
  "duration_ms": 1977, "duration_api_ms": 1678,
  "ttft_ms": 1862, "ttft_stream_ms": 1437, "time_to_request_ms": 213,
  "fast_mode_state": "off", "fast_mode_disabled_reason": "sdk_opt_in_required"
}
```

Error case (budget exhausted) differs in exactly these fields:

```json
{ "is_error": true, "subtype": "error_max_budget_usd",
  "terminal_reason": "budget_exhausted",
  "errors": ["Reached maximum budget ($0.1)"] }
```

Consequences for lado:

- **The final assistant text is cleanly isolated in `result`.** No tool chatter to strip.
- Success/failure is machine-readable three ways: `is_error`, `subtype`, `terminal_reason`.
- `errors` is an array, present only on failure.
- **Errors are written to stdout as part of the JSON. stderr was empty in every probe.** Do not
  treat "stderr is empty" as "the run succeeded".
- `permission_denials` is a structured array — denials need no text scraping.

### Structured output — `--json-schema`

**Verified** (added 2026-07-26, probed for `diff-93a.6`). Passing
`--json-schema '<JSON Schema>'` adds a **`structured_output`** key to the result object: the
parsed object itself, alongside the stringified copy in `result`.

```json
{
  "structured_output": { "capital": "Paris" },
  "result": "{\"capital\":\"Paris\"}",
  "stop_reason": "tool_use",
  "subtype": "success",
  "is_error": false
}
```

Consequences for lado:

- **`stop_reason` is `"tool_use"`, not `"end_turn"`.** Structured output is implemented as a
  **forced tool call**, so the schema is enforced API-side. Do not treat `stop_reason != "end_turn"`
  as anomalous when a schema is in play.
- **Deserialise `structured_output` directly.** No prose parsing, no fenced-block extraction, no
  handling of the model wrapping JSON in commentary.
- **Malformed JSON is largely off the table.** The failure mode left to design for is
  *well-formed output that is wrong*, which no schema catches — hence the semantic validation in
  `diff-93a.6`.
- **Cost floor:** this trivial run cost **$0.195** with **19,305 cache-creation tokens**, *with*
  `--setting-sources ''`. That is the per-run tax before any real work.

Still unverified: whether `structured_output` can be **absent or null on an otherwise-successful
run** (model ends its turn without calling the tool). Treat it as optional.

---

## 3. Output — `--output-format stream-json`

**Verified.** Newline-delimited JSON (NDJSON); every line parsed independently (17/17).
Requires `--verbose`.

Observed event sequence for a trivial prompt:

```
system/hook_started   x6
system/hook_response  x5
system/init
system/hook_progress
assistant
rate_limit_event
result/success
system/hook_response      <-- AFTER the result
```

Two things lado must handle:

- **The `result` event is not the last line.** A `system/hook_response` followed it. Parse by
  filtering on `type === "result"`, never by taking the last line.
- **`rate_limit_event` is a first-class event type.** Given subscription auth (§6), this is the
  channel through which quota pressure becomes observable.
- Hook events appear inline; `--include-hook-events` and `--include-partial-messages` add more
  _(help only)_.

---

## 4. Exit codes

**Verified**

| Situation | Exit |
|---|---|
| `--help` | 0 |
| Unrecognised flag | 1 |
| Successful run | 0 |
| Budget exhausted | 1 |

Exit code alone does not distinguish *why* a run failed — read `subtype` for that.

---

## 5. Cost, usage and limits

**Verified**

- Usage and cost **are** reported: `total_cost_usd`, plus per-model `costUSD`, `inputTokens`,
  `outputTokens`, `cacheCreationInputTokens`, `cacheReadInputTokens`, `contextWindow`.
- Timing: `duration_ms`, `duration_api_ms`, `ttft_ms`, `ttft_stream_ms`, `time_to_request_ms`.
- **`--max-budget-usd` is enforced after the fact, not before.** A `--max-budget-usd 0.10` run
  reported `total_cost_usd: 0.32753` — 3.3× the cap — because context caching is billed before
  any budget check runs. **Treat it as a stop signal, not a ceiling.**
- **`--max-turns` does not exist** in 2.1.220. `--max-budget-usd` is the only run-length control.

---

## 6. Authentication — and why "cost" is the wrong unit here

**Verified on this machine**

- No `ANTHROPIC_API_KEY`, no `ANTHROPIC_AUTH_TOKEN`, no `apiKeyHelper`, no Bedrock/Vertex.
- `~/.claude/.credentials.json` holds `claudeAiOauth` with `subscriptionType` and
  `rateLimitTier`.

So this machine is **subscription-authenticated**. `total_cost_usd` is therefore a *notional
API-equivalent* figure, not a charge. The real scarce resource is **rate-limit quota — the same
quota the user's interactive Claude Code sessions consume.** A lado run that burns quota can
degrade the user's own coding session, which is a worse failure than a small charge.

This is conditional, not universal: a user running lado with an API key **is** billed, and
`total_cost_usd` is reported either way, so lado can detect and present both cases.

Interaction with `--bare`: it forces API-key auth and never reads OAuth, so it is **not** a free
way to trim context on a subscription machine.

---

## 7. Ambient config is inherited — and it dominates the cost

**Verified**

- The user's SessionStart hooks fired in headless mode (6 `hook_started` events) — so a spawned
  run inherits hooks, plugins, MCP servers and `CLAUDE.md`.
- Context tokens dwarfed the actual work in every probe. A 2-token prompt returning 4 tokens:
  - first run: `cacheCreationInputTokens: 32742`
  - later runs: `cacheCreationInputTokens ≈ 13500`, `cacheReadInputTokens ≈ 19221`
- **Caching amortises across separate process invocations** — later runs re-read ~19k rather than
  recreating it. Repeated lado runs get cheaper, but never cheap.

Design levers: `--bare` (but see §6 auth caveat), `--setting-sources`, `--system-prompt` to
replace rather than inherit, `--strict-mcp-config`.

**This is the strongest argument for the eligibility gate** (`diff-93a.3`): there is a large
fixed overhead per run regardless of how small the diff is. Diagramming a lockfile bump costs
nearly as much as diagramming a real refactor.

---

## 8. Skills — the key negative finding

**Verified: a `/skill-name` slash command in a `-p` prompt does NOT resolve as a skill.**

Method: piped `/q what is 7 times 6` to `claude -p --output-format stream-json`. Evidence:

- No `tool_use` block named `Skill` (or `Task`) anywhere in the event stream.
- No model switch — the `/q` command advertised the haiku model, yet `modelUsage` contained only
  `claude-opus-5`.
- A second probe, `/q what is 2+2`, returned `"4"` — i.e. the text was answered literally as a
  prompt rather than dispatched as a command.

**Consequence for `diff-93a.6`:** lado cannot invoke `c4-diff` by writing `/c4-diff base head`
into a `-p` prompt. The skill's instructions must reach the model some other way — inlined into
the prompt body, or via `--append-system-prompt[-file]`, or possibly `--plugin-dir` (untested).

_Caveat:_ the `/q` command used as the probe was itself malformed (no `model:` frontmatter, an
unsupported `args:` block). That does not weaken the finding — the absence of any `Skill` tool
use shows dispatch never occurred — but a purpose-built probe would make it airtight.

---

## 9. Permissions

**Verified**

- `--permission-mode dontAsk` completed without hanging, in a repo, non-interactively.
- `permission_denials` is a structured array on the result object.

**Help only**

- `--permission-mode` choices: `acceptEdits`, `auto`, `bypassPermissions`, `manual`, `dontAsk`,
  `plan`.
- `--allowedTools` / `--disallowedTools` accept names with glob patterns, e.g. `"Bash(git *)"`.
- `--add-dir` grants tool access to additional directories.
- `--dangerously-skip-permissions` bypasses all checks; `--allow-dangerously-skip-permissions`
  merely makes it available.
- Workspace-trust dialog is skipped in non-interactive mode.

---

## 10. Unverified — do not design against these

1. **SIGTERM/SIGINT handling.** Whether cancellation is graceful, leaves partial artifacts, or
   orphans child processes. Directly blocks part of *Running a task that takes minutes*
   (`diff-93a.8`). All probes used an external `timeout` wrapper.
2. **Any built-in timeout.** None documented; none observed.
3. **Context-window overflow behaviour** — error, truncation, or silent degradation.
4. **Unauthenticated / rate-limited runs** — exit codes and result shapes. Not probed because
   this machine is authenticated and under quota.
5. ~~**Whether the `Write` tool is scoped by `--add-dir`,** or needs separate sandbox config.~~
   **Moot as of `diff-93a.6`** — the graph returns on stdout and lado writes the artifact folder,
   so the agent is given no `Write` tool at all. Still open in a weaker form for `diff-93a.13`:
   whether `Bash` can be scoped to git, since `Bash` can write regardless.
6. **Whether `--plugin-dir` makes a skill invocable,** and how it would be addressed given §8.
7. **Settings precedence** across CLI flags, local, project, user.
8. **Prompt size limits** on stdin.
9. ~~**`--json-schema` validation-failure reporting.**~~ **Probed 2026-07-26 — see §2,
   "Structured output".** The flag works and yields a parsed `structured_output` field, enforced
   API-side as a forced tool call. Residual unknown: whether `structured_output` can be absent on
   an otherwise-successful run.
10. **`--max-budget-usd` overshoot magnitude** on a long run. Observed 3.3× on a trivial one;
    unknown whether the absolute overshoot stays bounded by context size.

---

## Method

`claude --help`, plus five live runs from `/home/trobanga/code/lado`:

1. `--output-format json --max-budget-usd 0.10` → budget-exhausted error shape.
2. `--output-format json` → success shape.
3. `--output-format json` with `/q ...` → skill non-dispatch, first evidence.
4. `--output-format stream-json --verbose` with `/q ...` → NDJSON shape, skill non-dispatch
   confirmed, event taxonomy.
5. `--output-format json --json-schema '{...}' --setting-sources ''` (2026-07-26, for
   `diff-93a.6`) → `structured_output` shape, `stop_reason: "tool_use"`, cost floor.

All used `--no-session-persistence --permission-mode dontAsk` and an external `timeout`.
