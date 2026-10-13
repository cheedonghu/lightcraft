//! Identity shared by the current bridge and future round adapters.
//! Runtime scope is never a durable library identifier: archives belong to the
//! library directory and photo record, while generation only rejects late work.
use lightcraft_catalog::PhotoId;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhotoScope {
    pub library_generation: u64,
    pub photo: PhotoId,
}

/// Independent of admission, backend ids, connection and submission counters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LocalRoundId(String);

impl LocalRoundId {
    pub fn new() -> Result<Self, String> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let mut bytes = [0u8; 16];
            getrandom::fill(&mut bytes).map_err(|_| "无法生成 AI 轮次身份。".to_owned())?;
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            Ok(Self(format!("local-{hex}")))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Err("AI 助手目前仅支持桌面版。".into())
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// All four members must match before an asynchronous result can affect a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoundIdentity {
    pub scope: PhotoScope,
    pub local_round_id: LocalRoundId,
    pub submission_seq: u64,
    pub connection_generation: u64,
}
