use std::env;
use std::io::Read;
use std::process;
fn main() {
    let file = env::args().nth(1).unwrap();
    let mut cat = process::Command::new("lbzcat")
        .arg(file)
        .stdout(process::Stdio::piped())
        .spawn()
        .expect("failed to launch lbzcat");

    let mut buf: [u8; 16 * 1024] = [0; 16 * 1024];
    if let Some(ref mut stdout) = cat.stdout {
        while let Ok(n) = stdout.read(&mut buf) {
            //print!("{n}...");
            if n == 0 {
                break;
            }
        }
        println!();
    }
    let res = cat.wait().expect("Could not wait");
    assert!(res.success());
}
