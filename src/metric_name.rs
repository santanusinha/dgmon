// SPDX-License-Identifier: Apache-2.0
//! Shared metric-name helpers used by both the Prometheus renderer and
//! the tsink storage writer.

/// Convert a Prometheus metric name into a valid Prometheus metric name.
/// Replaces characters that are not alphanumeric or underscore with
/// underscores, strips a known engine prefix (`vllm:` or `sglang:`), and
/// prefixes with `dgmon_inference_` to avoid collisions.
pub fn sanitize_metric_name(name: &str) -> String {
    let stripped = strip_engine_prefix(name);
    let mut out = String::with_capacity(stripped.len() + 16);
    out.push_str("dgmon_inference_");
    for c in stripped.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Strip a known engine prefix (`vllm:` or `sglang:`) from a raw metric name.
/// Returns the name unchanged when no engine prefix is present.
pub fn strip_engine_prefix(name: &str) -> &str {
    for prefix in ["vllm:", "sglang:"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return rest;
        }
    }
    name
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_invalid_chars() {
        assert_eq!(sanitize_metric_name("vllm:num_requests_running"), "dgmon_inference_num_requests_running");
        assert_eq!(sanitize_metric_name("sglang:num_requests_waiting"), "dgmon_inference_num_requests_waiting");
        assert_eq!(sanitize_metric_name("vllm:time_to_first_token_seconds_bucket{le=\"0.01\"}"), "dgmon_inference_time_to_first_token_seconds_bucket_le__0_01__");
    }

    #[test]
    fn sanitize_handles_leading_digits() {
        assert_eq!(sanitize_metric_name("123abc"), "dgmon_inference_123abc");
    }

    #[test]
    fn sanitize_handles_empty() {
        assert_eq!(sanitize_metric_name(""), "dgmon_inference_");
    }

    #[test]
    fn strip_engine_prefix_removes_known_prefixes() {
        assert_eq!(strip_engine_prefix("vllm:foo"), "foo");
        assert_eq!(strip_engine_prefix("sglang:bar"), "bar");
        assert_eq!(strip_engine_prefix("no_prefix"), "no_prefix");
    }
}
