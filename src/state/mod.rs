use anyhow::Result;
use serde::{Deserialize, Serialize};

mod v1;

pub use v1::*;

/// VSTに保存する用のパラメータ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum State {
    V1(V1State),
}

pub fn serialize_state(
    params: &PluginParams,
    critical_params: &CriticalPluginParams,
) -> Result<Vec<u8>> {
    let state = State::V1(V1State {
        params: serde_bytes::ByteBuf::from(bincode::serialize(params)?),
        critical_params: serde_bytes::ByteBuf::from(bincode::serialize(critical_params)?),
    });
    let bytes = bincode::serialize(&state)?;
    let compressed = zstd::encode_all(bytes.as_slice(), 0)?;
    Ok(compressed)
}

pub fn deserialize_state(data: &[u8]) -> Result<(PluginParams, CriticalPluginParams)> {
    let decompressed = zstd::decode_all(data)?;
    #[allow(unused_mut)]
    let mut state: State = bincode::deserialize(decompressed.as_slice())?;

    // TODO: マイグレーションがここに入る

    #[allow(irrefutable_let_patterns)]
    let State::V1(state) = state
    else {
        unreachable!()
    };
    Ok((
        bincode::deserialize(&state.params.into_vec())?,
        bincode::deserialize(&state.critical_params.into_vec())?,
    ))
}
