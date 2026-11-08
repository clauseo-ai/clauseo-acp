# The fallback guard: install-pinned-tools' `allow-fallback` (a failed GitHub download falls back to
# crates.io, npm or go install, trusting those registries' checksums instead of the pinned sha256)
# is for CI jobs only. It fails when:
# - release.yml contains allow-fallback anywhere;
# - any other workflow's job that sets it signs, notarizes, publishes or uploads release assets
#   (the markers below, read from the job's names, commands, actions, inputs and env, not comments).
# It proves itself on planted workflows. fast-lane runs it.
# Run it from anywhere: ruby .github/scripts/fallback-guard-test.rb

require "yaml"
require "tmpdir"

Dir.chdir(File.expand_path("../..", __dir__))

# What a release, signing or publishing job contains.
MARKERS = /gh\s+release|action-gh-release|upload-release-asset|releases\/assets|\bcodesign\b|\bsigntool\b|\bminisign\b|\bcosign\b|gpg\s+(--detach-)?sign|notari[sz]|TAURI_SIGNING|APPLE_CERTIFICATE|\bsigning\b|\bpublish/i

def falls_back?(step)
  step["uses"].to_s.start_with?("./.github/actions/install-pinned-tools") &&
    step.fetch("with", {}).key?("allow-fallback") && step["with"]["allow-fallback"].to_s != "false"
end

# Each violation, for the workflow files given; release is the release workflow's path.
def violations(files, release)
  found = []
  files.each do |file|
    if File.basename(file) == File.basename(release) && File.read(file).include?("allow-fallback")
      found << "#{file}: the release workflow mentions allow-fallback"
      next
    end
    doc = YAML.safe_load(File.read(file), aliases: true) || {}
    (doc["jobs"] || {}).each do |id, job|
      next unless (job["steps"] || []).any? { |s| falls_back?(s) }
      text = { "name" => job["name"], "env" => job["env"], "steps" => job["steps"] }.to_yaml
      if (m = text.match(MARKERS))
        found << "#{file}: job #{id} sets allow-fallback but looks like a release, signing or publishing job (#{m[0].inspect})"
      end
    end
  end
  found
end

failures = 0
files = Dir[".github/workflows/*.{yml,yaml}"].sort
found = violations(files, ".github/workflows/release.yml")
found.each { |v| puts "FAIL: #{v}" }
failures += found.size
setters = files.flat_map do |f|
  doc = YAML.safe_load(File.read(f), aliases: true) || {}
  (doc["jobs"] || {}).select { |_, j| (j["steps"] || []).any? { |s| falls_back?(s) } }.keys.map { |j| "#{File.basename(f)}:#{j}" }
end
if found.empty?
  puts "ok: release.yml has no allow-fallback, and no job that sets it signs or publishes (it is set in #{setters.join(", ")})"
end

# The action itself: off by default, and every fallback step, and the download steps' tolerance of
# failure, hang on allow-fallback being "true".
action = YAML.safe_load(File.read(".github/actions/install-pinned-tools/action.yml"))
gate = "inputs.allow-fallback == 'true'"
steps = action.dig("runs", "steps")
problems = []
problems << "allow-fallback's default is not \"false\"" unless action.dig("inputs", "allow-fallback", "default") == "false"
steps.each do |st|
  name = st["name"] || st["uses"]
  if st["run"].to_s.include?("fallback.sh") || name.to_s.include?("after the fallback")
    problems << "step #{name.inspect} runs without #{gate}" unless st["if"].to_s.start_with?(gate)
  end
  if st.key?("continue-on-error") && st["continue-on-error"] != "${{ #{gate} }}"
    problems << "step #{name.inspect} tolerates failure without #{gate}"
  end
end
fallback_steps = steps.count { |st| st["run"].to_s.include?("fallback.sh") }
problems << "#{fallback_steps} steps run fallback.sh, not 2" unless fallback_steps == 2
problems.each { |pr| puts "FAIL: install-pinned-tools: #{pr}" }
failures += problems.size
puts "ok: install-pinned-tools: allow-fallback defaults to \"false\", and both fallback steps, the check after them and the downloads' continue-on-error hang on it" if problems.empty?

# Planted cases: each bad one must be caught, each good one pass.
Dir.mktmpdir do |dir|
  tools = { "uses" => "./.github/actions/install-pinned-tools", "with" => { "tools" => "cargo-nextest", "allow-fallback" => "true" } }
  write = lambda do |name, jobs|
    path = File.join(dir, name)
    File.write(path, { "on" => ["workflow_dispatch"], "jobs" => jobs }.to_yaml)
    path
  end
  cases = {
    "release.yml with allow-fallback" => [write.call("release.yml", { "j" => { "steps" => [tools] } }), true],
    "a job that runs gh release upload" => [write.call("a.yml", { "j" => { "steps" => [tools, { "run" => "gh release upload v1 app.dmg" }] } }), true],
    "a job that signs with TAURI_SIGNING_PRIVATE_KEY" => [write.call("b.yml", { "j" => { "env" => { "TAURI_SIGNING_PRIVATE_KEY" => "x" }, "steps" => [tools] } }), true],
    "a job named publish" => [write.call("c.yml", { "publish" => { "name" => "Publish the packages", "steps" => [tools] } }), true],
    "a test job" => [write.call("d.yml", { "j" => { "steps" => [tools, { "run" => "cargo nextest run" }] } }), false],
    "a release job without allow-fallback" => [write.call("e.yml", { "j" => { "steps" => [{ "uses" => "./.github/actions/install-pinned-tools" }, { "run" => "gh release upload v1 x" }] } }), false],
  }
  cases.each do |name, (path, bad)|
    got = !violations([path], File.join(dir, "release.yml")).empty?
    if got == bad
      puts "ok: planted: #{name} #{bad ? "is caught" : "passes"}"
    else
      puts "FAIL: planted: #{name} #{bad ? "is not caught" : "is flagged"}"
      failures += 1
    end
  end
end

if failures.positive?
  puts "#{failures} failed"
  exit 1
end
puts "all passed"
