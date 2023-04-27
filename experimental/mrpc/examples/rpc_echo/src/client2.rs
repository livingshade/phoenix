pub mod rpc_hello {
    // The string specified here must match the proto package name
    mrpc::include_proto!("rpc_hello");
    // include!("../../../mrpc/src/codegen.rs");
}

use env_logger;
use env_logger::{Builder, Env};
use rpc_hello::greeter_client::GreeterClient;
use rpc_hello::HelloRequest;
use std::{thread, time::Duration, time::Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::set_max_level(log::LevelFilter::Info);
    let env = Env::new().filter_or("RUST_LOG", "info");
    env_logger::init_from_env(env);

    let client = GreeterClient::connect("localhost:5000")?;
    log::info!("Client Connected!");

    let mut apple_count = 0;
    let mut banana_count = 0;
    let mut last_print_time = Instant::now();
    let mut last_send_time = Instant::now();
    let interval = Duration::from_secs(4);
    let sleep_interval = Duration::from_millis(250);
    let mut i = 0;
    loop {
        i = i ^ 1;
        let mut req = HelloRequest {
            name: "Apple".into(),
        };
        if i == 0 {
            req = HelloRequest {
                name: "Banana".into(),
            };
        }
        let reply = smol::block_on(client.say_hello(req));
        match reply {
            Ok(reply) => {
                let reply = String::from_utf8_lossy(&reply.message);
                if reply == "Hello Apple!" {
                    apple_count += 1;
                } else {
                    banana_count += 1;
                }
            }
            Err(_e) => {
                //println!("error: {}", e);
            }
        }

        thread::sleep(Duration::from_millis(250));

        let elapsed = Instant::now().duration_since(last_print_time);
        if elapsed >= interval {
            log::info!("Apple  count: {}", apple_count);
            log::info!("Banana count: {}", banana_count);
            apple_count = 0;
            banana_count = 0;
            last_print_time = Instant::now();
        }
    }
}
