//! A stand-in for the agent that starts the guard, in the guard's parent-death tests: `parent_stub
//! <program> <args...>` starts the program with this process's stdin, stdout, stderr and
//! environment, writes its pid to the file `PARENT_STUB_CHILD_PID` names, and waits for it. The
//! test then SIGKILLs this process while it still holds the guard's stdin itself, so only the
//! guard's parent check can notice. Cargo builds it for the tests; it is never shipped.

#[expect(
    clippy::disallowed_methods,
    reason = "the test stub starts the guard as an agent would, and records its pid for the test"
)]
fn main() -> std::io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let program = args
        .next()
        .ok_or_else(|| std::io::Error::other("no program"))?;
    let mut child = std::process::Command::new(program).args(args).spawn()?;
    if let Some(file) = std::env::var_os("PARENT_STUB_CHILD_PID") {
        std::fs::write(file, child.id().to_string())?;
    }
    child.wait()?;
    Ok(())
}
