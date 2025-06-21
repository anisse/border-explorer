use memchr::memmem;
use std::env;
use std::io::{BufRead, BufReader};
use std::process;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = env::args().nth(1).ok_or(ArgError {})?;
    let needle = env::args().nth(2).ok_or(ArgError {})?;
    let mut cat = lbzcat(&file)?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .for_each(|line| {
                if let Some(l) = grep(&line, &needle) {
                    println!("{l}");
                }
            });
    }

    let res = cat.wait().map_err(|e| format!("Could not wait: {e}"))?;
    res.success().then_some(()).ok_or("failure")?;
    Ok(())
}

struct ArgError {}
impl std::fmt::Display for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "not enough arguments")
    }
}
impl std::fmt::Debug for ArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::error::Error for ArgError {}

fn lbzcat(file: &str) -> Result<process::Child, String> {
    let cat = process::Command::new("lbzcat")
        .arg(file)
        .stdout(process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to launch lbzcat: {e}"))?;

    Ok(cat)
}

fn grep<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    if memmem::find(line.as_ref(), needle.as_ref()).is_some() {
        return Some(line);
    }
    None
}
