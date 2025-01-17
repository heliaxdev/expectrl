#[cfg(not(feature = "async"))]
#[cfg(feature = "log")]
fn main() -> Result<(), expectrl::Error> {
    let mut p = expectrl::spawn("cat")?;
    p.set_log(std::io::stdout());
    p.send_line("Hello World")?;

    // reading doesn't apear here because
    // under the hood we use buffering and buffering and we are not able to see this buffer.
    p.expect("Hello World")?;

    Ok(())
}

#[cfg(any(not(feature = "log"), feature = "async"))]
fn main() {
    println!(
        "To run the example set necessary features `--no-default-features --features log,sync`"
    );

    std::process::exit(-1);
}
