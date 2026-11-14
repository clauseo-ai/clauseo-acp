#!/usr/bin/env python3
"""Asks the pinned Claude Code binary which permission rules of a settings file it accepts, and
whether each command in a cases file is denied by them, with no login and no network.

1. Rules: Claude Code loads the settings as its --settings file and lists the rules it kept
   (the SDK's list_permission_rules control request); every rule of the file must be among
   them but those in "expect_skipped". Then `claude doctor` reads the same settings as a
   project's .claude/settings.json and must report exactly the expected
   'Invalid permission rule "…" was skipped' warnings.
2. Commands: Claude Code talks to a local stand-in for the Messages API (fakeapi.py), which
   answers each turn with one tool_use carrying the command and records the tool_result it
   sends back ("Permission to use Bash with command ... has been denied." when a rule denies
   it; the command's own output otherwise).

Usage: probe.py <claude binary> <cases.json>   (PROBE_SKIP_RULES=1 skips the rules check;
PROBE_HOME sets the HOME Claude Code expands $HOME to)
cases.json: {"settings": {...Claude Code flag settings...},
             "expect_skipped": ["Bash(...)", ...],
             "cases": [{"name": "...", "tool": "Bash", "command": "...", "denied": true}, ...]}
A case may also set "mode" (a --permission-mode, default "default"; "auto" needs no gate offline:
2.1.293 enables auto mode for the fake API's default model) and "classifier": true/false, checked
against whether Claude Code sent the fake API an auto mode classifier request during the case
(fakeapi.py logs every request to a file and answers the classifier "allow", or "block" with
CLASSIFIER_VERDICT=block in the environment). Verdicts: DENIED (a deny rule; the only one
"denied" checks), REFUSED (a path check, a classifier block, or a prompt -p mode refuses), ran.
A case's "ran": true/false checks that verdict is (or is not) ran. A file's "server_verdict"
("flagged" or "not_flagged") has the fake API send the server-side classifier's verdict with each
tool use (fakeapi.py's SERVER_VERDICT): "flagged" refuses whatever Claude Code allowed only as
read-only, and lets through what a rule allowed. PROBE_VERBOSE=1 prints the tool result of each case. A case may give "input" instead of
"command" (any tool's input, e.g. {"tool": "Read", "input": {"file_path": "/x"}}).
Prints the rule counts and one line per case (DENIED or ran), and exits 1 on any
disagreement. A case whose "denied" is null is reported, not checked. Only the Bash tool exists
off Windows; a PowerShell case is reported as "no tool".
"""
import json, os, shutil, socket, subprocess, sys, tempfile, time

binary, cases_file = sys.argv[1], sys.argv[2]
spec = json.load(open(cases_file))
here = os.path.dirname(os.path.abspath(__file__))
work = tempfile.mkdtemp(prefix="claude-rule-probe-")
settings = os.path.join(work, "settings.json")
json.dump(spec["settings"], open(settings, "w"))
cmd_file = os.path.join(work, "cmd.json")
out_file = os.path.join(work, "out.txt")
req_log = os.path.join(work, "requests.jsonl")
with socket.socket() as probe_socket:
    probe_socket.bind(("127.0.0.1", 0))
    port = probe_socket.getsockname()[1]
# "server_verdict" (flagged or not_flagged): the server-side classifier's verdict the fake API
# sends with each tool use (fakeapi.py's SERVER_VERDICT), as the first-party API does in auto mode.
server_env = dict(os.environ)
if spec.get("server_verdict"):
    server_env["SERVER_VERDICT"] = spec["server_verdict"]
server = subprocess.Popen([sys.executable, os.path.join(here, "fakeapi.py"), cmd_file, out_file, str(port),
                           req_log], env=server_env)
time.sleep(0.8)
home = os.path.join(work, "home")
os.makedirs(home)
# Claude Code puts its HOME in place of an expandable $HOME before it matches a Bash command;
# PROBE_HOME sets that HOME (the laptop's, for the host's generated cases).
env = dict(os.environ, HOME=os.environ.get("PROBE_HOME", home), CLAUDE_CONFIG_DIR=home,
           ANTHROPIC_BASE_URL=f"http://127.0.0.1:{port}", ANTHROPIC_API_KEY="sk-ant-probe",
           CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_TELEMETRY="1")
failed = 0


def unescaped(text, c, last):
    """The first (last) c with an even run of backslashes before it (Claude Code's p and m)."""
    found = [i for i, ch in enumerate(text)
             if ch == c and (len(text[:i]) - len(text[:i].rstrip("\\"))) % 2 == 0]
    return (found[-1] if last else found[0]) if found else -1


def canon(rule):
    """A rule as Claude Code reads it (lYe, then Lho): its tool and content. Claude Code lists
    the rules it kept written its own way (Bash(*C:\\\\Users*) for Bash(*C:\\Users*)), so
    rules are compared read, not as written. A malformed rule is itself."""
    start, end = unescaped(rule, "(", False), unescaped(rule, ")", True)
    if start < 0 or end != len(rule) - 1 or end <= start:
        return rule
    raw = rule[start + 1:end]
    return rule[:start], raw.replace("\\(", "(").replace("\\)", ")").replace("\\\\", "\\")


