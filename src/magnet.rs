use crate::InputError;
use data_encoding::{BASE32_NOPAD, HEXLOWER};
use url::Url;

#[derive(Debug, PartialEq, Eq, serde::Serialize)]
pub struct Magnet {
    info_hash: String,
    pub display_name: Option<String>,
}

impl Magnet {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        if input.len() > 16 * 1024 {
            return Err(InputError::TooLarge);
        }
        if input.trim() != input || input.chars().any(char::is_control) {
            return Err(InputError::InvalidMagnet);
        }
        let url = Url::parse(input).map_err(|_| InputError::InvalidMagnet)?;
        if url.scheme() != "magnet"
            || url.has_host()
            || !url.path().is_empty()
            || url.fragment().is_some()
        {
            return Err(InputError::InvalidMagnet);
        }
        let mut info_hash = None;
        let mut display_name = None;
        for (key, value) in url.query_pairs() {
            match key.as_ref() {
                "xt" => {
                    let topic = value.to_ascii_lowercase();
                    if topic.starts_with("urn:btmh:") {
                        return Err(InputError::UnsupportedFormat);
                    }
                    let hash = topic
                        .strip_prefix("urn:btih:")
                        .ok_or(InputError::UnsupportedFormat)?;
                    let bytes = match hash.len() {
                        40 => HEXLOWER.decode(hash.as_bytes()),
                        32 => BASE32_NOPAD.decode(hash.to_ascii_uppercase().as_bytes()),
                        _ => return Err(InputError::InvalidHash),
                    }
                    .map_err(|_| InputError::InvalidHash)?;
                    let normalized = HEXLOWER.encode(&bytes);
                    if info_hash
                        .as_ref()
                        .is_some_and(|existing| existing != &normalized)
                    {
                        return Err(InputError::ConflictingHash);
                    }
                    info_hash = Some(normalized);
                }
                "dn" if display_name.is_none() => display_name = Some(value.into_owned()),
                _ => {}
            }
        }
        Ok(Self {
            info_hash: info_hash.ok_or(InputError::InvalidHash)?,
            display_name,
        })
    }

    pub fn info_hash(&self) -> &str {
        &self.info_hash
    }
}
