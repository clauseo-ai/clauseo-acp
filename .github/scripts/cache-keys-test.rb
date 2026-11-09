# Checks the workflows' cache rules, from the workflow files themselves. fast-lane runs it.
# Run it from anywhere: ruby .github/scripts/cache-keys-test.rb
#
# 1. No job that a pull request, a pull request review or a merge group can run holds
#    `actions: write`: in a merge group a fork's code runs with what the job grants, and the token
#    can be read from the runner's memory. A job counts as reachable unless its `if:` is false for
#    every such event (evaluated below; what cannot be known, such as needs.*, counts as reachable).
# 2. Every key a Swatinem/rust-cache step saves (prefix-key, key, shared-key, the job id and the
#    runner's OS and arch, expanded over the job's matrix for main's scheduled and on-demand runs)
#    matches exactly one line of cache-families.txt, and no rust-cache step saves in a pull
#    request or a merge group.
# 3. Every other save goes through ./.github/actions/cache-save-main, with a key from one of the
#    known key steps; the key each of those writes (its `echo "key=..."` line, with sample values)
#    matches exactly one line. No setup-node step turns its own cache on.
# Ruby, for its YAML parser, which the runners and macOS have.

require "yaml"
require "json"

ROOT = File.expand_path("../..", __dir__)
Dir.chdir(ROOT)
$failures = 0
def fail!(msg)
  puts "FAIL: #{msg}"
  $failures += 1
end

def ok(msg)
  puts "ok: #{msg}"
end

FAMILIES = File.readlines(".github/scripts/cache-families.txt")
  .map(&:strip).reject { |l| l.empty? || l.start_with?("#") }
  .map { |l| n, re = l.split(/\s+/); [n, Regexp.new(re)] }

def families_of(key)
  FAMILIES.select { |_, re| re.match?(key) }.map(&:first)
end

def check_key(key, where)
  f = families_of(key)
  fail!("#{where}: key #{key} matches #{f.size} families in cache-families.txt, not 1 (#{f.join(", ")})") if f.size != 1
  f.size == 1
end

# --- A small evaluator for GitHub expressions --------------------------------------------------

TOKEN = /\s*(?:('(?:[^']|'')*')|(\d+(?:\.\d+)?)\b|(==|!=|&&|\|\||<=|>=|<|>|!|\(|\)|,)|([A-Za-z_][\w-]*(?:\.[\w*-]+|\[[^\]]*\])*))/m

class Expr
  def initialize(src, ctx)
    @toks = []
    s = src.strip
    until s.empty?
      m = TOKEN.match(s)
      raise "cannot read #{s[0, 40].inspect}" unless m && m.begin(0).zero?
      @toks << (m[1] ? [:str, m[1][1..-2].gsub("''", "'")] : m[2] ? [:num, m[2].to_f] : m[3] ? [:op, m[3]] : [:id, m[4]])
      s = s[m.end(0)..-1].lstrip
    end
    @ctx = ctx
    @i = 0
  end

  def value
    v = parse_or
    raise "trailing #{@toks[@i].inspect}" if @i < @toks.size
    v
  end

  def self.truthy?(v)
    !(v.nil? || v == false || v == "" || v == 0)
  end

  private

  def peek; @toks[@i]; end
  def take; t = @toks[@i]; @i += 1; t; end
  def op?(o); peek && peek[0] == :op && peek[1] == o; end

  def parse_or
    l = parse_and
    while op?("||")
      take
      r = parse_and
      l = Expr.truthy?(l) ? l : r
    end
    l
  end

  def parse_and
    l = parse_cmp
    while op?("&&")
      take
      r = parse_cmp
      l = Expr.truthy?(l) ? r : l
    end
    l
  end

  def parse_cmp
    l = parse_unary
    while peek && peek[0] == :op && %w[== != < > <= >=].include?(peek[1])
      o = take[1]
      r = parse_unary
      a = l.is_a?(String) ? l.downcase : l
      b = r.is_a?(String) ? r.downcase : r
      l = case o
          when "==" then a == b
          when "!=" then a != b
          else false
          end
    end
    l
  end

  def parse_unary
    if op?("!")
      take
      return !Expr.truthy?(parse_unary)
    end
    parse_primary
  end

  def parse_primary
    t = take
    case t[0]
    when :str, :num then t[1]
    when :op
      raise "unexpected #{t[1]}" unless t[1] == "("
      v = parse_or
      take
      v
    when :id
      if op?("(")
        take
        args = []
        until op?(")")
          args << parse_or
          take if op?(",")
        end
        take
        call(t[1], args)
      else
        case t[1]
        when "true" then true
        when "false" then false
        when "null" then nil
        else @ctx.call(t[1])
        end
      end
    end
  end

  def call(name, args)
    case name.downcase
    when "always", "success" then true
    when "cancelled", "failure" then false
    when "fromjson" then args[0].is_a?(String) ? JSON.parse(args[0]) : nil
    when "contains" then args[0].respond_to?(:include?) && !args[1].nil? && args[0].include?(args[1])
    when "startswith" then args[0].is_a?(String) && args[0].downcase.start_with?(args[1].to_s.downcase)
    when "format" then args[0].to_s.gsub(/\{(\d+)\}/) { args[Regexp.last_match(1).to_i + 1].to_s }
    end
  end