def result_lines(result):
    """The tool results fakeapi.py recorded (one JSON list per tool_result message)."""
    lines = []
    for raw in result.splitlines():
        try:
            parsed = json.loads(raw)
        except ValueError:
            continue
        lines += [str(item) for item in parsed] if isinstance(parsed, list) else []
    return lines


def kept_rules():
    """The rules Claude Code kept from the --settings file (source flagSettings)."""
    requests = [{"type": "control_request", "request_id": "init", "request": {"subtype": "initialize"}},
                {"type": "control_request", "request_id": "rules",
                 "request": {"subtype": "list_permission_rules"}}]
    stdin = "".join(json.dumps(request) + "\n" for request in requests)
    run = subprocess.run([binary, "-p", "--input-format", "stream-json", "--output-format",
                          "stream-json", "--verbose", "--settings", settings],
                         input=stdin, capture_output=True, text=True, env=env, cwd=work, timeout=300)
    for line in run.stdout.splitlines():
        try:
            message = json.loads(line)
        except ValueError:
            continue
        response = message.get("response") or {}
        if message.get("type") == "control_response" and response.get("request_id") == "rules":
            rules = response["response"]["state"]["rules"]
            return [rule["rule"] for rule in rules if rule.get("source") == "flagSettings"]
    raise SystemExit("no list_permission_rules response:\n" + run.stdout[-2000:] + run.stderr[-2000:])


def doctor_warnings():
    """The 'Invalid permission rule' lines `claude doctor` reports for the settings."""
    project = os.path.join(work, "project")
    os.makedirs(os.path.join(project, ".claude"))
    shutil.copy(settings, os.path.join(project, ".claude", "settings.json"))
    run = subprocess.run([binary, "doctor"], capture_output=True, text=True, env=env, cwd=project,
                         stdin=subprocess.DEVNULL, timeout=300)
    return [line for line in run.stdout.splitlines() if "Invalid permission rule" in line]


try:
    permissions = spec["settings"].get("permissions", {})
    check_rules = os.environ.get("PROBE_SKIP_RULES") != "1"
    emitted = [rule for key in ("deny", "allow", "ask") for rule in permissions.get(key, [])]
    skipped = set(spec.get("expect_skipped", []))
    kept_list = kept_rules() if check_rules else emitted
    kept = {canon(rule) for rule in kept_list}
    missing = [rule for rule in emitted if canon(rule) not in kept and rule not in skipped]
    kept_skipped = [rule for rule in skipped if canon(rule) in kept]
    warnings = doctor_warnings() if check_rules else [None] * len(skipped)
    rules_ok = not missing and not kept_skipped and len(warnings) == len(skipped)
    failed += 0 if rules_ok else 1
    print(f"{'ok ' if rules_ok else 'BAD'} rules: {len(emitted)} in the file, {len(kept_list)} kept, "
          f"{len(skipped)} expected skipped, {len(warnings)} doctor warnings")
    for rule in missing[:20]:
        print(f"    not kept: {rule}")
    for rule in kept_skipped:
        print(f"    kept, expected skipped: {rule}")
    for line in warnings[:20]:
        print(f"    doctor: {line.strip()[:300]}")
    for case in spec["cases"]:
        json.dump(case, open(cmd_file, "w"))
        for stale in (out_file, req_log):
            if os.path.exists(stale):
                os.remove(stale)
        mode = case.get("mode", "default")
        subprocess.run([binary, "-p", "go", "--settings", settings, "--max-turns", "2",
                        "--permission-mode", mode],
                       env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=120)
        result = open(out_file).read() if os.path.exists(out_file) else ""
        if "NO TOOL" in result:
            verdict = "no tool"
        else:
            # DENIED: a deny rule refused it ("denied" checks only this). REFUSED: anything else
            # stopped it before the shell: a path check, an auto mode classifier block, or a
            # permission prompt, which -p mode refuses.
            # Every refusal is an is_error result; so is a command's own non-zero exit, which
            # Claude Code reports as "Exit code N".
            refused = any(line.startswith("ERROR: ") and not line.startswith("ERROR: Exit code")
                          for line in result_lines(result))
            verdict = "DENIED" if "has been denied" in result else "REFUSED" if refused else "ran"
        expected = case.get("denied")
        ok = expected is None or verdict == "no tool" or (verdict == "DENIED") == expected
        requests = []
        if os.path.exists(req_log):
            requests = [json.loads(line) for line in open(req_log) if line.strip()]
        classified = any(r.get("classifier") for r in requests)
        want_classifier = case.get("classifier")
        if want_classifier is not None and verdict != "no tool":
            ok = ok and classified == want_classifier
        want_ran = case.get("ran")
        if want_ran is not None and verdict != "no tool":
            ok = ok and (verdict == "ran") == want_ran
        failed += 0 if ok else 1
        note = f" {mode}" if mode != "default" else ""
        note += " classifier" if classified else ""
        print(f"{'ok ' if ok else 'BAD'} {verdict:7} {case['tool']:10} {case.get('name', '')}: {case.get('command', case.get('input'))}"
              f"{'  [' + note.strip() + ']' if note else ''}")
        if os.environ.get("PROBE_VERBOSE") == "1":
            print(f"    {result.strip()[:300]}")
finally:
    server.terminate()
    server.wait()
    shutil.rmtree(work, ignore_errors=True)
sys.exit(1 if failed else 0)
