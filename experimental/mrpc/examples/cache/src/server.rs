//! This code defines a simple mRPC server that implements the Greeter service.
//! It listens for incoming "Hello" requests and sends back a greeting message.

// Import the auto-generated code for the "rpc_hello" module from the Proto file.
pub mod cache {
    // The string specified here must match the proto package name
    mrpc::include_proto!("cache");
}

use cache::cache_server::{Cache, CacheServer};
use cache::{Key, Value};

use mrpc::{RRef, WRef};

#[derive(Debug, Default)]
struct MyCache;

// Implement the Greeter trait for MyGreeter using async_trait.
#[mrpc::async_trait]
impl Cache for MyCache {
    // Define the say_hello function which takes an RRef<HelloRequest>
    // and returns a Result with a WRef<HelloReply>.
    async fn get(&self, request: RRef<Key>) -> Result<WRef<Value>, mrpc::Status> {
        // Log the received request.
        eprintln!("request: {:?}", request);

        // Create a new HelloReply with a greeting message.
        let message = format!("Hello {}!", String::from_utf8_lossy(&request.key));
        let reply = WRef::new(Value {
            value: message.as_bytes().into(),
        });

        Ok(reply)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Start the server, binding it to port 5000.
    smol::block_on(async {
        let mut server = mrpc::stub::LocalServer::bind("0.0.0.0:5000")?;
        println!("binded");
        // Add the Greeter service to the server using the custom MyGreeter implementation.
        server
            .add_service(CacheServer::new(MyCache::default()))
            .serve()
            .await?;
        eprintln!("server stopped");
        Ok(())
    })
}
