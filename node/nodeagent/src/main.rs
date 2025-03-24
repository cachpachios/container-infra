use std::time::Duration;

fn main() {
    println!("Hello world!");
    println!("This is NodeAgent, built statically and running hopefully inside a VM.");

    loop {
        std::thread::sleep(Duration::from_secs(1));
        println!("We are still running, but doing nothing... zzz");
    }
}
