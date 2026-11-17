//! `IntOp` with `+`, `-`, `&` or `|`, the operators the hooks and the preflight use.

use super::Machine;

/// Sets `var` to `a op b`, both expanded and read as integers.
pub(super) fn int_op(machine: &mut Machine, var: &str, a: &str, op: &str, b: &str) {
    let a: i64 = machine.expand(a).parse().unwrap();
    let b: i64 = machine.expand(b).parse().unwrap();
    let value = match op {
        "+" => a + b,
        "-" => a - b,
        "&" => a & b,
        _ => a | b,
    };
    machine.set(var, value.to_string());
}