end

# A value from a workflow: a plain value, "${{ expr }}" alone (its value), or text with
# expressions in it (a string).
def evaluate(v, ctx)
  return v unless v.is_a?(String)
  if (m = v.strip.match(/\A\$\{\{(.*)\}\}\z/m)) && !m[1].include?("}}")
    return Expr.new(m[1], ctx).value
  end
  v.gsub(/\$\{\{(.*?)\}\}/m) { Expr.new(Regexp.last_match(1), ctx).value.to_s }
end

def cond?(v, ctx)
  return true if v.nil?
  v = "${{ #{v} }}" if v.is_a?(String) && !v.include?("${{")
  Expr.truthy?(evaluate(v, ctx))
end

def context(event, matrix = {}, runner_os = nil)
  ref = case event
        when "merge_group" then "refs/heads/gh-readonly-queue/main/pr-1-0000"
        when /\Apull_request/ then "refs/pull/1/merge"
        else "refs/heads/main"
        end
  lambda do |id|
    case id
    when "github.event_name" then event
    when "github.ref" then ref
    when "runner.os" then runner_os
    when /\Amatrix\.(.+)\z/ then matrix[Regexp.last_match(1)]
    when /\Aenv\./ then "true" # a job's switches (BUILD, RUN, UI) as on a run that builds
    end
  end
end

# --- The workflows -------------------------------------------------------------------------------

PR_EVENTS = %w[pull_request pull_request_target pull_request_review pull_request_review_comment merge_group].freeze
# The jobs that hold actions: write, to cancel a pull request's runs for an older head: the two
# cancel jobs (#737, #760), which run no pull request code and no uses: step. workflow-token-test.rb
# checks their shape and keeps the same list; a new entry needs its own review there.
ALLOWED_ACTIONS_WRITE = {
  ".github/workflows/e2e-windows.yml" => ["e2e-cancel-stale"],
  ".github/workflows/windows-pr-tests.yml" => ["windows-cancel-stale"],
}.freeze

def triggers(doc)
  on = doc.key?("on") ? doc["on"] : doc[true]
  case on
  when String then [on]
  when Array then on
  when Hash then on.keys
  else []
  end
end

def actions_write?(perms)
  perms == "write-all" || (perms.is_a?(Hash) && perms["actions"] == "write")
end

def runner_of(label)
  case label.to_s
  when /\Aubuntu/ then %w[Linux x64 Linux]
  when /\Amacos/ then %w[Darwin arm64 macOS]
  when "windows-11-arm" then %w[Windows_NT arm64 Windows]
  when /\Awindows/ then %w[Windows_NT x64 Windows]
  end
end

