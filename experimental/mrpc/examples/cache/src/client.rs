pub mod cache {
    // The string specified here must match the proto package name
    mrpc::include_proto!("cache");
    // include!("../../../mrpc/src/codegen.rs");
}

use cache::cache_client::CacheClient;
use cache::Key;
use std::{thread, time::Duration, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = CacheClient::connect("localhost:5000")?;
    println!("client connected");
    let mut apple_count = 0;
    let mut banana_count = 0;
    let mut last_print_time = Instant::now();
    let interval = Duration::from_secs(5);
    let mut i = 0;
    loop {
        i = i ^ 1;
        let mut req = Key { key: "k1".into() };
        if i == 0 {
            req = Key { key: "k2".into() };
        }
        let reply = smol::block_on(client.get(req));
        match reply {
            Ok(reply) => {
                let reply = String::from_utf8_lossy(&reply.value);
                if reply == "v1" {
                    apple_count += 1;
                } else {
                    banana_count += 1;
                }
            }
            Err(e) => {
                println!("error: {}", e);
            }
        }

        thread::sleep(Duration::from_millis(250));

        let elapsed = Instant::now().duration_since(last_print_time);
        if elapsed >= interval {
            println!("v1 count: {}, v2 count: {}", apple_count, banana_count);
            last_print_time = Instant::now();
        }
    }
}
