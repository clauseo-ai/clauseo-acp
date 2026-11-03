// limit-warning's feed as the host sends it to a webview (U12): each `limits_view_changed` line, verbatim, that
// clauseo-host published to its limits subscription with fake_agent's `limit-warning` as claude-acp, a first read
// that fails, and the host clock on 2026-11-03 (1793715200000 ms), the day PR #228's Windows smoke ran. Captured
// with a one-off print in crates/host/tests/host/limits_host.rs (live_warning_publishes_the_limits_and_a_near_meter
// with the read failed and the clock advanced 43 days), not kept. The live_only line is the turn's usage_update:
// five_hour at 96%, `allowed_warning`, reset at the scenario's resetsAt.
export const LIMIT_WARNING_FEED: readonly string[] = [
  '{"limits_view_changed":{"claude":{"billing":"plan","windows":[],"at_limit_text":"When you reach a limit, Claude pauses until it resets.","freshness":"loading"}}}',
  '{"limits_view_changed":{"claude":{"billing":"plan","windows":[],"at_limit_text":"When you reach a limit, Claude pauses until it resets.","freshness":"unavailable"}}}',
  '{"limits_view_changed":{"claude":{"billing":"plan","meter":{"tone":"near","used_percent":96.0,"window_label":"5-hour limit","resets_at":4102444800000},"windows":[{"id":"five_hour","kind":"session","label":"5-hour limit","used_percent":96.0,"tone":"near","resets_at":4102444800000,"freshness":"fresh"}],"at_limit_text":"When you reach a limit, Claude pauses until it resets.","freshness":"live_only","checked_at":1793715200000,"next_read_at":4102444800000}}}',
];
