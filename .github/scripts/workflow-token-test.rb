# The token rule: no job that a pull request, a pull request review or a merge group can run holds
# `actions: write` while it checks out that event's code. In a merge group a fork's code runs with
# what the job grants, and the token can be read from the runner's memory; a pull request from a
# branch of this repository is the same. A checkout counts as fixed only when its `ref` reads
# nothing but the base (github.event.pull_request.base.ref or .sha, github.event.merge_group.base_sha,
# github.base_ref, github.event_name to choose between them) or is a literal; with no `ref` it is
# the event's own code. A job or a step is reachable unless its `if:` is false for that event
# (workflow_expr.rb; what cannot be known counts as reachable).
# The second rule: a job those events can run holds `actions: write` only if ALLOWED_ACTIONS_WRITE
# lists it, and a listed job has no `uses:` step at all, so it checks out nothing and runs only its
# own inline script. The listed jobs cancel a pull request's runs for an older head. A job that
# checks out even the base branch's scripts reads, and does not cancel (windows-pr-tests.yml's
# decide, which held actions: write until ci-decide-token). fast-lane runs it.
# Run it from anywhere: ruby .github/scripts/workflow-token-test.rb
# WORKFLOWS_GLOB, for the self-test below, reads other files.

require "yaml"
require "tmpdir"
require_relative "workflow_expr"

Dir.chdir(File.expand_path("../..", __dir__))

PR_EVENTS = %w[pull_request pull_request_target pull_request_review pull_request_review_comment merge_group].freeze
BASE_IDS = %w[github.event_name github.event.pull_request.base.ref github.event.pull_request.base.sha
              github.event.merge_group.base_sha github.base_ref].freeze

# file => job ids. Each entry is a job of the shape above; a new one needs its own review.
ALLOWED_ACTIONS_WRITE = {
  ".github/workflows/e2e-windows.yml" => ["e2e-cancel-stale"],
  ".github/workflows/windows-pr-tests.yml" => ["windows-cancel-stale"],
}.freeze

def context(event)
  ref = event == "merge_group" ? "refs/heads/gh-readonly-queue/main/pr-1-0000" : "refs/pull/1/merge"
  lambda do |id|
    case id
    when "github.event_name" then event
    when "github.ref" then ref
    end
  end
end

def triggers(doc)
  on = doc.key?("on") ? doc["on"] : doc[true]
  on.is_a?(Hash) ? on.keys : Array(on)
end

def actions_write?(perms)
  perms == "write-all" || (perms.is_a?(Hash) && perms["actions"] == "write")
end

def fixed_ref?(ref)
  return false if ref.nil? || ref.to_s.strip.empty?
  return true unless ref.to_s.include?("${{")
  ids = WorkflowExpr.identifiers(ref)
  !ids.empty? && (ids - BASE_IDS).empty?
end

