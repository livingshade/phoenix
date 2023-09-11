use std::env;
use std::path::{Path, PathBuf};

use clap::Parser;
use phoenix_api::engine::SchedulingMode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use ipc::control::{CommandSignalRequest, Request};
use ipc::unix::DomainSocket;

const MAX_MSG_LEN: usize = 65536;

const DEFAULT_PHOENIX_PREFIX: &str = "/tmp/phoenix";
const DEFAULT_PHOENIX_CONTROL: &str = "control.sock";

lazy_static::lazy_static! {
    static ref PHOENIX_PREFIX: PathBuf = {
        env::var("PHOENIX_PREFIX").map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_PREFIX), |p| {
            let path = PathBuf::from(p);
            assert!(path.is_dir(), "{path:?} is not a directly");
            path
        })
    };

    static ref PHOENIX_CONTROL_SOCK: PathBuf = {
        env::var("PHOENIX_CONTROL")
            .map_or_else(|_| PathBuf::from(DEFAULT_PHOENIX_CONTROL), PathBuf::from)
    };
}

#[derive(Debug, Clone, clap::Parser)]
#[command(name = "Phoenix addon manager")]
struct Opts {
    #[arg(long)]
    engine: String,
    #[arg(long)]
    param: String,
}

fn main() {
    let opts = Opts::parse();

    let uuid = Uuid::new_v4();
    let arg0 = env::args().next().unwrap();
    let appname = Path::new(&arg0).file_name().unwrap().to_string_lossy();

    let sock_path = PHOENIX_PREFIX.join(format!("phoenix-client-{}_{}.sock", appname, uuid));

    if sock_path.exists() {
        std::fs::remove_file(&sock_path).expect("remove_file");
    }
    let sock = DomainSocket::bind(sock_path).unwrap();

    let request = CommandSignalRequest {
        engine_type: opts.engine,
        param: opts.param,
    };
    let req = Request::CommandSignal(request);
    let buf = bincode::serialize(&req).unwrap();
    assert!(buf.len() < MAX_MSG_LEN);

    let service_path = PHOENIX_PREFIX.join(PHOENIX_CONTROL_SOCK.as_path());
    sock.send_to(&buf, &service_path).unwrap();
}