def matrix_combos(job, ctx)
  m = job.dig("strategy", "matrix")
  return [{}] unless m
  m = evaluate(m, ctx) if m.is_a?(String)
  include = m["include"]
  include = evaluate(include, ctx) if include.is_a?(String)
  axes = m.reject { |k, _| %w[include exclude].include?(k) }
  base = axes.reduce([{}]) do |acc, (k, vals)|
    vals = evaluate(vals, ctx) if vals.is_a?(String)
    acc.product(Array(vals)).map { |h, v| h.merge(k => v) }
  end
  base = [] if axes.empty? && include
  base + Array(include)
end

workflows = Dir[".github/workflows/*.{yml,yaml}"].sort
docs = workflows.map { |f| [f, YAML.safe_load(File.read(f), aliases: true)] }

# 1. actions: write
reachable_write = 0
docs.each do |file, doc|
  events = triggers(doc) & PR_EVENTS
  next if events.empty?
  (doc["jobs"] || {}).each do |id, job|
    perms = job.key?("permissions") ? job["permissions"] : doc["permissions"]
    next unless actions_write?(perms)
    reach = events.select { |ev| cond?(job["if"], context(ev)) }
    next if reach.empty?
    if Array(ALLOWED_ACTIONS_WRITE[file]).include?(id)
      puts "note: #{file} job #{id} holds actions: write on #{reach.join(", ")} (it predates this rule)"
      next
    end
    fail!("#{file}: job #{id} holds actions: write and runs on #{reach.join(", ")}")
    reachable_write += 1
  end
end
ok("no new job a pull request, review or merge group runs holds actions: write") if reachable_write.zero?

# 2. rust-cache keys
rust_keys = []
docs.each do |file, doc|
  (doc["jobs"] || {}).each do |id, job|
    steps = (job["steps"] || []).select { |s| s["uses"].to_s.start_with?("Swatinem/rust-cache@") }
    next if steps.empty?
    (%w[schedule workflow_dispatch] + %w[pull_request merge_group]).each do |ev|
      matrix_combos(job, context(ev)).each do |combo|
        ctx0 = context(ev, combo)
        next unless cond?(job["if"], ctx0)
        os_type, arch, runner_os = runner_of(evaluate(job["runs-on"], ctx0))
        next unless os_type
        ctx = context(ev, combo, runner_os)
        steps.each do |step|
          next unless cond?(step["if"], ctx)
          with = step["with"] || {}
          save = with.fetch("save-if", true)
          saves = save == false || save.to_s == "false" ? false : Expr.truthy?(evaluate(save.to_s, ctx))
          next unless saves
          if PR_EVENTS.include?(ev)
            fail!("#{file}: job #{id}'s rust-cache saves on #{ev} (#{combo})")
            next
          end
          prefix = evaluate(with["prefix-key"] || "v0-rust", ctx)
          key = if with["shared-key"]
                  "#{prefix}-#{evaluate(with["shared-key"], ctx)}"
                else
                  k = with["key"] ? "-#{evaluate(with["key"], ctx)}" : ""
                  "#{prefix}#{k}-#{id}"
                end
          rust_keys << ["#{key}-#{os_type}-#{arch}-0123abcd-89abcdef", "#{file} job #{id}"]
        end
      end
    end
  end
end
rust_keys.uniq!
bad = rust_keys.reject { |k, where| check_key(k, where) }
ok("each of the #{rust_keys.size} keys rust-cache saves matches one family: #{rust_keys.map { |k, _| k.sub(/-0123abcd-89abcdef\z/, "") }.join(", ")}") if bad.empty?

# 3. The other saves
BUILDERS = {
  "${{ steps.pnpm-store.outputs.key }}" => ".github/actions/pnpm-store/action.yml",
  "${{ steps.cache-key.outputs.key }}" => ".github/actions/install-pinned-tools/tool-cache.sh",
  "${{ steps.key.outputs.key }}" => ".github/actions/install-tauri-linux-deps/action.yml",
}.freeze
files = workflows + Dir[".github/actions/**/action.{yml,yaml}"].sort
step_lists = files.flat_map do |f|
  doc = YAML.safe_load(File.read(f), aliases: true) || {}
  lists = (doc["jobs"] || {}).values.map { |j| j["steps"] || [] }
  lists << doc.dig("runs", "steps") if doc.dig("runs", "steps")
  lists.map { |l| [f, l] }
