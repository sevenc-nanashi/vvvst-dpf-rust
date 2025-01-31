use anyhow::Result;
use serde::{Deserialize, Serialize};

pub struct Voice {
    pub bytes: Vec<u8>,
    pub sample_rate: f32,
    pub samples_len: usize,
}
impl Serialize for Voice {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bytes(&self.to_vec())
    }
}
impl<'de> Deserialize<'de> for Voice {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let bytes = serde_bytes::ByteBuf::deserialize(deserializer)?;
        Ok(Voice::new(bytes.to_vec()).map_err(serde::de::Error::custom)?)
    }
}
impl std::fmt::Debug for Voice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Voice").finish()
    }
}
impl Clone for Voice {
    fn clone(&self) -> Self {
        Voice::new(self.to_vec()).unwrap()
    }
}
impl Voice {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let mut reader =
            wav_io::reader::Reader::from_vec(bytes.clone()).map_err(anyhow::Error::msg)?;
        let header = reader.read_header().map_err(anyhow::Error::msg)?;
        let samples_len = reader.get_samples_f32().map_err(anyhow::Error::msg)?.len();

        Ok(Voice {
            bytes,
            sample_rate: header.sample_rate as f32,
            samples_len,
        })
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.bytes.clone()
    }

    pub fn duration(&self) -> f32 {
        (self.samples_len as f32) / (self.sample_rate as f32)
    }

    pub fn reader(&self) -> wav_io::reader::Reader {
        wav_io::reader::Reader::from_vec(self.bytes.clone())
            .expect("unreachable: bytes are validated in constructor")
    }
}
