# A small evaluator for GitHub Actions expressions, for the workflow tests: literals, contexts
# (looked up through the caller's lambda), ==, !=, &&, ||, !, parentheses and the status and
# string functions. What it cannot know (needs.*, a step's outputs) is nil, so a condition on it
# reads as GitHub's would for an empty value; callers treat a job as reachable unless its `if:`
# is false.

require "json"

class WorkflowExpr
  TOKEN = /\s*(?:('(?:[^']|'')*')|(\d+(?:\.\d+)?)\b|(==|!=|&&|\|\||<=|>=|<|>|!|\(|\)|,)|([A-Za-z_][\w-]*(?:\.[\w*-]+|\[[^\]]*\])*))/m

  def self.truthy?(v)
    !(v.nil? || v == false || v == "" || v == 0)
  end

  # A workflow value: plain, "${{ expr }}" alone (its value), or text with expressions (a string).
  def self.evaluate(v, ctx)
    return v unless v.is_a?(String)
    if (m = v.strip.match(/\A\$\{\{(.*)\}\}\z/m)) && !m[1].include?("}}")
      return new(m[1], ctx).value
    end
    v.gsub(/\$\{\{(.*?)\}\}/m) { new(Regexp.last_match(1), ctx).value.to_s }
  end

  # An `if:`, which may leave out the ${{ }}.
  def self.cond?(v, ctx)
    return true if v.nil?
    v = "${{ #{v} }}" if v.is_a?(String) && !v.include?("${{")
    truthy?(evaluate(v, ctx))
  end

  # The context names an expression reads.
  def self.identifiers(src)
    src.to_s.scan(/\$\{\{(.*?)\}\}/m).flatten.flat_map do |e|
      e.gsub(/'(?:[^']|'')*'/, "").scan(/[A-Za-z_][\w-]*(?:\.[\w*-]+)+/)
    end.uniq
  end

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

  private

  def peek; @toks[@i]; end
  def take; t = @toks[@i]; @i += 1; t; end
  def op?(o); peek && peek[0] == :op && peek[1] == o; end

  def parse_or
    l = parse_and
    while op?("||")
      take
      r = parse_and
      l = WorkflowExpr.truthy?(l) ? l : r
    end
    l
  end

  def parse_and
    l = parse_cmp
    while op?("&&")
      take
      r = parse_cmp
      l = WorkflowExpr.truthy?(l) ? r : l
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
      l = o == "==" ? a == b : o == "!=" ? a != b : false
    end
    l
  end

  def parse_unary
    if op?("!")
      take
      return !WorkflowExpr.truthy?(parse_unary)
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
      # A function call. (Not a modifier `if` that assigns args in its condition: Ruby reads the
      # `args` in the body as a method call there, so every call raised NameError.)
      if op?("(")
        take
        return call(t[1], arguments)
      end
      { "true" => true, "false" => false, "null" => nil }.fetch(t[1]) { @ctx.call(t[1]) }
    end
  end

  def arguments
    list = []
    until op?(")")
      list << parse_or
      take if op?(",")
    end
    take
    list
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