# Each violation as "file: job id: what". allowed nil checks the first rule only (its self-tests).
def violations(files, allowed = ALLOWED_ACTIONS_WRITE)
  files.flat_map do |file|
    doc = YAML.safe_load(File.read(file), aliases: true) || {}
    events = triggers(doc) & PR_EVENTS
    (doc["jobs"] || {}).flat_map do |id, job|
      perms = job.key?("permissions") ? job["permissions"] : doc["permissions"]
      next [] unless actions_write?(perms)
      events.flat_map do |ev|
        ctx = context(ev)
        next [] unless WorkflowExpr.cond?(job["if"], ctx)
        listed = allowed.nil? || (allowed[file] || []).include?(id)
        uses = (job["steps"] || []).select { |s| s.key?("uses") }
        extra = []
        extra << "#{file}: job #{id} holds actions: write on #{ev} and is not in ALLOWED_ACTIONS_WRITE" unless listed
        extra << "#{file}: job #{id} holds actions: write on #{ev} and has a uses: step (#{uses.map { |s| s["uses"] }.join(", ")})" if allowed && listed && !uses.empty?
        extra + (job["steps"] || []).select do |s|
          s["uses"].to_s.start_with?("actions/checkout@") && WorkflowExpr.cond?(s["if"], ctx) &&
            !fixed_ref?(s.dig("with", "ref"))
        end.map { |s| "#{file}: job #{id} holds actions: write and on #{ev} checks out #{s.dig("with", "ref") ? "ref #{s.dig("with", "ref")}" : "the event's code"}" }
      end
    end
  end.uniq
end

failures = 0

# The rule on the workflows.
files = Dir[ENV.fetch("WORKFLOWS_GLOB", ".github/workflows/*.{yml,yaml}")].sort
found = violations(files)
found.each { |v| puts "FAIL: #{v}" }
failures += found.size
puts "ok: no job holds actions: write while it checks out pull request or merge group code, and only the listed jobs hold it, with no uses: step (#{files.size} workflows)" if found.empty?

# The rule itself, on planted jobs: it must catch each bad one and pass each good one.
Dir.mktmpdir do |dir|
  checkout = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1"
  planted = {
    "default checkout" => [{ "uses" => checkout }, true],
    "head ref" => [{ "uses" => checkout, "with" => { "ref" => "${{ github.event.pull_request.head.sha }}" } }, true],
    "github.sha" => [{ "uses" => checkout, "with" => { "ref" => "${{ github.event.merge_group.base_sha || github.sha }}" } }, true],
    "base ref" => [{ "uses" => checkout, "with" => { "ref" => "${{ github.event_name == 'merge_group' && github.event.merge_group.base_sha || github.event.pull_request.base.ref }}" } }, false],
    "skipped on these events" => [{ "uses" => checkout, "if" => "github.event_name == 'schedule'" }, false],
  }
  planted.each_with_index do |(name, (step, bad)), i|
    path = File.join(dir, "w#{i}.yml")
    File.write(path, { "on" => %w[pull_request merge_group schedule],
                       "jobs" => { "j" => { "permissions" => { "actions" => "write" }, "steps" => [step] } } }.to_yaml)
    got = !violations([path], nil).empty?
    if got == bad
      puts "ok: self-test: #{name} #{bad ? "is caught" : "passes"}"
    else
      puts "FAIL: self-test: #{name} #{bad ? "is not caught" : "is flagged"}"
      failures += 1
    end
  end
  path = File.join(dir, "job-if.yml")
  File.write(path, { "on" => %w[pull_request schedule],
                     "jobs" => { "j" => { "if" => "github.event_name == 'schedule'", "permissions" => { "actions" => "write" },
                                          "steps" => [{ "uses" => checkout }] } } }.to_yaml)
  if violations([path], nil).empty?
    puts "ok: self-test: a job its if: keeps off pull requests passes"
  else
    puts "FAIL: self-test: a job its if: keeps off pull requests is flagged"
    failures += 1
  end
  # Function calls in an if: (workflow_expr.rb raised NameError on any call before ci-cache-plan-a).
  { "startsWith(github.ref, 'refs/heads/main') && github.event_name == 'schedule'" => false,
    "always() && contains(format('{0}-x', github.event_name), 'pull')" => true }.each_with_index do |(cond, bad), k|
    fpath = File.join(dir, "job-if-call-#{k}.yml")
    File.write(fpath, { "on" => %w[pull_request schedule],
                        "jobs" => { "j" => { "if" => cond, "permissions" => { "actions" => "write" },
                                             "steps" => [{ "uses" => checkout }] } } }.to_yaml)
    caught = !violations([fpath], nil).empty?
    if caught == bad
      puts "ok: self-test: if: #{cond} #{bad ? "is caught" : "passes"}"
    else
      puts "FAIL: self-test: if: #{cond} #{bad ? "is not caught" : "is flagged"}"
      failures += 1
    end
  end

  # The second rule: [steps, listed, bad].
  run_only = [{ "run" => "gh api -X POST repos/o/r/actions/runs/1/cancel" }]
  base_checkout = { "uses" => checkout, "with" => { "ref" => "${{ github.event.pull_request.base.ref }}" } }
  {
    "a listed job that only runs a script" => [run_only, true, false],
    "an unlisted job that only runs a script" => [run_only, false, true],
    "an unlisted job that checks out the base (decide before ci-decide-token)" => [[base_checkout] + run_only, false, true],
    "a listed job that checks out the base" => [[base_checkout] + run_only, true, true],
  }.each_with_index do |(name, (steps, listed, bad)), i|
    path = File.join(dir, "a#{i}.yml")
    File.write(path, { "on" => %w[pull_request pull_request_review merge_group],
                       "jobs" => { "j" => { "permissions" => { "actions" => "write" }, "steps" => steps } } }.to_yaml)
    got = !violations([path], listed ? { path => ["j"] } : {}).empty?
    if got == bad
      puts "ok: self-test: #{name} #{bad ? "is caught" : "passes"}"
    else
      puts "FAIL: self-test: #{name} #{bad ? "is not caught" : "is flagged"}"
      failures += 1
    end
  end
end

if failures.positive?
  puts "#{failures} failed"
  exit 1
end
puts "all passed"
