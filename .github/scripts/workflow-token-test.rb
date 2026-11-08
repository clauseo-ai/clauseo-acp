# The token rule: no job that a pull request, a pull request review or a merge group can run holds
# `actions: write` while it checks out that event's code. In a merge group a fork's code runs with
# what the job grants, and the token can be read from the runner's memory; a pull request from a
# branch of this repository is the same. A checkout counts as fixed only when its `ref` reads
# nothing but the base (github.event.pull_request.base.ref or .sha, github.event.merge_group.base_sha,
# github.base_ref, github.event_name to choose between them) or is a literal; with no `ref` it is
# the event's own code. A job or a step is reachable unless its `if:` is false for that event
# (workflow_expr.rb; what cannot be known counts as reachable). fast-lane runs it.
# Run it from anywhere: ruby .github/scripts/workflow-token-test.rb
# WORKFLOWS_GLOB, for the self-test below, reads other files.

require "yaml"
require "tmpdir"
require_relative "workflow_expr"

Dir.chdir(File.expand_path("../..", __dir__))

PR_EVENTS = %w[pull_request pull_request_target pull_request_review pull_request_review_comment merge_group].freeze
BASE_IDS = %w[github.event_name github.event.pull_request.base.ref github.event.pull_request.base.sha
              github.event.merge_group.base_sha github.base_ref].freeze

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

# Each violation as "file: job id: what".
def violations(files)
  files.flat_map do |file|
    doc = YAML.safe_load(File.read(file), aliases: true) || {}
    events = triggers(doc) & PR_EVENTS
    (doc["jobs"] || {}).flat_map do |id, job|
      perms = job.key?("permissions") ? job["permissions"] : doc["permissions"]
      next [] unless actions_write?(perms)
      events.flat_map do |ev|
        ctx = context(ev)
        next [] unless WorkflowExpr.cond?(job["if"], ctx)
        (job["steps"] || []).select do |s|
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
puts "ok: no job holds actions: write while it checks out pull request or merge group code (#{files.size} workflows)" if found.empty?

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
    got = !violations([path]).empty?
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
  if violations([path]).empty?
    puts "ok: self-test: a job its if: keeps off pull requests passes"
  else
    puts "FAIL: self-test: a job its if: keeps off pull requests is flagged"
    failures += 1
  end
end

if failures.positive?
  puts "#{failures} failed"
  exit 1
end
puts "all passed"
