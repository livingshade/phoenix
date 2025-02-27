use serde::{Deserialize, Serialize};

type IResult<T> = Result<T, phoenix_api::Error>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Request {
    NewConfig(f32, u64),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ResponseKind {}

#[derive(Debug, Serialize, Deserialize)]
pub struct Response(pub IResult<ResponseKind>);