end
used = {}
step_lists.each do |f, steps|
  steps.each do |s|
    uses = s["uses"].to_s
    if uses.match?(%r{\Aactions/cache(/save)?@}) && f != ".github/actions/cache-save-main/action.yml"
      fail!("#{f}: #{uses} saves outside ./.github/actions/cache-save-main")
    end
    if uses.start_with?("actions/setup-node@") && s.dig("with", "cache").to_s != "" && s.dig("with", "cache").to_s != "false"
      fail!("#{f}: setup-node's cache: #{s.dig("with", "cache")} saves on every ref; use ./.github/actions/pnpm-store")
    end
    if uses == "./.github/actions/cache-save-main"
      key = s.dig("with", "key").to_s
      if BUILDERS.key?(key)
        used[key] = true
      else
        fail!("#{f}: cache-save-main saves key #{key.inspect}, from no known key step")
      end
    end
  end
end
# The package lists install-tauri-linux-deps is given, for its sample keys.
extras = step_lists.flat_map do |_, steps|
  steps.select { |s| s["uses"].to_s == "./.github/actions/install-tauri-linux-deps" }.map { |s| s.dig("with", "extra-packages").to_s }
end
docs.each { |_, d| (d["jobs"] || {}).each_value { |j| Array(j.dig("strategy", "matrix", "include")).each { |i| extras << i["extra"].to_s if i.is_a?(Hash) && i.key?("extra") } } }
lists = extras.reject { |e| e.include?("${{") }.uniq.map do |e|
  (["libwebkit2gtk-4.1-dev"] + e.split).uniq.sort.join("+")
end
SAMPLES = {
  "RUNNER_OS" => %w[Linux Windows macOS], "arch" => %w[x64 arm64], "LOCK_HASH" => ["a" * 64],
  "RUNNER_ARCH" => %w[X64 ARM64], "ImageOS" => %w[ubuntu22 ubuntu24 macos15 win25 win11-arm64],
  "ImageVersion" => ["20261101.347.1"], "list" => lists.map { |l| "#{l}+" },
  "names" => ["cargo-nextest", "cargo-deny+cargo-nextest+gitleaks+lefthook"], "hash" => ["0123456789abcdef"],
}.freeze
sample_count = 0
BUILDERS.each do |expr, src|
  next unless used[expr]
  line = File.read(src)[/echo "key=([^"]+)"/, 1]
  unless line
    fail!("#{src}: no `echo \"key=...\"` line to read its key from")
    next
  end
  vars = line.scan(/\$\{?(\w+)/).flatten.uniq
  missing = vars - SAMPLES.keys
  unless missing.empty?
    fail!("#{src}: no sample value for #{missing.join(", ")}")
    next
  end
  vars.map { |v| SAMPLES[v].map { |x| [v, x] } }.reduce([[]]) { |a, b| a.product(b).map(&:flatten) }.each do |flat|
    vals = Hash[*flat]
    key = line.gsub(/\$\{(\w+)(:-[^}]*|%[^}]*)?\}|\$(\w+)/) do
      name = Regexp.last_match(1) || Regexp.last_match(3)
      v = vals[name]
      mod = Regexp.last_match(2).to_s
      mod.start_with?("%") ? v.sub(/#{Regexp.escape(mod[1..-1])}\z/, "") : v
    end
    check_key(key, src)
    sample_count += 1
  end
end
ok("every save goes through cache-save-main with a known key step; #{sample_count} sample keys of those steps each match one family (debs lists: #{lists.join(", ")})")

if $failures.positive?
  puts "#{$failures} failed"
  exit 1
end
puts "all passed"
