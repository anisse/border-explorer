use memchr::memmem;
use std::env;
use std::io::{BufRead, BufReader};
use std::process;

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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = env::args().nth(1).expect("not enough arguments");
    let needle = env::args().nth(2).expect("not enough arguments");
    let mut cat = lbzcat(&file)?;
    if let Some(ref mut stdout) = cat.stdout {
        BufReader::new(stdout)
            .lines()
            .map_while(Result::ok)
            .for_each(|line| {
                if let Some(l) = grep(&line, &needle) {
                    print!("{l}");
                }
            });
    }

    let res = cat.wait().expect("Could not wait");
    assert!(res.success());
    Ok(())
}
