#!/usr/bin/env python3
"""Asks the pinned Claude Code binary whether each command in a cases file is denied by the
rules in a settings file, with no login and no network: Claude Code talks to a local stand-in
for the Messages API (fakeapi.py), which answers each turn with one tool_use carrying the
command and records the tool_result Claude Code sends back ("Permission to use Bash with
command ... has been denied." when a rule denies it; the command's own output otherwise).

Usage: probe.py <claude binary> <cases.json>
cases.json: {"settings": {...Claude Code flag settings...},
             "cases": [{"name": "...", "tool": "Bash", "command": "...", "denied": true}, ...]}
Prints one line per case (DENIED or ran) and exits 1 if any case's "denied" disagrees.
Only the Bash tool exists off Windows; a PowerShell case is reported as "no tool".
"""
import json, os, shutil, subprocess, sys, tempfile, time

binary, cases_file = sys.argv[1], sys.argv[2]
spec = json.load(open(cases_file))
here = os.path.dirname(os.path.abspath(__file__))
work = tempfile.mkdtemp(prefix="claude-rule-probe-")
settings = os.path.join(work, "settings.json")
json.dump(spec["settings"], open(settings, "w"))
cmd_file = os.path.join(work, "cmd.json")
out_file = os.path.join(work, "out.txt")
port = 18900 + os.getpid() % 90
server = subprocess.Popen([sys.executable, os.path.join(here, "fakeapi.py"), cmd_file, out_file, str(port)])
time.sleep(0.8)
home = os.path.join(work, "home")
os.makedirs(home)
env = dict(os.environ, HOME=home, CLAUDE_CONFIG_DIR=home,
           ANTHROPIC_BASE_URL=f"http://127.0.0.1:{port}", ANTHROPIC_API_KEY="sk-ant-probe",
           CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_TELEMETRY="1")
failed = 0
try:
    for case in spec["cases"]:
        json.dump(case, open(cmd_file, "w"))
        if os.path.exists(out_file):
            os.remove(out_file)
        subprocess.run([binary, "-p", "go", "--settings", settings, "--max-turns", "2",
                        "--permission-mode", "default"],
                       env=env, cwd=work, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                       stderr=subprocess.DEVNULL, timeout=120)
        result = open(out_file).read() if os.path.exists(out_file) else ""
        if "NO TOOL" in result:
            verdict = "no tool"
        else:
            verdict = "DENIED" if "has been denied" in result else "ran"
        expected = case.get("denied")
        ok = expected is None or verdict == "no tool" or (verdict == "DENIED") == expected
        failed += 0 if ok else 1
        print(f"{'ok ' if ok else 'BAD'} {verdict:7} {case['tool']:10} {case.get('name', '')}: {case['command']}")
finally:
    server.terminate()
    server.wait()
    shutil.rmtree(work, ignore_errors=True)
sys.exit(1 if failed else 0)
