//! The arguments in a macOS argument area as `sysctl KERN_PROCARGS2` gives it (`macos.rs`), as
//! xnu lays it out (`kern_sysctl.c`, `sysctl_procargsx`; `kern_exec.c`, `exec_extract_strings`):
//! the argument count, a native-endian `int`; the executable's path, ended by a NUL; NULs padding
//! the path to a multiple of the pointer size from its start; then `argc` arguments, each ended
//! by a NUL; then the environment and more. Only the arguments count: an environment entry that
//! happened to read `--user-data-dir=<profile>` must not match. Plain bytes in, plain bytes out,
//! so it is tested on every OS.

/// The pointer size xnu pads the executable's path to: 8 on every 64-bit Mac.
const WORD: usize = 8;

/// The `argc` arguments of `area`, each ended by a NUL, as `/proc/<pid>/cmdline` gives them on
/// Linux; empty when the count is 0 or negative. `None` when `area` is shorter than its count,
/// or than any argument it says it holds.
///
/// The arguments start where the path's padding ends, so an empty first argument (a lone NUL
/// right after the padding) is read as one, and the count still ends at the last argument, never
/// in the environment. An area whose padding is shorter than that rule says (the NULs end
/// before it) starts at its first byte past the NULs instead, as `ps` reads it.
pub(crate) fn arguments(area: &[u8]) -> Option<&[u8]> {
    let count: [u8; 4] = area.get(..4)?.try_into().ok()?;
    let argc = usize::try_from(i32::from_ne_bytes(count)).unwrap_or(0);
    let rest = &area[4..];
    if argc == 0 {
        return Some(&[]);
    }
    let path_end = rest.iter().position(|byte| *byte == 0)?;
    let padded = (path_end + 1).next_multiple_of(WORD);
    let past_nuls = path_end + rest.get(path_end..)?.iter().position(|byte| *byte != 0)?;
    let first = padded.min(past_nuls);
    let mut end = first;
    for _ in 0..argc {
        end += rest.get(end..)?.iter().position(|byte| *byte == 0)? + 1;
    }
    rest.get(first..end)
}

#[cfg(test)]
mod tests {
    use super::{WORD, arguments};

    /// An argument area as xnu writes it: `argc`, the path padded with NULs to a multiple of
    /// [`WORD`], the arguments, then the environment.
    fn area(argc: i32, path: &str, args: &[&str], env: &[&str]) -> Vec<u8> {
        let mut area = argc.to_ne_bytes().to_vec();
        area.extend_from_slice(path.as_bytes());
        let padded = (path.len() + 1).next_multiple_of(WORD);
        area.extend(std::iter::repeat_n(0, padded - path.len()));
        for text in args.iter().chain(env) {
            area.extend_from_slice(text.as_bytes());
            area.push(0);
        }
        area
    }

    #[test]
    fn only_the_arguments_are_read_never_the_path_or_the_environment() {
        let flag = "--user-data-dir=/p";
        let given = area(2, "/Apps/Chrome", &["chrome", "--headless"], &[flag]);
        assert_eq!(arguments(&given), Some(&b"chrome\0--headless\0"[..]));
        let given = area(1, flag, &["chrome"], &[]);
        assert_eq!(arguments(&given), Some(&b"chrome\0"[..]));
        // A path that fills its word exactly gets a whole word of NULs.
        let given = area(2, "/c234567", &["chrome", flag], &["HOME=/h"]);
        assert_eq!(
            arguments(&given),
            Some(&b"chrome\0--user-data-dir=/p\0"[..])
        );
    }

    /// An empty first argument is one argument, so the count never reaches the environment.
    #[test]
    fn an_empty_first_argument_is_counted_and_the_environment_is_not_read() {
        let given = area(2, "/c", &["", "--x"], &["--user-data-dir=/p"]);
        assert_eq!(arguments(&given), Some(&b"\0--x\0"[..]));
        let given = area(3, "/Apps/Chrome", &["", "", "--y"], &["--user-data-dir=/p"]);
        assert_eq!(arguments(&given), Some(&b"\0\0--y\0"[..]));
    }

    /// Padding shorter than the rule (not what xnu writes): read from the first byte past it.
    #[test]
    fn short_padding_starts_at_the_first_argument_byte() {
        let mut given = 1_i32.to_ne_bytes().to_vec();
        given.extend_from_slice(b"/c\0chrome\0A=1\0");
        assert_eq!(arguments(&given), Some(&b"chrome\0"[..]));
    }

    #[test]
    fn a_count_of_zero_or_less_reads_no_argument() {
        assert_eq!(arguments(&area(0, "/c", &[], &["A=1"])), Some(&[][..]));
        assert_eq!(arguments(&area(-1, "/c", &["x"], &[])), Some(&[][..]));
    }

    #[test]
    fn a_cut_short_area_reads_as_none() {
        assert_eq!(arguments(&[1, 0]), None);
        assert_eq!(arguments(&area(3, "/c", &["a", "b"], &[])), None);
        assert_eq!(arguments(&area(1, "/c", &[], &[])), None);
        let mut cut = area(1, "/c", &["chrome"], &[]);
        cut.pop();
        assert_eq!(arguments(&cut), None);
    }
}
