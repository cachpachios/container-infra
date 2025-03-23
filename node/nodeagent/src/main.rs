use std::time::Duration;

fn main() {
    loop {
        println!("Running NodeAgent {}", env!("CARGO_PKG_VERSION"));
        std::thread::sleep(Duration::from_secs(5));
    }
}
