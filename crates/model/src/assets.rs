//! The document's images (docs/render/look.md#textures): the materials' textures, each stored
//! once, by the hash of its bytes, as it came (not re-encoded).

use std::sync::Arc;

use base64::Engine;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// What an image is encoded in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImageKind {
    Png,
    Jpeg,
}

impl ImageKind {
    /// Read from its first bytes; none for another encoding.
    pub fn of(bytes: &[u8]) -> Option<ImageKind> {
        match bytes {
            [0x89, b'P', b'N', b'G', ..] => Some(ImageKind::Png),
            [0xFF, 0xD8, 0xFF, ..] => Some(ImageKind::Jpeg),
            _ => None,
        }
    }

    /// Its file extension, without the dot.
    pub fn extension(self) -> &'static str {
        match self {
            ImageKind::Png => "png",
            ImageKind::Jpeg => "jpg",
        }
    }
}

/// An image of the document, its bytes shared (the history, the clipboard).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Asset {
    pub kind: ImageKind,
    pub bytes: Arc<[u8]>,
}

impl Asset {
    /// The image of `bytes`, a PNG or a JPEG; none for anything else.
    pub fn new(bytes: impl Into<Arc<[u8]>>) -> Option<Asset> {
        let bytes = bytes.into();
        Some(Asset { kind: ImageKind::of(&bytes)?, bytes })
    }

    /// Its name among the document's assets: 128 bits of the BLAKE3 hash of its bytes, in
    /// hexadecimal (as a B-rep's entry, docs/io/file-format.md).
    pub fn id(&self) -> String {
        id_of(&self.bytes)
    }
}

/// The name of bytes: 128 bits of their BLAKE3 hash, in hexadecimal.
pub(crate) fn id_of(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex()[..32].to_owned()
}

/// Inline (an operation, the document's one JSON): its kind and its bytes in base64.
#[derive(Serialize, Deserialize)]
struct Inline {
    kind: ImageKind,
    base64: String,
}

impl Serialize for Asset {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Inline { kind: self.kind, base64: base64::engine::general_purpose::STANDARD.encode(&self.bytes) }.serialize(s)
    }
}

impl<'de> Deserialize<'de> for Asset {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let i = Inline::deserialize(d)?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(&i.base64).map_err(serde::de::Error::custom)?;
        Asset::new(bytes).filter(|a| a.kind == i.kind).ok_or_else(|| serde::de::Error::custom("not a PNG nor a JPEG image"))
    }
}
