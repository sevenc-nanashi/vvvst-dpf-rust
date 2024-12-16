use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub async fn pack<T: Serialize>(
    value: T,
    stream: impl tokio::io::AsyncWrite + std::marker::Unpin,
) -> anyhow::Result<()> {
    let serialized = bincode::serialize(&value).unwrap();
    let length = serialized.len() as u32;
    let length_buf = length.to_le_bytes();

    let mut stream = tokio::io::BufWriter::new(stream);
    tokio::io::AsyncWriteExt::write_all(&mut stream, &length_buf).await?;

    tokio::io::AsyncWriteExt::write_all(&mut stream, &serialized).await?;
    tokio::io::AsyncWriteExt::flush(&mut stream).await?;

    Ok(())
}

pub async fn unpack<T: DeserializeOwned>(
    stream: impl tokio::io::AsyncRead + std::marker::Unpin,
) -> anyhow::Result<T> {
    let mut length_buf = [0; 4];
    let mut stream = tokio::io::BufReader::new(stream);
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut length_buf).await?;

    let length = u32::from_le_bytes(length_buf);
    let mut serialized = vec![0; length as usize];

    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut serialized).await?;

    Ok(bincode::deserialize(&serialized)?)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToManagerMessage {
    Hello,
    Ping,
    Exit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToClientMessage {
    Hello,
    Pong,
    EngineStatus(EngineStatus),
    Error(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum EngineStatus {
    NotRunning,
    Running { port: u16 },
    Dead,
}
