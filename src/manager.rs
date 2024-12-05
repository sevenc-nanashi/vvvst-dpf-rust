use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub async fn pack<T: Serialize>(value: T, stream: impl tokio::io::AsyncWrite + std::marker::Unpin) {
    let serialized = bincode::serialize(&value).unwrap();
    let length = serialized.len() as u32;
    let length_buf = length.to_le_bytes();

    let mut stream = tokio::io::BufWriter::new(stream);
    tokio::io::AsyncWriteExt::write_all(&mut stream, &length_buf)
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::write_all(&mut stream, &serialized)
        .await
        .unwrap();
    tokio::io::AsyncWriteExt::flush(&mut stream).await.unwrap();
}

pub async fn unpack<T: DeserializeOwned>(
    stream: impl tokio::io::AsyncRead + std::marker::Unpin,
) -> T {
    let mut length_buf = [0; 4];
    let mut stream = tokio::io::BufReader::new(stream);
    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut length_buf)
        .await
        .unwrap();

    let length = u32::from_le_bytes(length_buf);
    let mut serialized = vec![0; length as usize];

    tokio::io::AsyncReadExt::read_exact(&mut stream, &mut serialized)
        .await
        .unwrap();

    bincode::deserialize(&serialized).unwrap()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    Hello(ClientType),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClientType {
    Vst,
    Manager,
}
